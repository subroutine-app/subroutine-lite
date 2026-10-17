use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

use serde_json::{Value, json};

#[test]
#[ignore = "requires a built host JNI library and JDK; run test-jvm.nu"]
fn real_jvm_round_trips_the_native_contract() {
    let java_home =
        PathBuf::from(std::env::var_os("JAVA_HOME").expect("JAVA_HOME must name a JDK"));
    let library = std::env::var_os("SUBROUTINE_PARSER_JNI_LIBRARY")
        .expect("host JNI library path is required");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/jvm/NativeItemParser.java");
    let mut child = Command::new(java_home.join("bin/java"))
        .arg("-Xcheck:jni")
        .arg("-Dfile.encoding=UTF-8")
        .arg(source)
        .arg(library)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("could not launch the JVM");
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut call = |request: &str| {
        writeln!(input, "{request}").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        serde_json::from_str::<Value>(&line).expect("JNI returned invalid JSON")
    };
    for kind in [
        "Action",
        "Event",
        "Marker",
        "Signal",
        "RoutineStep",
        "ActionTemplate",
        "EventTemplate",
    ] {
        let request = json!({"version": 1, "text": "😀 café 東京 daily for 20m", "kind": kind, "now": "2026-10-15T12:00:00Z", "timezone": "Asia/Tokyo", "defaultDate": null});
        let parsed = call(&request.to_string());
        assert!(parsed["error"].is_null(), "{parsed}");
        assert_eq!(parsed["title"], "😀 café 東京");
        assert_eq!(parsed["durationSeconds"], 1200);
        assert_eq!(parsed["recurrence"]["timezone"], "Asia/Tokyo");
        let text: Vec<_> = request["text"].as_str().unwrap().encode_utf16().collect();
        for highlight in parsed["highlights"].as_array().unwrap() {
            let start = highlight["start"].as_u64().unwrap() as usize;
            let end = highlight["end"].as_u64().unwrap() as usize;
            let selected = String::from_utf16(&text[start..end]).unwrap();
            if highlight["kind"] == "Duration" {
                assert_eq!(selected, "for 20m");
            }
        }
    }
    for (text, timezone, date, expected) in [
        (
            "Read at 9am",
            "Asia/Tokyo",
            Some("2030-06-05"),
            Some("2030-06-05T00:00:00Z"),
        ),
        (
            "Read at 9am",
            "America/New_York",
            Some("2030-06-05"),
            Some("2030-06-05T13:00:00Z"),
        ),
        ("Read", "America/New_York", Some("2030-06-05"), None),
        ("", "America/New_York", None, None),
        (
            "Read 2026-11-01 at 1:30am",
            "America/New_York",
            None,
            Some("2026-11-01T05:30:00Z"),
        ),
    ] {
        let request = json!({"version": 1, "text": text, "kind": "Action", "now": "2026-10-15T12:00:00Z", "timezone": timezone, "defaultDate": date});
        let parsed = call(&request.to_string());
        assert!(parsed["error"].is_null());
        assert_eq!(parsed["whenValue"], json!(expected));
    }
    for request in ["@null", "@surrogate", "@oversize", "{}", "{bad json"] {
        assert!(call(request)["error"].is_string());
    }
    for (version, kind, text, timezone, date) in [
        (2, "Action", "private title", "UTC", None),
        (1, "Unknown", "private title", "UTC", None),
        (1, "Action", "private title", "invalid", None),
        (1, "Action", "private title", "UTC", Some("2026-02-30")),
        (
            1,
            "Action",
            "private title 2026-03-08 at 2:30am",
            "America/New_York",
            None,
        ),
        (1, "Action", "private title every 0 days", "UTC", None),
    ] {
        let request = json!({"version": version, "text": text, "kind": kind, "now": "2026-10-15T12:00:00Z", "timezone": timezone, "defaultDate": date});
        let parsed = call(&request.to_string());
        let error = parsed["error"].as_str().unwrap();
        assert!(!error.contains("private title"));
    }
    drop(call);
    drop(input);
    assert!(child.wait().unwrap().success());
}
