use chrono::NaiveDate;

use super::super::{date::try_date_anchor, title::is_escaped};
use crate::{
    ast::ParseDraft,
    lexer::{SpannedToken, Token},
};

pub(in crate::parse) const RECURRENCE_BOUND_CONFLICT_WARNING: &str =
    "multiple recurrence bounds supplied; using the last one";

pub(in crate::parse) fn try_nl_recurrence_end(
    tokens: &[SpannedToken],
    i: usize,
    today: NaiveDate,
) -> Option<(NaiveDate, usize)> {
    if !tokens.get(i)?.text.eq_ignore_ascii_case("until") || is_escaped(tokens, i + 1) {
        return None;
    }

    let (date, date_len) = try_date_anchor(tokens, i + 1, today)?;
    Some((date, 1 + date_len))
}

pub(in crate::parse) fn try_nl_recurrence_count(
    tokens: &[SpannedToken],
    i: usize,
) -> Option<(u32, usize)> {
    if !tokens.get(i)?.text.eq_ignore_ascii_case("for")
        || !matches!(tokens.get(i + 1)?.token, Token::Number)
        || is_escaped(tokens, i + 2)
    {
        return None;
    }

    let unit = tokens.get(i + 2)?.text.to_ascii_lowercase();
    if !matches!(unit.as_str(), "occurrence" | "occurrences" | "times") {
        return None;
    }

    Some((tokens[i + 1].text.parse().ok()?, 3))
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
