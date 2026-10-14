use chrono::{DateTime, Local, NaiveDate, Offset as _, TimeZone, Utc};
use chronoutil::RelativeDuration;
use gpui::{Context, Window, actions};
use parser::{EntityKind, HighlightKind, ParseDraft, WhenSpec};
use subroutine_core::{Action, AnyItem, Event, ItemType, Recurrence, SchedulePoint, Signal};
use uuid::Uuid;

use super::{DraftView, EditKind, ItemManager, ItemSubject, apply_action_when};

pub(crate) const DRAFT_KEY_CONTEXT: &str = "ItemDraft";
actions!(item_draft, [DraftAsAction, DraftAsEvent, OpenDraftTypeMenu]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DraftType {
    Action,
    Event,
}

impl DraftType {
    pub(crate) fn item_type(self) -> ItemType {
        match self {
            Self::Action => ItemType::Action,
            Self::Event => ItemType::Event,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Action => "Action",
            Self::Event => "Event",
        }
    }
}

/// Whether a seeded Event duration was suggested or explicitly selected (for example, drawn).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DraftDuration {
    Suggested,
    Explicit,
}

enum DraftSeed {
    Action(Action),
    Event(Event, DraftDuration),
}

pub(super) struct InlineDraft {
    seed: DraftSeed,
    pub kind: DraftType,
}

impl InlineDraft {
    pub fn new(item: &AnyItem, duration: DraftDuration) -> Option<Self> {
        let (seed, kind) = match item {
            AnyItem::Action(action) => (DraftSeed::Action(action.clone()), DraftType::Action),
            AnyItem::Event(event) => (DraftSeed::Event(event.clone(), duration), DraftType::Event),
            _ => return None,
        };
        Some(Self { seed, kind })
    }

    pub fn event(start: Option<SchedulePoint>, queued: bool) -> Self {
        Self {
            seed: DraftSeed::Action(Action::new("").with_start(start).with_queued(queued)),
            kind: DraftType::Event,
        }
    }

    pub fn parse(&self, text: &str) -> Result<ParseDraft, parser::ParseError> {
        let kind = match self.kind {
            DraftType::Action => EntityKind::Action,
            DraftType::Event => EntityKind::Event,
        };
        parse(text, kind, self.date())
    }

    fn date(&self) -> Option<NaiveDate> {
        match &self.seed {
            DraftSeed::Action(action) => action.start.map(Into::into),
            DraftSeed::Event(event, _) => Some(event.start.with_timezone(&Local).date_naive()),
        }
    }

    fn action(&self) -> Action {
        match &self.seed {
            DraftSeed::Action(action) => action.clone(),
            DraftSeed::Event(event, duration) => {
                let mut action = Action::new(event.title.clone())
                    .with_queued(true)
                    .with_start(Some(SchedulePoint::DateTime(event.start)))
                    .with_duration((*duration == DraftDuration::Explicit).then_some(event.duration))
                    .with_content(event.content.clone())
                    .with_recurrence(event.recurrence);
                action.id = event.id;
                action.recurrence_id = event.lineage_id;
                action.template_id = event.template_id;
                action.source_provider = event.source_provider.clone();
                action.source_external_id = event.source_external_id.clone();
                action
            }
        }
    }

    pub fn preview(
        &self,
        parsed: Option<&ParseDraft>,
        recurrence: Option<Recurrence>,
    ) -> Result<AnyItem, &'static str> {
        if self.kind == DraftType::Event
            && let DraftSeed::Event(event, _) = &self.seed
        {
            return apply_event(event, parsed, recurrence).map(AnyItem::Event);
        }

        let seed = self.action();
        let mut action = apply_action(&seed, parsed, recurrence)?;
        if self.kind == DraftType::Action {
            return Ok(AnyItem::Action(action));
        }

        // A date moves an explicitly selected clock; it never supplies a missing one.
        if let Some(SchedulePoint::DateTime(start)) = seed.start {
            action.start = Some(SchedulePoint::DateTime(apply_datetime_when(
                start,
                parsed.and_then(|draft| draft.when.as_ref()),
            )?));
        }
        let Some(SchedulePoint::DateTime(start)) = action.start else {
            return Ok(AnyItem::Action(action));
        };
        let mut event = Event::new(
            action.title,
            start,
            action.duration.unwrap_or(RelativeDuration::hours(1)),
        )
        .with_content(action.content)
        .with_recurrence(action.recurrence);
        event.id = action.id;
        event.lineage_id = action.recurrence_id;
        event.template_id = action.template_id;
        event.source_provider = action.source_provider;
        event.source_external_id = action.source_external_id;
        Ok(AnyItem::Event(event))
    }

    pub fn needs_time(&self, parsed: &Result<Option<ParseDraft>, parser::ParseError>) -> bool {
        if self.kind != DraftType::Event {
            return false;
        }
        let Ok(parsed) = parsed else {
            return false;
        };
        let Ok(recurrence) =
            parser::recurrence_to_rule(parsed.as_ref().and_then(|draft| draft.recurrence.as_ref()))
        else {
            return false;
        };
        matches!(
            self.preview(parsed.as_ref(), recurrence),
            Ok(AnyItem::Action(_))
        )
    }

    pub fn finish(
        &self,
        parsed: Option<&ParseDraft>,
        recurrence: Option<Recurrence>,
    ) -> Result<AnyItem, &'static str> {
        let item = self.preview(parsed, recurrence)?;
        if self.kind == DraftType::Event && !matches!(item, AnyItem::Event(_)) {
            return Err("Choose a time for this event");
        }
        crate::item_subject::validate_item_timing(&item)?;
        Ok(item)
    }
}

pub(super) fn has_when(draft: &ParseDraft) -> bool {
    draft
        .highlights
        .iter()
        .any(|(_, kind)| *kind == HighlightKind::When)
}

pub(super) fn parse(
    text: &str,
    kind: EntityKind,
    date: Option<NaiveDate>,
) -> Result<ParseDraft, parser::ParseError> {
    let Some(date) = date else {
        return match kind {
            EntityKind::Action => parser::parse_action(text),
            EntityKind::Event => parser::parse_event(text),
            EntityKind::Signal => parser::parse_signal(text),
            EntityKind::Marker => parser::parse_marker(text),
            EntityKind::RoutineStep => parser::parse_routine_step(text),
            EntityKind::ActionTemplate => parser::parse_action_template(text),
            EntityKind::EventTemplate => parser::parse_event_template(text),
        };
    };
    let mut draft = parser::parse_on_date(text, kind, date)?;
    // The injected date is context, not an instruction to erase selected timing or queue state.
    if !has_when(&draft) && draft.recurrence.is_none() {
        draft.when = None;
    }
    Ok(draft)
}

pub(super) fn apply_action(
    original: &Action,
    parsed: Option<&ParseDraft>,
    recurrence: Option<Recurrence>,
) -> Result<Action, &'static str> {
    let mut action = original.clone();
    if let Some(draft) = parsed {
        action.title = draft.title.clone();
        if has_when(draft) {
            if let Some(when) = &draft.when {
                apply_action_when(&mut action, when);
            }
        } else {
            action.start = recurrence_start(original.start, draft.when.as_ref())?;
        }
        if let Some(duration) = draft.duration {
            action.duration = Some(duration.into());
        }
        if let Some(recurrence) = recurrence {
            action.recurrence = Some(recurrence);
        }
        if let Some(content) = &draft.content {
            action.content = Some(content.clone());
        }
    }
    Ok(action)
}

pub(super) fn apply_event(
    original: &Event,
    parsed: Option<&ParseDraft>,
    recurrence: Option<Recurrence>,
) -> Result<Event, &'static str> {
    let mut event = original.clone();
    if let Some(draft) = parsed {
        event.title = draft.title.clone();
        event.start = apply_datetime_when(event.start, draft.when.as_ref())?;
        if let Some(duration) = draft.duration {
            event.duration = duration.into();
        }
        if let Some(recurrence) = recurrence {
            event.recurrence = Some(recurrence);
        }
        if let Some(content) = &draft.content {
            event.content = Some(content.clone());
        }
    }
    Ok(event)
}

pub(super) fn apply_signal(
    original: &Signal,
    parsed: &ParseDraft,
    recurrence: Option<Recurrence>,
) -> Result<Signal, &'static str> {
    let mut signal = original.clone();
    signal.title = parsed.title.clone();
    signal.datetime = apply_datetime_when(signal.datetime, parsed.when.as_ref())?;
    if let Some(recurrence) = recurrence {
        signal.recurrence = Some(recurrence);
    }
    if let Some(content) = &parsed.content {
        signal.content = Some(content.clone());
    }
    Ok(signal)
}

fn recurrence_start(
    original: Option<SchedulePoint>,
    when: Option<&WhenSpec>,
) -> Result<Option<SchedulePoint>, &'static str> {
    match (original, when) {
        (Some(SchedulePoint::DateTime(start)), when) => {
            apply_datetime_when(start, when).map(|start| Some(SchedulePoint::DateTime(start)))
        }
        (_, Some(WhenSpec::NaiveDate(date))) => Ok(Some(SchedulePoint::Date(*date))),
        (_, Some(WhenSpec::DateTime(start))) => Ok(Some(SchedulePoint::DateTime(*start))),
        (_, None) => Ok(original),
    }
}

fn apply_datetime_when(
    original: DateTime<Utc>,
    when: Option<&WhenSpec>,
) -> Result<DateTime<Utc>, &'static str> {
    match when {
        Some(WhenSpec::DateTime(time)) => Ok(*time),
        Some(WhenSpec::NaiveDate(date)) => reschedule_in(original, *date, &Local),
        None => Ok(original),
    }
}

fn reschedule_in(
    original: DateTime<Utc>,
    date: NaiveDate,
    timezone: &impl TimeZone,
) -> Result<DateTime<Utc>, &'static str> {
    let local = original.with_timezone(timezone);
    let offset = local.offset().fix().local_minus_utc();
    let naive = original
        .naive_utc()
        .checked_add_signed(chrono::Duration::seconds(i64::from(offset)))
        .ok_or("The date is outside the supported calendar range")?;
    if date == naive.date() {
        return Ok(original);
    }
    timezone
        .from_local_datetime(&date.and_time(naive.time()))
        .earliest()
        .map(|time| time.with_timezone(&Utc))
        .ok_or("This time does not exist in the local timezone. Choose another time.")
}

impl ItemManager {
    pub(crate) fn view_draft(&self, view: DraftView) -> Option<&AnyItem> {
        let editing = self.editing_item.as_ref()?;
        match (&editing.kind, &editing.original) {
            (EditKind::ViewDraft { view: owner, .. }, ItemSubject::Live(item))
                if *owner == view =>
            {
                Some(item)
            }
            _ => None,
        }
    }

    pub(crate) fn draft_item(&self, id: Uuid) -> Option<&AnyItem> {
        let editing = self
            .editing_item
            .as_ref()
            .filter(|editing| editing.is_draft())?;
        match &editing.original {
            ItemSubject::Live(item) if item.id() == id => Some(item),
            _ => None,
        }
    }

    pub(crate) fn draft_type(&self, id: Uuid) -> Option<DraftType> {
        if matches!(
            self.editing_item.as_ref()?.kind,
            EditKind::ViewDraft {
                view: DraftView::Home
                    | DraftView::Queue
                    | DraftView::Focus
                    | DraftView::Unqueued
                    | DraftView::Unscheduled
                    | DraftView::Routines
                    | DraftView::SavedItems,
                ..
            }
        ) {
            return None;
        }
        self.draft_item(id)?;
        Some(self.editing_item.as_ref()?.draft.as_ref()?.kind)
    }

    pub(crate) fn set_draft_type(
        &mut self,
        id: Uuid,
        target: DraftType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(current) = self.draft_type(id) else {
            return;
        };
        let editing = self.editing_item.as_mut().expect("draft is being edited");
        if current != target {
            editing.draft.as_mut().expect("switchable draft").kind = target;
            editing.update_parse(&editing.value(cx));
            cx.notify();
        }
        cx.focus_view(&editing.input, window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, FixedOffset};

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2030, 11, 20).unwrap()
    }

    fn at(date: NaiveDate, hour: u32) -> DateTime<Utc> {
        Local
            .from_local_datetime(&date.and_hms_opt(hour, 0, 0).unwrap())
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
    }

    fn finish(draft: &InlineDraft, text: &str) -> AnyItem {
        let parsed = draft.parse(text).unwrap();
        let recurrence = parser::recurrence_to_rule(parsed.recurrence.as_ref()).unwrap();
        draft.finish(Some(&parsed), recurrence).unwrap()
    }

    #[test]
    fn pending_event_keeps_its_preview_until_timed_submission() {
        let mut draft = InlineDraft::event(Some(date().into()), true);
        let seed = draft.preview(None, None).unwrap();
        assert!(matches!(seed, AnyItem::Action(_)));
        assert_eq!(seed.start(), Some(date().into()));
        assert_eq!(seed.duration(), None);
        assert!(draft.needs_time(&Ok(None)));
        let text = "Review every Monday for 45 minutes";
        let parsed = draft.parse(text).unwrap();
        let rule = parser::recurrence_to_rule(parsed.recurrence.as_ref()).unwrap();
        assert_eq!(
            draft.finish(Some(&parsed), rule).unwrap_err(),
            "Choose a time for this event"
        );
        assert_eq!(parsed.raw, text);
        assert!(draft.needs_time(&Ok(Some(parsed))));

        let text = "Review 2030-11-21 at 10am for 45 minutes";
        for kind in [DraftType::Event, DraftType::Action, DraftType::Event] {
            draft.kind = kind;
            assert!(!draft.needs_time(&draft.parse(text).map(Some)));
            let saved = finish(&draft, text);
            assert_eq!(saved.item_type(), kind.item_type());
            assert_eq!(saved.title(), "Review");
            assert_eq!(
                saved.start(),
                Some(at(date() + Duration::days(1), 10).into())
            );
            assert_eq!(saved.duration(), Some(RelativeDuration::minutes(45)));
        }
        assert!(draft.needs_time(&draft.parse("Review").map(Some)));
        let invalid = draft.parse("Review 2030-02-30 at 10am").map(Some);
        assert!(invalid.is_err());
        assert!(!draft.needs_time(&invalid));
        let preview = draft.preview(None, None).unwrap();
        assert_eq!(
            serde_json::to_value(preview).unwrap(),
            serde_json::to_value(seed).unwrap()
        );
        let unscheduled = InlineDraft::event(None, false).preview(None, None).unwrap();
        assert_eq!(unscheduled.start(), None);
    }

    #[test]
    fn action_round_trip_preserves_metadata_without_event_defaults() {
        let hour = RelativeDuration::hours(1);
        for duration in [None, Some(RelativeDuration::minutes(40))] {
            let seed = AnyItem::Action(
                Action::new("Review")
                    .with_start(Some(at(date(), 13).into()))
                    .with_duration(duration)
                    .with_pinned(true)
                    .with_queued(false)
                    .with_content(Some("Notes"))
                    .with_routine_id(Uuid::now_v7()),
            );
            let mut draft = InlineDraft::new(&seed, DraftDuration::Suggested).unwrap();
            draft.kind = DraftType::Event;
            assert_eq!(
                finish(&draft, "Review").duration(),
                Some(duration.unwrap_or(hour))
            );
            draft.kind = DraftType::Action;
            assert!(!draft.needs_time(&Ok(None)));
            let restored = finish(&draft, "Review");
            assert_eq!(
                serde_json::to_value(restored).unwrap(),
                serde_json::to_value(seed).unwrap()
            );
        }
    }

    #[test]
    fn event_duration_provenance_survives_round_trip_and_clause_removal() {
        let hour = RelativeDuration::hours(1);
        let mut event = Event::new("Review", at(date(), 13), hour);
        event.busy_override = Some(false);
        event.template_id = Some(Uuid::now_v7());
        for provenance in [DraftDuration::Suggested, DraftDuration::Explicit] {
            let mut draft = InlineDraft::new(&AnyItem::Event(event.clone()), provenance).unwrap();
            assert!(!draft.needs_time(&Ok(None)));
            draft.kind = DraftType::Action;
            assert_eq!(finish(&draft, "Review for 1 hour").duration(), Some(hour));
            let action = finish(&draft, "Review");
            assert_eq!(
                action.duration(),
                (provenance == DraftDuration::Explicit).then_some(hour)
            );
            assert_eq!(action.start(), Some(event.start.into()));
            draft.kind = DraftType::Event;
            let AnyItem::Event(restored) = finish(&draft, "Review") else {
                panic!("Event selected");
            };
            assert_eq!(restored, event);
        }
    }

    #[test]
    fn contextual_parsing_anchors_bare_times_but_not_relative_phrases() {
        for kind in [EntityKind::Action, EntityKind::Event, EntityKind::Signal] {
            assert_eq!(
                parse("Review", kind.clone(), Some(date())).unwrap().when,
                None
            );
            let parsed = parse("Review at 10am", kind, Some(date())).unwrap();
            assert_eq!(parsed.when, Some(WhenSpec::DateTime(at(date(), 10))));
        }
        let before = Utc::now();
        let parsed = parse("Review in 2 hours", EntityKind::Event, Some(date())).unwrap();
        let after = Utc::now();
        let start = parsed.when.unwrap().unwrap_datetime();
        assert!(start >= before + Duration::hours(2));
        assert!(start <= after + Duration::hours(2));
    }

    #[test]
    fn recurrence_preserves_action_clock_and_queue_unless_date_is_explicit() {
        let original = Action::new("Review").with_start(Some(at(date(), 13).into()));
        for (text, queued, target) in [
            ("Review daily", true, date()),
            ("Review every Monday", false, date() + Duration::days(5)),
        ] {
            let original = original.clone().with_queued(queued);
            let parsed = parse(text, EntityKind::Action, Some(date())).unwrap();
            let rule = parser::recurrence_to_rule(parsed.recurrence.as_ref()).unwrap();
            assert!(rule.is_some());
            let updated = apply_action(&original, Some(&parsed), rule).unwrap();
            assert_eq!(updated.start, Some(at(target, 13).into()));
            assert_eq!(updated.queued, queued);
            assert_eq!(updated.recurrence, rule);
        }
        let parsed = parse("Review daily 2030-11-21", EntityKind::Action, Some(date())).unwrap();
        let rule = parser::recurrence_to_rule(parsed.recurrence.as_ref()).unwrap();
        let updated = apply_action(&original.with_queued(true), Some(&parsed), rule).unwrap();
        assert_eq!(updated.start, Some((date() + Duration::days(1)).into()));
        assert!(!updated.queued);
    }

    #[test]
    fn date_edits_preserve_local_clock_and_reject_out_of_bounds_conversion() {
        let start = at(date(), 13);
        let moved = date() + Duration::days(1);
        let event = Event::new("Review", start, RelativeDuration::minutes(40));
        let parsed = parse("Review 2030-11-21", EntityKind::Event, Some(date())).unwrap();
        let updated = apply_event(&event, Some(&parsed), None).unwrap();
        assert_eq!(updated.start, at(moved, 13));
        assert_eq!(updated.duration, event.duration);
        let signal = Signal::new("Review", start);
        let parsed = parse("Review 2030-11-21", EntityKind::Signal, Some(date())).unwrap();
        assert_eq!(
            apply_signal(&signal, &parsed, None).unwrap().datetime,
            updated.start
        );
        let draft = InlineDraft::event(Some(start.into()), false);
        assert_eq!(
            finish(&draft, "Review 2030-11-21").start(),
            Some(updated.start.into())
        );

        let zone = FixedOffset::east_opt(14 * 60 * 60).unwrap();
        let original = date().and_hms_opt(10, 15, 0).unwrap().and_utc();
        let target = moved + Duration::days(1);
        let local = reschedule_in(original, target, &zone)
            .unwrap()
            .with_timezone(&zone);
        assert_eq!(local.naive_local(), target.and_hms_opt(0, 15, 0).unwrap());
        assert!(reschedule_in(original, NaiveDate::MIN, &zone).is_err());
    }
}
