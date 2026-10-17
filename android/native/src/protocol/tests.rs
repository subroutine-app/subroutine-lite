use super::*;
use serde_json::{Value, json};

fn request(text: &str, kind: &str) -> Value {
    json!({"version": 1, "text": text, "kind": kind, "now": "2026-10-15T12:02:00Z", "timezone": "America/New_York", "defaultDate": "2030-06-05"})
}

fn response(request: Value) -> Value {
    serde_json::to_value(parse(&request.to_string()).unwrap()).unwrap()
}

#[test]
fn all_kinds_return_the_exact_contract_and_core_recurrence() {
    for kind in [
        "Action",
        "Event",
        "Marker",
        "Signal",
        "RoutineStep",
        "ActionTemplate",
        "EventTemplate",
    ] {
        let parsed = response(request(
            "🧪 café 東京 daily for 20m for 3 occurrences",
            kind,
        ));
        assert!(parsed["error"].is_null(), "{parsed}");
        assert_eq!(parsed["title"], "🧪 café 東京");
        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["durationSeconds"], 1200);
        assert_eq!(
            parsed["recurrence"],
            json!({"rule": {"type": "relative", "value": {"unit": "days", "interval": 1}}, "timezone": "America/New_York", "remaining": 3, "end_date": null})
        );
        assert_eq!(parsed.as_object().unwrap().len(), 9);
        let decoded: Recurrence = serde_json::from_value(parsed["recurrence"].clone()).unwrap();
        assert_eq!(decoded.remaining, Some(3));
    }
}

#[test]
fn empty_and_implicit_dates_do_not_replace_manual_timing() {
    for text in ["", " \t\n", "\u{2003}"] {
        let parsed = response(request(text, "Event"));
        assert!(parsed["error"].is_null());
        assert_eq!(parsed["title"], "");
        assert_eq!(parsed["highlights"], json!([]));
        for field in ["whenValue", "naiveTime", "durationSeconds", "recurrence"] {
            assert!(parsed[field].is_null());
        }
    }
    for text in ["Read", "Read for 20m"] {
        let parsed = response(request(text, "Action"));
        assert!(parsed["whenValue"].is_null());
    }
    assert_eq!(
        response(request("Read at 9am", "Action"))["whenValue"],
        "2030-06-05T13:00:00Z"
    );
    assert_eq!(
        response(request("Read tomorrow", "Action"))["whenValue"],
        "2026-10-16"
    );
    assert_eq!(
        response(request("Read now", "Action"))["whenValue"],
        "2026-10-15T12:05:00Z"
    );
    let template = response(request("Read at 9am", "ActionTemplate"));
    assert!(template["whenValue"].is_null());
    assert_eq!(template["naiveTime"], "09:00:00");
    let template = response(request("Read at 9am", "EventTemplate"));
    assert!(template["whenValue"].is_null());
    assert!(template["naiveTime"].is_null());
    assert_eq!(template["title"], "Read at 9am");
}

#[test]
fn utf16_highlights_address_the_original_text_including_surrogate_pairs() {
    let text = "🧑🏽‍💻 café 東京 e\u{301} tomorrow for 15m";
    let parsed = response(request(text, "Action"));
    let units: Vec<_> = text.encode_utf16().collect();
    let mut clauses = Vec::new();
    for highlight in parsed["highlights"].as_array().unwrap() {
        let start = highlight["start"].as_u64().unwrap() as usize;
        let end = highlight["end"].as_u64().unwrap() as usize;
        let slice = String::from_utf16(&units[start..end]).unwrap();
        if highlight["kind"] != "Title" {
            clauses.push((slice, highlight["kind"].as_str().unwrap().to_owned()));
        }
    }
    assert_eq!(
        clauses,
        [
            ("tomorrow".into(), "When".into()),
            ("for 15m".into(), "Duration".into())
        ]
    );
    let offsets = utf16_offsets("😀é");
    assert_eq!(
        offsets,
        vec![Some(0), None, None, None, Some(2), None, Some(3)]
    );
}

#[test]
fn recurrence_bounds_warn_and_keep_the_last_bound() {
    let parsed = response(request(
        "Read daily for 3 occurrences until tomorrow",
        "Marker",
    ));
    assert_eq!(parsed["recurrence"]["end_date"], "2026-10-16");
    assert!(parsed["recurrence"]["remaining"].is_null());
    assert_eq!(parsed["warnings"].as_array().unwrap().len(), 1);
    let parsed = response(request("Read every Monday at 9am", "Event"));
    assert_eq!(parsed["whenValue"], "2030-06-10T13:00:00Z");
    assert_eq!(
        parsed["recurrence"]["rule"],
        json!({"type": "week", "value": ["monday"]})
    );
}

#[test]
fn validation_is_bounded_and_does_not_quote_private_input() {
    for (field, value, expected) in [
        ("version", json!(2), "version"),
        ("kind", json!("Routine"), "kind"),
        ("now", json!("2026-10-15T12:00:00+03:00"), "UTC"),
        ("timezone", json!("not/a-zone"), "IANA"),
        ("defaultDate", json!("2026-02-30"), "defaultDate"),
        ("defaultDate", json!("2026-1-1"), "defaultDate"),
        ("text", json!("a".repeat(MAX_TEXT_BYTES + 1)), "limit"),
    ] {
        let mut input = request("private title", "Action");
        input[field] = value;
        assert!(
            response(input)["error"]
                .as_str()
                .unwrap()
                .contains(expected)
        );
    }
    assert!(
        parse(&" ".repeat(MAX_REQUEST_BYTES + 1))
            .unwrap()
            .error
            .unwrap()
            .contains("limit")
    );
    assert!(parse("{invalid json").unwrap().error.is_some());
    assert!(response(request(&"a".repeat(MAX_TEXT_BYTES), "Action"))["error"].is_null());
    for clause in [
        "for -1h",
        "2026-02-30",
        "every 0 days",
        "daily for 0 occurrences",
        "2026-03-08 at 2:30am",
    ] {
        let parsed = response(request(&format!("private title {clause}"), "Action"));
        let error = parsed["error"].as_str().unwrap();
        assert!(!error.contains("private title"));
        assert_eq!(parsed["title"], "");
        assert_eq!(parsed["highlights"], json!([]));
    }
    let parsed = response(request("Read 2026-11-01 at 1:30am", "Action"));
    assert_eq!(parsed["whenValue"], "2026-11-01T05:30:00Z");
}
