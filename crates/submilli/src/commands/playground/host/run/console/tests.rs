use std::io::Write;

use super::*;

#[test]
fn console_sink_caps_retained_output_without_losing_prior_bytes() {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let mut sink = Sink {
        output: buffer.clone(),
        lines: None,
    };
    let chunk = vec![b'x'; MAX_CONSOLE_BYTES / 2];
    sink.write_all(&chunk).unwrap();
    sink.write_all(&chunk).unwrap();
    let error = sink.write_all(b"extra").unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::FileTooLarge);
    assert_eq!(
        error.to_string(),
        "console output exceeds 1 MiB; print less output"
    );
    let output = buffer.lock().unwrap();
    assert_eq!(output.len(), MAX_CONSOLE_BYTES);
    assert!(output.capacity() <= MAX_CONSOLE_BYTES);
    assert!(output.iter().all(|byte| *byte == b'x'));
}

#[test]
fn console_text_splits_into_lines_without_a_trailing_empty_one() {
    assert!(split_console("").is_empty());
    assert_eq!(split_console("a\nb\n"), ["a", "b"]);
    assert_eq!(split_console("a\n\nb"), ["a", "", "b"]);
}
