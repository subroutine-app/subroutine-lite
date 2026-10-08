use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, Utc};

use crate::{
    ast::{EntityKind, HighlightKind, ParseDraft, RecurrenceSpec, WhenSpec},
    lexer::SpannedToken,
};

use super::{
    duration::try_nl_duration,
    recurrence::{
        bounds::{
            set_recurrence_end_date, set_recurrence_remaining, try_nl_recurrence_count,
            try_nl_recurrence_end,
        },
        has_recurrence_clause, try_nl_recurrence,
    },
    title::is_escaped,
    when::{try_nl_time, try_nl_when},
};

pub(super) struct Match {
    pub len: usize,
    clause: Clause,
}

enum Clause {
    When(WhenSpec),
    Time(NaiveTime),
    Recurrence(RecurrenceSpec),
    EndDate(NaiveDate),
    Count(u32),
    Duration(Duration),
}

impl Match {
    pub(super) fn apply(self, draft: &mut ParseDraft) -> HighlightKind {
        match self.clause {
            Clause::When(when) => {
                draft.when = Some(when);
                HighlightKind::When
            }
            Clause::Time(time) => {
                draft.naive_time = Some(time);
                HighlightKind::When
            }
            Clause::Recurrence(recurrence) => {
                draft.recurrence = Some(recurrence);
                HighlightKind::Recurrence
            }
            Clause::EndDate(date) => {
                set_recurrence_end_date(draft, date);
                HighlightKind::Recurrence
            }
            Clause::Count(remaining) => {
                set_recurrence_remaining(draft, remaining);
                HighlightKind::Recurrence
            }
            Clause::Duration(duration) => {
                draft.duration = Some(duration);
                HighlightKind::Duration
            }
        }
    }
}

pub(super) fn longest_match(
    tokens: &[SpannedToken],
    i: usize,
    now: DateTime<Local>,
    granularity: Duration,
    draft: &ParseDraft,
) -> Option<Match> {
    if is_escaped(tokens, i) {
        return None;
    }

    let mut best: Option<Match> = None;
    let mut consider = |len, clause| {
        if len > 0 && best.as_ref().is_none_or(|matched| len > matched.len) {
            best = Some(Match { len, clause });
        }
    };

    if has_recurrence_clause(tokens)
        && let Some((date, len)) = try_nl_recurrence_end(tokens, i, now.date_naive())
    {
        consider(len, Clause::EndDate(date));
    }

    if has_recurrence_clause(tokens)
        && let Some((remaining, len)) = try_nl_recurrence_count(tokens, i)
    {
        consider(len, Clause::Count(remaining));
    }

    if draft.kind == EntityKind::ActionTemplate
        && draft.naive_time.is_none()
        && let Some((time, len)) = try_nl_time(tokens, i)
    {
        consider(len, Clause::Time(time));
    }

    if draft.when.is_none()
        && let Some((when, len)) = try_nl_when(
            tokens,
            i,
            draft.kind.clone(),
            now.with_timezone(&Utc),
            now.date_naive(),
            granularity,
        )
    {
        consider(len, Clause::When(when));
    }

    if draft.recurrence.is_none()
        && let Some((recurrence, len)) = try_nl_recurrence(tokens, i)
    {
        consider(len, Clause::Recurrence(recurrence));
    }

    if draft.duration.is_none()
        && let Some((duration, len)) = try_nl_duration(tokens, i)
    {
        consider(len, Clause::Duration(duration));
    }

    best
}
