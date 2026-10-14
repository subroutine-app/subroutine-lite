use std::collections::HashSet;
use std::fmt::Write;

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc, Weekday};
use chronoutil::RelativeDuration;
use subroutine_core::{Action, Event, Recurrence, RecurrenceRule, RecurrenceUnit, SchedulePoint};
use todoist_api::TodoistWrapper;
use todoist_api::models::{CreateTaskArgs, PaginatedResponse, Task, UpdateTaskArgs};

use crate::provider::{Provider, SyncOutcome};

pub struct TodoistProvider {
    client: TodoistWrapper,
    runtime: tokio::runtime::Runtime,
}

impl TodoistProvider {
    pub fn from_env() -> Result<Self> {
        let token = std::env::var("TODOIST_API_KEY")
            .map_err(|_| anyhow::anyhow!("TODOIST_API_KEY environment variable not set"))?;
        Ok(Self::new(token))
    }

    pub fn new(api_token: String) -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create tokio runtime for Todoist provider");
        let client = TodoistWrapper::new(api_token);
        Self { client, runtime }
    }

    fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        self.runtime.block_on(f)
    }
}

impl Provider for TodoistProvider {
    fn id(&self) -> &'static str {
        "todoist"
    }

    fn name(&self) -> &'static str {
        "Todoist"
    }

    fn ensure_authorized(&mut self) -> Result<()> {
        tracing::info!("verifying Todoist API token...");
        match self.block_on(self.client.get_tasks(None, None)) {
            Ok(response) => {
                tracing::info!(
                    "Todoist authorization ok — found {} tasks",
                    response.results.len()
                );
                Ok(())
            }
            Err(e) => {
                tracing::warn!("Todoist authorization failed: {e}");
                anyhow::bail!("Todoist authorization failed: {e}")
            }
        }
    }

    fn sync(&mut self) -> Result<SyncOutcome> {
        tracing::info!("Todoist sync starting");
        let mut outcome = SyncOutcome::new(self.id());

        let tasks = fetch_tasks(|cursor| Ok(self.block_on(self.client.get_tasks(None, cursor))?))
            .map_err(|e| {
            tracing::error!("Todoist failed to fetch tasks: {e}");
            anyhow::anyhow!("failed to fetch tasks from Todoist: {e}")
        })?;

        tracing::info!("Todoist fetched {} tasks", tasks.len());

        for task in &tasks {
            if task.is_deleted {
                tracing::debug!("skipping deleted task {}", task.id);
                continue;
            }

            let action = convert_task(task);
            let version = task.updated_at.clone();
            outcome.push_action(task.id.clone(), version, action);
        }

        tracing::info!(
            "Todoist sync complete — {} actions",
            outcome.actions().count(),
        );
        Ok(outcome)
    }

    fn push_action(&mut self, action: &Action) -> Result<()> {
        let external_id = action.source_external_id.as_deref().ok_or_else(|| {
            anyhow::anyhow!("action has no source_external_id — cannot push to Todoist")
        })?;

        let is_completed = action.is_completed();

        if action.source_provider.as_deref() != Some("todoist") {
            let args = build_create_args(action);
            self.block_on(self.client.create_task(&args)).map_err(|e| {
                tracing::error!("Todoist failed to create task: {e}");
                anyhow::anyhow!("failed to create task in Todoist: {e}")
            })?;
        } else if is_completed {
            self.block_on(self.client.complete_task(external_id))
                .map_err(|e| {
                    tracing::error!("Todoist failed to complete task {external_id}: {e}");
                    anyhow::anyhow!("failed to complete task in Todoist: {e}")
                })?;
        } else {
            let args = build_update_args(action);
            if !args.has_updates() {
                return Ok(());
            }
            self.block_on(self.client.update_task(external_id, &args))
                .map_err(|e| {
                    tracing::error!("Todoist failed to update task {external_id}: {e}");
                    anyhow::anyhow!("failed to update task in Todoist: {e}")
                })?;
        }

        Ok(())
    }

    fn push_event(&mut self, _event: &Event) -> Result<()> {
        Ok(())
    }
}

fn fetch_tasks(
    mut fetch_page: impl FnMut(Option<String>) -> Result<PaginatedResponse<Task>>,
) -> Result<Vec<Task>> {
    let mut tasks = Vec::new();
    let mut cursor = None;
    let mut cursors = HashSet::new();

    loop {
        let page = fetch_page(cursor)?;
        tasks.extend(page.results);
        let Some(next) = page.next_cursor else {
            return Ok(tasks);
        };
        anyhow::ensure!(
            cursors.insert(next.clone()),
            "Todoist returned a repeated pagination cursor"
        );
        cursor = Some(next);
    }
}

fn convert_task(task: &todoist_api::models::Task) -> Action {
    let completion = task.checked.then(|| {
        task.completed_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now)
    });

    let start = task.due.as_ref().and_then(due_to_start);

    let recurrence = task.due.as_ref().and_then(|due| {
        due.is_recurring
            .then(|| convert_todoist_recurrence(&due.string))
            .flatten()
    });

    Action::new(task.content.clone())
        .with_content(build_task_content(task))
        .with_queued(completion.is_none() && matches!(start, Some(SchedulePoint::DateTime(_))))
        .with_start(start)
        .with_duration(task.duration.as_ref().and_then(convert_duration))
        .with_completion(completion)
        .with_recurrence(recurrence)
        .with_source_provider("todoist")
        .with_source_external_id(task.id.clone())
}

fn convert_duration(d: &todoist_api::models::Duration) -> Option<RelativeDuration> {
    let amount = i64::from(d.amount);
    match d.unit.as_str() {
        "minute" => Some(RelativeDuration::minutes(amount)),
        "hour" => Some(RelativeDuration::hours(amount)),
        "day" => Some(RelativeDuration::days(amount)),
        other => {
            tracing::debug!(unit = %other, "unrecognised Todoist duration unit");
            None
        }
    }
}

fn due_to_start(due: &todoist_api::models::Due) -> Option<SchedulePoint> {
    if let Some(ref dt) = due.datetime
        && let Ok(dt) = DateTime::parse_from_rfc3339(dt)
    {
        return Some(SchedulePoint::DateTime(dt.with_timezone(&Utc)));
    }
    NaiveDate::parse_from_str(&due.date, "%Y-%m-%d")
        .ok()
        .map(SchedulePoint::Date)
}

fn build_task_content(task: &todoist_api::models::Task) -> Option<String> {
    let mut parts = String::new();

    if !task.description.is_empty() {
        parts.push_str(&task.description);
    }

    let priority_label = match task.priority {
        1 => Some("p1"),
        2 => Some("p2"),
        3 => Some("p3"),
        4 => Some("p4"),
        _ => None,
    };

    if let Some(label) = priority_label {
        if !parts.is_empty() {
            parts.push('\n');
        }
        let _ = write!(parts, "[priority: {label}]");
    }

    if !task.labels.is_empty() {
        if !parts.is_empty() {
            parts.push('\n');
        }
        let _ = write!(parts, "[labels: {}]", task.labels.join(", "));
    }

    if parts.is_empty() { None } else { Some(parts) }
}

fn convert_todoist_recurrence(due_string: &str) -> Option<Recurrence> {
    let s = due_string.trim().to_lowercase();

    let body = s
        .strip_prefix("every ")
        .or_else(|| s.strip_prefix("ev "))
        .unwrap_or(&s);

    let (body, interval) = match body.strip_prefix("other ") {
        Some(rest) => (rest, 2),
        None => (body, 1),
    };

    if let Some((count, rest)) = split_first_number(body) {
        return unit_from_todoist(rest.trim()).map(|unit| Recurrence::new(relative(unit, count)));
    }

    if interval == 1
        && let Some(days) = weekday_set(body)
    {
        return Some(Recurrence::new(RecurrenceRule::WeeklyDays(days)));
    }

    let first_word = body.split_whitespace().next().unwrap_or(body);

    if matches!(first_word, "weekday" | "workday") && interval == 1 {
        return Some(Recurrence::new(RecurrenceRule::WeeklyDays(
            chrono::WeekdaySet::from_array([
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
            ]),
        )));
    }

    if first_word == "weekend" && interval == 1 {
        return Some(Recurrence::new(RecurrenceRule::WeeklyDays(
            chrono::WeekdaySet::from_array([Weekday::Sat, Weekday::Sun]),
        )));
    }

    unit_from_todoist(first_word).map(|unit| Recurrence::new(relative(unit, interval)))
}

fn relative(unit: RecurrenceUnit, interval: u32) -> RecurrenceRule {
    match unit {
        RecurrenceUnit::Days => RecurrenceRule::days(interval),
        RecurrenceUnit::Weeks => RecurrenceRule::weeks(interval),
        RecurrenceUnit::Months => RecurrenceRule::months(interval),
        RecurrenceUnit::Years => RecurrenceRule::years(interval),
    }
}

fn weekday_set(body: &str) -> Option<chrono::WeekdaySet> {
    let mut days = chrono::WeekdaySet::EMPTY;
    for name in body
        .split([',', ' '])
        .map(str::trim)
        .filter(|part| !part.is_empty() && *part != "and")
    {
        days.insert(weekday_from_todoist(name)?);
    }
    (!days.is_empty()).then_some(days)
}

fn weekday_from_todoist(name: &str) -> Option<Weekday> {
    Some(match name {
        "monday" | "mon" => Weekday::Mon,
        "tuesday" | "tue" | "tues" => Weekday::Tue,
        "wednesday" | "wed" => Weekday::Wed,
        "thursday" | "thu" | "thurs" => Weekday::Thu,
        "friday" | "fri" => Weekday::Fri,
        "saturday" | "sat" => Weekday::Sat,
        "sunday" | "sun" => Weekday::Sun,
        _ => return None,
    })
}

fn split_first_number(s: &str) -> Option<(u32, &str)> {
    let end = s.find(|c: char| !c.is_ascii_digit())?;
    let num: u32 = s[..end].parse().ok()?;
    (num > 0).then_some((num, &s[end..]))
}

fn unit_from_todoist(word: &str) -> Option<RecurrenceUnit> {
    match word {
        "day" | "days" => Some(RecurrenceUnit::Days),
        "week" | "weeks" => Some(RecurrenceUnit::Weeks),
        "month" | "months" => Some(RecurrenceUnit::Months),
        "year" | "years" => Some(RecurrenceUnit::Years),
        _ => None,
    }
}

fn build_create_args(action: &Action) -> CreateTaskArgs {
    CreateTaskArgs {
        content: action.title.clone(),
        description: strip_metadata_content(&action.content),
        due_string: recurrence_to_due_string(action),
        due_date: due_date_from_action(action),
        due_datetime: due_datetime_from_action(action),
        ..Default::default()
    }
}

fn build_update_args(action: &Action) -> UpdateTaskArgs {
    let description = strip_metadata_content(&action.content);
    let due_date = due_date_from_action(action);
    let due_datetime = due_datetime_from_action(action);

    UpdateTaskArgs {
        content: Some(action.title.clone()),
        description: if description.is_some() {
            description
        } else {
            None
        },
        due_string: recurrence_to_due_string(action),
        due_date,
        due_datetime,
        ..Default::default()
    }
}

fn due_date_from_action(action: &Action) -> Option<String> {
    action
        .start
        .map(|start| start.date_naive().format("%Y-%m-%d").to_string())
}

fn due_datetime_from_action(action: &Action) -> Option<String> {
    match action.start {
        Some(SchedulePoint::DateTime(dt)) => Some(dt.to_rfc3339()),
        _ => None,
    }
}

fn recurrence_to_due_string(action: &Action) -> Option<String> {
    action.recurrence.map(|r| r.rule.describe())
}

fn strip_metadata_content(content: &Option<String>) -> Option<String> {
    let content = content.as_deref()?;
    let cleaned: String = content
        .lines()
        .filter(|line| !line.starts_with("[priority:") && !line.starts_with("[labels:"))
        .collect::<Vec<_>>()
        .join("\n");
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str) -> Task {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "user_id": "user",
            "content": id,
            "description": "",
            "project_id": "project",
            "labels": [],
            "added_at": "2026-07-04T12:00:00Z",
            "updated_at": "2026-07-05T12:00:00Z",
            "priority": 1,
            "child_order": 0,
            "day_order": 0,
            "is_collapsed": false,
        }))
        .unwrap()
    }

    #[test]
    fn a_failed_later_page_does_not_return_a_partial_task_list() {
        let mut pages = [
            Ok(PaginatedResponse {
                results: vec![task("first")],
                next_cursor: Some("next-page".into()),
            }),
            Err(anyhow::anyhow!("page unavailable")),
        ]
        .into_iter();

        let error = fetch_tasks(|_| pages.next().unwrap()).unwrap_err();
        assert!(error.to_string().contains("page unavailable"));
        assert!(pages.next().is_none());
    }

    #[test]
    fn a_repeated_cursor_stops_paging_with_an_error() {
        let mut cursors = ["page-a", "page-b", "page-a"].into_iter();
        let error = fetch_tasks(|_| {
            Ok(PaginatedResponse {
                results: vec![],
                next_cursor: Some(cursors.next().expect("paging must stop on a cycle").into()),
            })
        })
        .unwrap_err();

        assert!(error.to_string().contains("repeated pagination cursor"));
        assert!(cursors.next().is_none());
    }
}
