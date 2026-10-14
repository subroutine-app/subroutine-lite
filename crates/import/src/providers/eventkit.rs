use std::fmt::Write;

use anyhow::Result;
use chrono::{DateTime, Local, Timelike, Utc, Weekday};
use chronoutil::RelativeDuration;
use objc2_foundation::{NSOperatingSystemVersion, NSProcessInfo};
use subroutine_core::{Action, Event, Marker, Recurrence, RecurrenceRule, SchedulePoint};

use crate::provider::{Provider, SyncOutcome};

const DEFAULT_FUTURE_DAYS: i64 = 90;

// EventKit managers are !Send + !Sync, not main-thread-only. Construct, use,
// and drop them within one synchronous background job with an autorelease pool;
// never hold them across await points, and return only owned results.
pub struct EventKitProvider {
    events: Option<eventkit::EventsManager>,
    reminders: Option<eventkit::RemindersManager>,
    events_enabled: bool,
    reminders_enabled: bool,
    future_days: i64,
    accepted_events_only: bool,
}

impl Default for EventKitProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl EventKitProvider {
    pub fn availability() -> Result<()> {
        check_macos_version(NSProcessInfo::processInfo().operatingSystemVersion())
    }

    pub fn new() -> Self {
        Self {
            events: None,
            reminders: None,
            events_enabled: true,
            reminders_enabled: true,
            future_days: DEFAULT_FUTURE_DAYS,
            accepted_events_only: false,
        }
    }

    pub fn with_future_days(mut self, days: i64) -> Self {
        self.future_days = days.max(1);
        self
    }

    pub fn accepted_events_only(mut self) -> Self {
        self.accepted_events_only = true;
        self
    }

    pub fn reminders_only(mut self) -> Self {
        self.events_enabled = false;
        self
    }

    pub fn events_only(mut self) -> Self {
        self.reminders_enabled = false;
        self
    }

    fn events_manager(&mut self) -> &eventkit::EventsManager {
        self.events.get_or_insert_with(eventkit::EventsManager::new)
    }

    fn reminders_manager(&mut self) -> &eventkit::RemindersManager {
        self.reminders
            .get_or_insert_with(eventkit::RemindersManager::new)
    }
}

impl Provider for EventKitProvider {
    fn id(&self) -> &'static str {
        "eventkit"
    }

    fn name(&self) -> &'static str {
        "Apple Calendar & Reminders"
    }

    fn ensure_authorized(&mut self) -> Result<()> {
        Self::availability()?;
        tracing::info!(
            "requesting EventKit authorization (events: {}, reminders: {})",
            self.events_enabled,
            self.reminders_enabled,
        );
        if self.events_enabled {
            self.events_manager().ensure_authorized().map_err(|e| {
                tracing::warn!("EventKit calendar authorization denied: {e}");
                anyhow::anyhow!("EventKit calendar authorization: {e}")
            })?;
        }
        if self.reminders_enabled {
            self.reminders_manager().ensure_authorized().map_err(|e| {
                tracing::warn!("EventKit reminders authorization denied: {e}");
                anyhow::anyhow!("EventKit reminders authorization: {e}")
            })?;
        }
        tracing::debug!("EventKit authorization complete");
        Ok(())
    }

    fn sync(&mut self) -> Result<SyncOutcome> {
        Self::availability()?;
        tracing::info!(
            "EventKit sync starting (reminders: {}, events: next {} days)",
            self.reminders_enabled,
            self.future_days,
        );
        let mut outcome = SyncOutcome::new(self.id());
        let imported_at = Utc::now();

        if self.reminders_enabled {
            let mgr = self.reminders_manager();
            let reminders = mgr.fetch_all_reminders().map_err(|e| {
                tracing::error!("EventKit failed to fetch reminders: {e}");
                anyhow::anyhow!("failed to fetch reminders: {e}")
            })?;

            tracing::info!("EventKit fetched {} reminders", reminders.len());
            for r in &reminders {
                let action = convert_reminder(r, imported_at);
                let version = r.last_modified_date.map(|d| d.to_rfc3339());
                outcome.push_action(r.identifier.clone(), version, action);
            }
        }

        if self.events_enabled {
            let future_days = self.future_days;
            let accepted_events_only = self.accepted_events_only;
            let mgr = self.events_manager();
            let events = mgr.fetch_upcoming_events(future_days).map_err(|e| {
                tracing::error!("EventKit failed to fetch upcoming events: {e}");
                anyhow::anyhow!("failed to fetch upcoming events: {e}")
            })?;

            tracing::info!("EventKit fetched {} upcoming events", events.len());
            for e in &events {
                import_event(&mut outcome, e, accepted_events_only);
            }
        }

        tracing::info!(
            "EventKit sync complete — {} actions, {} events, {} markers",
            outcome.actions().count(),
            outcome.events().count(),
            outcome.markers().count(),
        );
        Ok(outcome)
    }
}

fn check_macos_version(version: NSOperatingSystemVersion) -> Result<()> {
    // eventkit-rs 0.5.6 calls the macOS 14 full-access selectors unconditionally.
    anyhow::ensure!(
        version.majorVersion >= 14,
        "Apple Calendar and Reminders import requires macOS 14 or later (running {}.{}.{}). \
         Update macOS to enable this provider.",
        version.majorVersion,
        version.minorVersion,
        version.patchVersion,
    );
    Ok(())
}

fn import_event(outcome: &mut SyncOutcome, event: &eventkit::EventItem, accepted_only: bool) {
    let external_id = event_identity(event);
    if accepted_only && !event_is_accepted(event) {
        outcome.exclude(external_id);
        return;
    }
    let version = event.last_modified_date.map(|date| date.to_rfc3339());
    if event.all_day {
        outcome.push_marker(external_id, version, convert_all_day_event(event));
    } else {
        outcome.push_event(external_id, version, convert_event(event));
    }
}

fn event_identity(event: &eventkit::EventItem) -> String {
    if event.identifier.trim().is_empty() {
        return event.identifier.clone();
    }
    event.occurrence_date.map_or_else(
        || event.identifier.clone(),
        |original_start| {
            format!(
                "eventkit-occurrence-v1:{}:{}:{}",
                event.identifier.len(),
                event.identifier,
                original_start
                    .with_timezone(&Utc)
                    .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
            )
        },
    )
}

fn event_is_accepted(event: &eventkit::EventItem) -> bool {
    participants_accept(event.organizer.as_ref(), &event.attendees)
}

fn participants_accept(
    organizer: Option<&eventkit::ParticipantInfo>,
    attendees: &[eventkit::ParticipantInfo],
) -> bool {
    attendees.is_empty()
        || organizer.is_some_and(|organizer| organizer.is_current_user)
        || attendees.iter().any(|attendee| {
            attendee.is_current_user && attendee.status == eventkit::ParticipantStatus::Accepted
        })
}

fn convert_reminder(r: &eventkit::ReminderItem, imported_at: DateTime<Utc>) -> Action {
    let start = r.due_date.map(|due| {
        if is_midnight(due) {
            SchedulePoint::Date(due.date_naive())
        } else {
            SchedulePoint::DateTime(due.with_timezone(&Utc))
        }
    });

    let completion = r.completed.then(|| {
        r.completion_date
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or(imported_at)
    });

    Action::new(r.title.clone())
        .with_content(build_reminder_content(r))
        .with_queued(completion.is_none() && matches!(start, Some(SchedulePoint::DateTime(_))))
        .with_start(start)
        .with_completion(completion)
        .with_source_provider("eventkit")
        .with_source_external_id(r.identifier.clone())
}

fn is_midnight(dt: DateTime<Local>) -> bool {
    dt.hour() == 0 && dt.minute() == 0 && dt.second() == 0
}

fn build_reminder_content(r: &eventkit::ReminderItem) -> Option<String> {
    let mut parts = String::new();

    if let Some(ref notes) = r.notes
        && !notes.trim().is_empty()
    {
        parts.push_str(notes);
    }

    let priority_label = match r.priority {
        1..=4 => Some("high"),
        5 => Some("medium"),
        6..=9 => Some("low"),
        _ => None,
    };

    if let Some(label) = priority_label {
        if !parts.is_empty() {
            parts.push('\n');
        }
        let _ = write!(parts, "[priority: {label}]");
    }

    if let Some(ref loc) = r.location {
        if !loc.trim().is_empty() && !parts.is_empty() {
            parts.push('\n');
        }
        let _ = write!(parts, "[location: {loc}]");
    }

    if parts.is_empty() { None } else { Some(parts) }
}

fn convert_event(e: &eventkit::EventItem) -> Event {
    let start = e.start_date.with_timezone(&Utc);
    let span = e.end_date.with_timezone(&Utc) - start;
    let duration = if span.num_seconds() > 0 {
        RelativeDuration::from(span)
    } else {
        RelativeDuration::hours(1)
    };

    let mut event = Event::new(e.title.clone(), start, duration)
        .with_content(e.notes.clone())
        .with_source_provider("eventkit")
        .with_source_external_id(event_identity(e));
    event.source_busy = match e.availability {
        eventkit::EventAvailability::Free => Some(false),
        eventkit::EventAvailability::Busy
        | eventkit::EventAvailability::Tentative
        | eventkit::EventAvailability::Unavailable => Some(true),
        eventkit::EventAvailability::NotSupported => None,
    };
    event
}

fn convert_all_day_event(e: &eventkit::EventItem) -> Marker {
    let start_date = e.start_date.date_naive();
    let end_date = e
        .end_date
        .date_naive()
        .pred_opt()
        .filter(|end| *end > start_date);

    let mut marker = Marker::new(e.title.clone(), start_date)
        .with_source_provider("eventkit")
        .with_source_external_id(event_identity(e));
    marker.content = e.notes.clone();
    marker.end_date = end_date;
    marker
}

#[allow(dead_code)]
fn convert_recurrence(ek: &eventkit::RecurrenceRule) -> Option<Recurrence> {
    if ek.months_of_year.is_some()
        || ek.weeks_of_year.is_some()
        || ek.days_of_year.is_some()
        || ek.set_positions.is_some()
    {
        tracing::debug!(?ek.frequency, "skipping EventKit rule with unsupported selectors");
        return None;
    }

    let interval = ek.interval.max(1) as u32;
    let rule = match (&ek.frequency, &ek.days_of_week, &ek.days_of_month) {
        (eventkit::RecurrenceFrequency::Weekly, Some(days), _) if interval == 1 => {
            RecurrenceRule::WeeklyDays(days.iter().filter_map(|d| weekday_from_ek(*d)).collect())
        }
        (eventkit::RecurrenceFrequency::Monthly, _, Some(days)) if interval == 1 => {
            match days.as_slice() {
                [day] if *day > 0 => RecurrenceRule::MonthlyDay(*day as u32),
                _ => return None,
            }
        }
        (_, Some(_), _) | (_, _, Some(_)) => return None,
        (frequency, None, None) => match frequency {
            eventkit::RecurrenceFrequency::Daily => RecurrenceRule::days(interval),
            eventkit::RecurrenceFrequency::Weekly => RecurrenceRule::weeks(interval),
            eventkit::RecurrenceFrequency::Monthly => RecurrenceRule::months(interval),
            eventkit::RecurrenceFrequency::Yearly => RecurrenceRule::years(interval),
        },
    };

    Some(match ek.end {
        eventkit::RecurrenceEndCondition::Never => Recurrence::new(rule),
        eventkit::RecurrenceEndCondition::AfterCount(n) => {
            Recurrence::new(rule).with_remaining(Some((n as u32).saturating_sub(1)))
        }
        eventkit::RecurrenceEndCondition::OnDate(date) => {
            Recurrence::new(rule).with_end_date(Some(date.date_naive()))
        }
    })
}

fn weekday_from_ek(day: u8) -> Option<Weekday> {
    Some(match day {
        1 => Weekday::Sun,
        2 => Weekday::Mon,
        3 => Weekday::Tue,
        4 => Weekday::Wed,
        5 => Weekday::Thu,
        6 => Weekday::Fri,
        7 => Weekday::Sat,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn recurring_slots_keep_distinct_stable_keys_across_moves_and_conversion() {
        let mut first = calendar_event(eventkit::EventAvailability::Busy);
        first.occurrence_date = Some(first.start_date);
        let mut second = first.clone();
        second.start_date += chrono::Duration::days(7);
        second.end_date += chrono::Duration::days(7);
        second.occurrence_date = Some(second.start_date);
        let mut outcome = SyncOutcome::new("eventkit");
        import_event(&mut outcome, &first, false);
        import_event(&mut outcome, &second, false);
        let ids = outcome
            .events()
            .map(|row| row.external_id.clone())
            .collect::<Vec<_>>();
        assert_ne!(ids[0], ids[1]);
        assert!(outcome.events().all(|row| row.item.source_external_id.as_deref() == Some(row.external_id.as_str())));

        first.is_detached = true;
        first.start_date += chrono::Duration::hours(3);
        first.end_date += chrono::Duration::hours(3);
        assert_eq!(event_identity(&first), ids[0]);
        first.all_day = true;
        let mut converted = SyncOutcome::new("eventkit");
        import_event(&mut converted, &first, false);
        let marker = converted.markers().next().unwrap();
        assert_eq!(marker.external_id, ids[0]);
        assert_eq!(
            marker.item.source_external_id.as_deref(),
            Some(ids[0].as_str())
        );

        let single = calendar_event(eventkit::EventAvailability::Busy);
        assert_eq!(event_identity(&single), single.identifier);
    }

    #[test]
    fn excluding_one_invitation_does_not_exclude_the_rest_of_its_series() {
        let mut accepted = calendar_event(eventkit::EventAvailability::Busy);
        accepted.occurrence_date = Some(accepted.start_date);
        let mut declined = accepted.clone();
        declined.start_date += chrono::Duration::days(7);
        declined.end_date += chrono::Duration::days(7);
        declined.occurrence_date = Some(declined.start_date);
        declined.attendees = vec![participant(true, eventkit::ParticipantStatus::Declined)];
        let mut outcome = SyncOutcome::new("eventkit");
        import_event(&mut outcome, &accepted, true);
        import_event(&mut outcome, &declined, true);
        assert_eq!(outcome.len(), 1);
        let exclusions = outcome.excluded_external_ids().collect::<Vec<_>>();
        assert_eq!(exclusions, [event_identity(&declined).as_str()]);
        assert_ne!(outcome.events().next().unwrap().external_id, exclusions[0]);
    }

    fn participant(
        is_current_user: bool,
        status: eventkit::ParticipantStatus,
    ) -> eventkit::ParticipantInfo {
        eventkit::ParticipantInfo {
            name: None,
            URL: None,
            role: eventkit::ParticipantRole::Required,
            status,
            is_current_user,
        }
    }

    fn calendar_event(availability: eventkit::EventAvailability) -> eventkit::EventItem {
        let start = Utc.with_ymd_and_hms(2026, 9, 16, 21, 0, 0).unwrap();
        eventkit::EventItem {
            identifier: "availability".into(),
            title: "Calendar entry".into(),
            notes: None,
            location: None,
            start_date: start.with_timezone(&Local),
            end_date: (start + chrono::Duration::minutes(30)).with_timezone(&Local),
            all_day: false,
            calendar_title: None,
            calendar_id: None,
            URL: None,
            availability,
            status: eventkit::EventStatus::Tentative,
            is_detached: false,
            occurrence_date: None,
            structured_location: None,
            creation_date: None,
            last_modified_date: None,
            external_identifier: None,
            timezone: None,
            attachments_count: 0,
            attendees: Vec::new(),
            organizer: None,
        }
    }
}
