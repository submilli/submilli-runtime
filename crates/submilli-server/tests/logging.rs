use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Mutex};

use serde_json::json;
use submilli_server::logging::{LogOutput, Logfmt, LogfmtFields, Stream, encode_record};

const TIMESTAMP: &str = "2026-10-03T14:05:55.563Z";

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn capture(filter: &str, emit: impl FnOnce()) -> String {
    let output = Capture::default();
    let writer = output.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .log_internal_errors(false)
        .event_format(Logfmt {
            timestamp: Some(TIMESTAMP.parse().unwrap()),
        })
        .fmt_fields(LogfmtFields)
        .with_env_filter(filter)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, emit);
    String::from_utf8(output.0.lock().unwrap().clone()).unwrap()
}

#[test]
fn execution_finished_snapshot() {
    let line = capture("info", || {
        tracing::info!(target: "submilli_server::execute",
            blueprint = "probe", session = "2543", fuel = 1000000000u64,
            wasm_fuel = 999999943u64, host_fuel = 57u64, memory_peak = 65536u64,
            wall_ms = 2318u128, outcome = "fuel_exhausted", "execution finished");
    });
    insta::assert_snapshot!(line, @r#"ts=2026-10-03T14:05:55.563Z level=info stream=log target=submilli_server::execute msg="execution finished" blueprint=probe session=2543 fuel=1000000000 wasm_fuel=999999943 host_fuel=57 memory_peak=65536 wall_ms=2318 outcome=fuel_exhausted"#);
}

#[test]
fn values_round_trip_through_logfmt_parser() {
    for value in [
        "",
        "plain",
        "a b",
        "a=b",
        "say \"hello\"",
        "a\\b",
        "space \\ slash",
        "a\nb\r\nc\td",
        "\0\u{1b}\u{7f}\u{85}",
        "שלום µ 日本語",
        "unicode\u{a0}space",
    ] {
        let line = capture("info", || tracing::info!(value, "message"));
        assert_eq!(line.lines().count(), 1);
        assert!(!line.contains('\u{1b}'));
        assert_eq!(parse_logfmt(&line)["value"], value);
    }
}

#[test]
fn oversized_events_do_not_emit_partial_records_or_break_subsequent_events() {
    let oversized = "x".repeat(1024 * 1024);
    let line = capture("info", || {
        tracing::info!(value = oversized, "too large");
        tracing::info!("healthy");
    });
    assert_eq!(line.lines().count(), 1);
    assert_eq!(parse_logfmt(&line)["msg"], "healthy");
}

#[test]
fn filtering_headers_and_explicit_parent_spans() {
    let line = capture("warn", || {
        let parent = tracing::warn_span!("request", request = "a b", stream = "spoof");
        parent.record("request", "updated");
        tracing::info!("filtered");
        tracing::warn!(parent: &parent, ts = "spoof", level = "spoof", stream = "audit", target = "spoof", msg = "spoof");
    });
    assert_eq!(line.lines().count(), 1);
    let fields = parse_logfmt(&line);
    assert_eq!(fields["msg"], "");
    assert_eq!(fields["stream"], "log");
    assert_eq!(fields["level"], "warn");
    assert_eq!(fields["fields.ts"], "spoof");
    assert_eq!(fields["request"], "updated");
    assert!(!line.contains("filtered"));
}

#[test]
fn structured_records_flatten_objects_and_lists() {
    let fields = json!({"context": {"host": "api.stripe.com"}, "contexts": [{"path": "/notes/a.md"}], "absent": null, "empty": {}, "list": [], "stream": "spoof"});
    let line = encode_record(
        TIMESTAMP.parse().unwrap(),
        &tracing::Level::INFO,
        Stream::Audit,
        "audit",
        "allowed",
        fields.as_object().unwrap(),
    )
    .unwrap();
    let fields = parse_logfmt(&line);
    assert_eq!(fields["stream"], "audit");
    assert_eq!(fields["fields.stream"], "spoof");
    assert_eq!(fields["context.host"], "api.stripe.com");
    assert_eq!(fields["contexts.0.path"], "/notes/a.md");
    assert_eq!(fields["absent"], "null");
    assert!(!fields.contains_key("empty"));
    assert!(!fields.contains_key("list"));
}

#[test]
fn structured_encoding_rejects_bad_keys_and_resource_exhaustion() {
    for fields in [
        json!({"bad key": true}),
        json!({"value": "x".repeat(1024 * 1024)}),
        json!({"value": vec![json!({}); 4097]}),
    ] {
        assert!(
            encode_record(
                TIMESTAMP.parse().unwrap(),
                &tracing::Level::INFO,
                Stream::Audit,
                "audit",
                "",
                fields.as_object().unwrap()
            )
            .is_err()
        );
    }
    let mut nested = json!(true);
    for _ in 0..66 {
        nested = json!({"nested": nested});
    }
    assert!(
        encode_record(
            TIMESTAMP.parse().unwrap(),
            &tracing::Level::INFO,
            Stream::Audit,
            "audit",
            "",
            nested.as_object().unwrap()
        )
        .is_err()
    );
    // Drop iteratively too, rather than letting a test manufacture unbounded recursion.
    while let serde_json::Value::Object(mut object) = nested {
        nested = object.remove("nested").unwrap_or(serde_json::Value::Null);
    }
}

#[test]
fn files_append_reopen_and_keep_the_old_file_on_failure() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.log");
    std::fs::write(&path, "existing\n").unwrap();
    let output = LogOutput::open(Some(path.clone())).unwrap();
    output.write_record("first\n").unwrap();
    drop(output);
    let output = LogOutput::open(Some(path.clone())).unwrap();
    output.write_record("second\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "existing\nfirst\nsecond\n"
    );
    let rotated = directory.path().join("rotated.log");
    std::fs::rename(&path, &rotated).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(output.reopen().is_err());
    output.write_record("still old\n").unwrap();
    std::fs::remove_dir(&path).unwrap();
    output.reopen().unwrap();
    output.write_record("fresh\n").unwrap();
    assert!(
        std::fs::read_to_string(rotated)
            .unwrap()
            .ends_with("still old\n")
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "fresh\n");
}

#[test]
fn concurrent_events_are_complete_lines() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server.log");
    let output = LogOutput::open(Some(path.clone())).unwrap();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .event_format(Logfmt::default())
        .fmt_fields(LogfmtFields)
        .with_writer(output)
        .finish();
    let dispatch = tracing::Dispatch::new(subscriber);
    std::thread::scope(|scope| {
        for worker in 0..8 {
            let dispatch = dispatch.clone();
            scope.spawn(move || {
                tracing::dispatcher::with_default(&dispatch, || {
                    for sequence in 0..100 {
                        tracing::info!(worker, sequence, "a complete event");
                    }
                });
            });
        }
    });
    let lines = std::fs::read_to_string(path).unwrap();
    assert_eq!(lines.lines().count(), 800);
    for line in lines.lines() {
        assert_eq!(parse_logfmt(line)["msg"], "a complete event");
    }
}

#[test]
fn invalid_destination_names_the_path() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("missing").join("server.log");
    let error = LogOutput::open(Some(path.clone())).err().unwrap();
    assert!(error.to_string().contains(&path.display().to_string()));
}

#[cfg(target_os = "linux")]
#[test]
fn special_destinations_are_rejected() {
    assert!(LogOutput::open(Some("/dev/full".into())).is_err());
}

#[test]
fn oversized_span_creation_and_updates_reject_events_without_partial_context() {
    let oversized = "x".repeat(1024 * 1024 + 1);
    let line = capture("info", || {
        let invalid = tracing::info_span!("invalid", value = oversized.as_str());
        tracing::info!(parent: &invalid, "rejected creation");
        let updated = tracing::info_span!("updated", value = "old");
        updated.record("value", oversized.as_str());
        tracing::info!(parent: &updated, "rejected update");
        tracing::info!("healthy");
    });
    assert_eq!(line.lines().count(), 1);
    assert_eq!(parse_logfmt(&line)["msg"], "healthy");
}

#[test]
fn span_rejections_with_unwritable_stderr_do_not_panic() {
    if std::env::var_os("SUBMILLI_LOGGING_CHILD").is_some() {
        oversized_span_creation_and_updates_reject_events_without_partial_context();
        return;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "span_rejections_with_unwritable_stderr_do_not_panic",
            "--nocapture",
        ])
        .env("SUBMILLI_LOGGING_CHILD", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stderr.take());
    assert!(child.wait().unwrap().success());
}

// Independent parsing: JSON handles quoted string escapes; logfmt bare tokens
// are delimited by spaces. This does not reuse the encoder's escape logic.
fn parse_logfmt(mut input: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    while !input.trim().is_empty() {
        input = input.trim_start();
        let (key, rest) = input.split_once('=').expect("key=value");
        let (value, remaining) = if rest.starts_with('"') {
            let mut values = serde_json::Deserializer::from_str(rest).into_iter::<String>();
            let value = values.next().unwrap().unwrap();
            (value, &rest[values.byte_offset()..])
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            (rest[..end].to_owned(), &rest[end..])
        };
        fields.insert(key.to_owned(), value);
        input = remaining;
    }
    fields
}
