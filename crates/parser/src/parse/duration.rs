use chrono::Duration;

use super::{ParseError, is_numeric, title::is_escaped};
use crate::lexer::SpannedToken;

pub(super) fn try_nl_duration(
    tokens: &[SpannedToken],
    i: usize,
) -> Result<Option<(Duration, usize)>, ParseError> {
    let Some(token) = tokens.get(i) else {
        return Ok(None);
    };
    let lower = token.text.to_ascii_lowercase();
    let intro_consumed = usize::from(matches!(lower.as_str(), "for" | "lasting" | "takes"));
    let dur_start = i + intro_consumed;

    if dur_start >= tokens.len() || is_escaped(tokens, dur_start) {
        return Ok(None);
    }

    for len in (1..=usize::min(8, tokens.len() - dur_start)).rev() {
        let text = join_adjacent(tokens, dur_start, len);
        if let Some(dur) = parse_duration_expr(&text)? {
            return Ok(Some((dur, intro_consumed + len)));
        }
    }

    Ok(None)
}

pub fn parse_duration_expr(text: &str) -> Result<Option<Duration>, ParseError> {
    let t = text.trim();
    let result = parse_combined_duration(t).and_then(|combined| match combined {
        Some(duration) => Ok(Some(duration)),
        None => parse_simple_duration(t),
    });
    result.map_err(|error| error.with_input(text))
}

fn parse_combined_duration(text: &str) -> Result<Option<Duration>, ParseError> {
    let lower = text.to_ascii_lowercase();
    let h_split = lower
        .find("hours")
        .map(|p| (p, 5))
        .or_else(|| lower.find("hour").map(|p| (p, 4)))
        .or_else(|| lower.find("hrs").map(|p| (p, 3)))
        .or_else(|| lower.find("hr").map(|p| (p, 2)))
        .or_else(|| lower.find('h').map(|p| (p, 1)));

    let Some((h_end, h_suffix_len)) = h_split else {
        return Ok(None);
    };
    let h_str = lower[..h_end].trim();
    if !is_numeric(h_str) {
        return Ok(None);
    }
    let after_h = lower[h_end + h_suffix_len..].trim();
    let Some(rest) = parse_simple_duration(after_h)? else {
        return Ok(None);
    };
    let hours = parse_amount(h_str, Duration::try_hours)?;
    hours
        .checked_add(&rest)
        .map(Some)
        .ok_or_else(|| ParseError::duration(text, "combined duration is out of range"))
}

fn parse_simple_duration(text: &str) -> Result<Option<Duration>, ParseError> {
    let lower = text.trim().to_ascii_lowercase();
    let units: &[(&str, fn(i64) -> Option<Duration>)] = &[
        ("minutes", Duration::try_minutes),
        ("minute", Duration::try_minutes),
        ("mins", Duration::try_minutes),
        ("min", Duration::try_minutes),
        ("weeks", Duration::try_weeks),
        ("week", Duration::try_weeks),
        ("days", Duration::try_days),
        ("day", Duration::try_days),
        ("hours", Duration::try_hours),
        ("hour", Duration::try_hours),
        ("hrs", Duration::try_hours),
        ("hr", Duration::try_hours),
        ("h", Duration::try_hours),
        ("d", Duration::try_days),
        ("w", Duration::try_weeks),
        ("m", Duration::try_minutes),
    ];
    for &(suffix, make) in units {
        if let Some(amount) = lower.strip_suffix(suffix)
            && is_numeric(amount)
        {
            return parse_amount(amount.trim(), make).map(Some);
        }
    }
    Ok(None)
}

fn parse_amount(text: &str, make: fn(i64) -> Option<Duration>) -> Result<Duration, ParseError> {
    let amount = text.parse::<i64>().map_err(|_| {
        ParseError::duration(
            text,
            "duration must be a whole number within the supported range",
        )
    })?;
    if amount < 0 {
        return Err(ParseError::duration(text, "duration cannot be negative"));
    }
    make(amount).ok_or_else(|| ParseError::duration(text, "duration is out of range"))
}

pub(super) fn join_adjacent(tokens: &[SpannedToken], start: usize, len: usize) -> String {
    let slice = &tokens[start..start + len];
    let mut out = String::new();
    for (idx, tok) in slice.iter().enumerate() {
        if idx == 0 {
            out.push_str(&tok.text);
        } else {
            let prev = &slice[idx - 1];
            if prev.span.end == tok.span.start {
                out.push_str(&tok.text);
            } else {
                out.push(' ');
                out.push_str(&tok.text);
            }
        }
    }
    out
}
