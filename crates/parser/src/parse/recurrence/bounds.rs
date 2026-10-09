use chrono::NaiveDate;

use super::super::{
    ParseError, date::try_date_anchor, duration::join_adjacent, numeric_end, title::is_escaped,
};
use crate::{ast::ParseDraft, lexer::SpannedToken};

pub(in crate::parse) const RECURRENCE_BOUND_CONFLICT_WARNING: &str =
    "multiple recurrence bounds supplied; using the last one";

pub(in crate::parse) fn try_nl_recurrence_end(
    tokens: &[SpannedToken],
    i: usize,
    today: NaiveDate,
) -> Result<Option<(NaiveDate, usize)>, ParseError> {
    if !tokens
        .get(i)
        .is_some_and(|token| token.text.eq_ignore_ascii_case("until"))
        || is_escaped(tokens, i + 1)
    {
        return Ok(None);
    }

    Ok(try_date_anchor(tokens, i + 1, today)?.map(|(date, len)| (date, 1 + len)))
}

pub(in crate::parse) fn try_nl_recurrence_count(
    tokens: &[SpannedToken],
    i: usize,
) -> Result<Option<(u32, usize)>, ParseError> {
    if !tokens
        .get(i)
        .is_some_and(|token| token.text.eq_ignore_ascii_case("for"))
    {
        return Ok(None);
    }
    let end = numeric_end(tokens, i + 1);
    let Some(unit) = tokens.get(end) else {
        return Ok(None);
    };
    if end == i + 1
        || is_escaped(tokens, end)
        || !matches!(
            unit.text.to_ascii_lowercase().as_str(),
            "occurrence" | "occurrences" | "times"
        )
    {
        return Ok(None);
    }
    let text = join_adjacent(tokens, i + 1, end - i - 1);
    let remaining = text
        .parse::<u32>()
        .ok()
        .filter(|count| *count > 0)
        .ok_or_else(|| {
            ParseError::recurrence(text, "occurrence count must be between 1 and 4294967295")
        })?;
    Ok(Some((remaining, end - i + 1)))
}

pub(in crate::parse) fn set_recurrence_end_date(draft: &mut ParseDraft, date: NaiveDate) {
    if draft.recurrence_remaining.take().is_some() {
        add_recurrence_bound_conflict_warning(draft);
    }
    draft.recurrence_end_date = Some(date);
}

pub(in crate::parse) fn set_recurrence_remaining(draft: &mut ParseDraft, remaining: u32) {
    if draft.recurrence_end_date.take().is_some() {
        add_recurrence_bound_conflict_warning(draft);
    }
    draft.recurrence_remaining = Some(remaining);
}

fn add_recurrence_bound_conflict_warning(draft: &mut ParseDraft) {
    if !draft
        .warnings
        .iter()
        .any(|warning| warning == RECURRENCE_BOUND_CONFLICT_WARNING)
    {
        draft
            .warnings
            .push(RECURRENCE_BOUND_CONFLICT_WARNING.to_string());
    }
}
