use chrono::Duration;

use super::title::is_escaped;
use crate::lexer::SpannedToken;

pub(super) fn try_nl_duration(tokens: &[SpannedToken], i: usize) -> Option<(Duration, usize)> {
    let n = tokens.len();
    if i >= n {
        return None;
    }
    let lower = tokens[i].text.to_ascii_lowercase();

    let intro_consumed = if matches!(lower.as_str(), "for" | "lasting" | "takes") {
        1usize
    } else {
        0
    };
    let dur_start = i + intro_consumed;

    if dur_start >= n {
        return None;
    }

    if is_escaped(tokens, dur_start) {
        return None;
    }

    for len in (1..=usize::min(4, n - dur_start)).rev() {
        let text = join_adjacent(tokens, dur_start, len);
        if let Some(dur) = parse_duration_expr(&text) {
            return Some((dur, intro_consumed + len));
        }
    }

    None
}

pub fn parse_duration_expr(text: &str) -> Option<Duration> {
    let t = text.trim();

    if let Some(dur) = parse_combined_duration(t) {
        return Some(dur);
    }

    parse_simple_duration(t)
}

fn parse_combined_duration(text: &str) -> Option<Duration> {
    let lower = text.to_ascii_lowercase();

    let h_split = lower
        .find("hours")
        .map(|p| (p, 5))
        .or_else(|| lower.find("hour").map(|p| (p, 4)))
        .or_else(|| lower.find("hrs").map(|p| (p, 3)))
        .or_else(|| lower.find("hr").map(|p| (p, 2)))
        .or_else(|| lower.find('h').map(|p| (p, 1)));

    let (h_end, h_suffix_len) = h_split?;
    let h_str = lower[..h_end].trim();
    let h: i64 = h_str.parse().ok()?;
    let after_h = lower[h_end + h_suffix_len..].trim();
    if after_h.is_empty() {
        return None;
    }
    let m = parse_simple_duration(after_h)?;
    Some(Duration::hours(h) + m)
}

fn parse_simple_duration(text: &str) -> Option<Duration> {
    let lower = text.trim().to_ascii_lowercase();

    let (n_str, unit): (&str, &str) = if let Some(s) = lower.strip_suffix("minutes") {
        (s, "minutes")
    } else if let Some(s) = lower.strip_suffix("minute") {
        (s, "minute")
    } else if let Some(s) = lower.strip_suffix("mins") {
        (s, "mins")
    } else if let Some(s) = lower.strip_suffix("min") {
        (s, "min")
    } else if let Some(s) = lower.strip_suffix("weeks") {
        (s, "weeks")
    } else if let Some(s) = lower.strip_suffix("week") {
        (s, "week")
    } else if let Some(s) = lower.strip_suffix("days") {
        (s, "days")
    } else if let Some(s) = lower.strip_suffix("day") {
        (s, "day")
    } else if let Some(s) = lower.strip_suffix("hours") {
        (s, "hours")
    } else if let Some(s) = lower.strip_suffix("hour") {
        (s, "hour")
    } else if let Some(s) = lower.strip_suffix("hrs") {
        (s, "hrs")
    } else if let Some(s) = lower.strip_suffix("hr") {
        (s, "hr")
    } else if let Some(s) = lower.strip_suffix('h') {
        (s, "h")
    } else if let Some(s) = lower.strip_suffix('d') {
        (s, "d")
    } else if let Some(s) = lower.strip_suffix('w') {
        (s, "w")
    } else {
        (lower.strip_suffix('m')?, "m")
    };
    let _ = unit;
    let n: i64 = n_str.trim().parse().ok()?;
    match unit {
        "d" | "day" | "days" => Some(Duration::days(n)),
        "w" | "week" | "weeks" => Some(Duration::weeks(n)),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(Duration::hours(n)),
        "m" | "min" | "mins" | "minute" | "minutes" => Some(Duration::minutes(n)),
        _ => None,
    }
}

fn join_adjacent(tokens: &[SpannedToken], start: usize, len: usize) -> String {
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
