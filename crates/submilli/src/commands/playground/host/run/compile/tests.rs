use super::*;

#[test]
fn a_compile_at_the_limits_fits_from_a_worker_sized_caller() {
    let source = format!(
        "function main(): number {{ const a = 1; const s = `{}`; return s.length; }}",
        "${a}x".repeat(510)
    );
    let compiled = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            on_compiler_thread(|| {
                interpreter::compile_script(&source, "limits.ts", FileId(0), &[], &[]).is_ok()
            })
            .unwrap()
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(compiled);
}

#[test]
fn a_panicking_compile_is_reported_not_propagated() {
    let result = on_compiler_thread(|| -> () { panic!("injected compiler panic") });
    assert!(matches!(result, Err(CompilerThreadError::Panicked)));
}

#[test]
fn compile_metadata_failure_preserves_error_and_discovery_warnings() {
    let (sources, file) = Sources::single("test.ts", "é").unwrap();
    let diagnostic = Diagnostic {
        severity: Severity::Error,
        span: interpreter::Span {
            file,
            start: 1,
            end: 2,
        },
        message: "original compile error".into(),
        help: vec![],
        notes: vec![],
    };
    let outcome = compile_failure(&sources, &[diagnostic], vec!["discovery warning".into()]);
    let error = outcome.error.unwrap();
    assert!(matches!(error.kind, ErrorKind::RuntimeError));
    assert!(error.message.starts_with("internal: "), "{}", error.message);
    assert!(error.message.contains("original compile error"));
    assert!(error.message.contains("invalid diagnostic metadata"));
    assert_eq!(outcome.discovery_warnings, ["discovery warning"]);
    assert_eq!(
        diagnostic_position(&sources, interpreter::Span::at(file)).unwrap(),
        (1, 1)
    );
}

#[test]
fn diagnostic_positions_use_each_file_and_no_position_for_compiler_errors() {
    let mut sources = Sources::new();
    sources.add("root.ts", "x").unwrap();
    let dependency = sources.add("dep.ts", "first\nsecond").unwrap();
    assert_eq!(
        diagnostic_position(&sources, interpreter::Span::new(dependency, 6, 12).unwrap()).unwrap(),
        (2, 1)
    );
    assert_eq!(
        diagnostic_position(&sources, interpreter::Span::at(FileId::COMPILER)).unwrap(),
        (0, 0)
    );
    assert!(diagnostic_position(&sources, interpreter::Span::at(FileId(999))).is_err());
}

#[test]
fn fatal_host_failure_is_reported_as_a_runtime_error() {
    let (sources, file) = Sources::single("test.ts", "function main(): void {}").unwrap();
    let cause = interpreter::runtime::host::fatal_host_error("host ABI: invalid result buffer");
    let err = cause.context("executing host call");
    let payload = classify_runtime_error(&err, &sources, file);
    assert!(matches!(payload.kind, ErrorKind::RuntimeError));
    assert!(payload.message.contains("internal host error: host ABI"));
    assert!(payload.message.contains("invalid result buffer"));
    assert!(payload.diagnostics.is_empty());
}
