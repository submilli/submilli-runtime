//! Malformed renderer inputs are injected internal state, not guest exploits.
use submilli_engine::rendering::{RenderError, RenderLimits, TRUNCATED};
use submilli_engine::{Diagnostic, FileId, Severity, Sources, Span, Type};

fn diagnostic(span: Span, message: String) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        span,
        message,
        help: Vec::new(),
        notes: Vec::new(),
    }
}

#[test]
fn invalid_metadata_preserves_original_failure_and_next_render_succeeds() {
    let (sources, file) = Sources::single("test.ts", "é\nx").unwrap();
    for span in [
        Span {
            file,
            start: 1,
            end: 2,
        },
        Span {
            file,
            start: 3,
            end: 1,
        },
        Span::at(FileId(99)),
    ] {
        let diag = diagnostic(span, "original failure".into());
        assert!(matches!(
            submilli_engine::diagnostics::render_checked(&diag, &sources),
            Err(RenderError::Source(_))
        ));
        let fallback = submilli_engine::diagnostics::render(&diag, &sources);
        assert!(fallback.contains("original failure"));
        assert!(fallback.contains("internal reporting failure"));
        assert!(!fallback.contains("test.ts:"));
    }
    let good = diagnostic(Span::new(file, 0, 2).unwrap(), "healthy error".into());
    let rendered = submilli_engine::diagnostics::render_checked(&good, &sources).unwrap();
    assert!(rendered.text.contains("test.ts:1:1"));
    assert!(!rendered.truncated);
}

#[test]
fn huge_source_context_and_many_diagnostics_share_limits() {
    let source = "\t日本😀".repeat(100_000);
    let (sources, file) = Sources::single("test.ts", &source).unwrap();
    let mut diag = diagnostic(
        Span::new(file, 0, source.len() as u32).unwrap(),
        "original failure".into(),
    );
    let rendered = submilli_engine::diagnostics::render_checked(&diag, &sources).unwrap();
    assert!(rendered.truncated);
    assert!(rendered.text.len() <= RenderLimits::default().bytes);
    assert!(rendered.text.contains("original failure"));
    assert!(rendered.text.ends_with(TRUNCATED));
    diag.span = Span::at(FileId::COMPILER);
    diag.message = "large error ".repeat(10_000);
    let diagnostics = vec![diag; 100];
    let collection =
        submilli_engine::diagnostics::render_collection(&diagnostics, &sources).unwrap();
    assert!(collection.truncated);
    assert!(collection.text.len() <= RenderLimits::collection().bytes);
    let list = submilli_engine::diagnostics::render_list(&diagnostics, &sources).unwrap();
    assert!(list.iter().map(String::len).sum::<usize>() <= RenderLimits::collection().bytes);
    assert!(list.last().unwrap().contains(TRUNCATED));
}

#[test]
fn invalid_rest_signature_is_checked_but_display_never_panics() {
    let ty = Type::Function {
        params: Vec::new(),
        optional: 0,
        ret: Box::new(Type::Number),
        predicate: None,
        has_rest: true,
    };
    assert!(matches!(
        ty.render_checked(RenderLimits::default()),
        Err(RenderError::InvalidMetadata(_))
    ));
    assert_eq!(ty.to_string(), "[diagnostic type unavailable]");
    assert_eq!(Type::Number.to_string(), "number");
}

#[test]
fn type_depth_boundary_and_wide_types_abbreviate() {
    let mut ty = Type::Number;
    for _ in 1..512 {
        ty = Type::Array(Box::new(ty));
    }
    assert!(
        !ty.render_checked(RenderLimits::default())
            .unwrap()
            .truncated
    );
    ty = Type::Array(Box::new(ty));
    assert!(
        ty.render_checked(RenderLimits::default())
            .unwrap()
            .truncated
    );
    let wide = Type::Tuple(submilli_engine::TupleType {
        elements: vec![Type::Number; 100_000],
        optional: 0,
    });
    let rendered = wide.render_checked(RenderLimits::default()).unwrap();
    assert!(rendered.truncated);
    assert!(rendered.text.len() <= RenderLimits::default().bytes);
}

#[test]
fn deeply_nested_type_rendering_fits_small_native_stacks() {
    use std::process::Command;
    use std::time::{Duration, Instant};
    if std::env::var_os("SUB633_RENDER_STACK_CHILD").is_some() {
        for stack in [2 * 1024 * 1024, 8 * 1024 * 1024] {
            std::thread::Builder::new()
                .stack_size(stack)
                .spawn(|| {
                    let mut ty = Type::Number;
                    for _ in 0..20_000 {
                        ty = Type::Array(Box::new(ty));
                    }
                    assert!(
                        ty.render_checked(RenderLimits::default())
                            .unwrap()
                            .truncated
                    );
                    assert!(ty.to_string().contains(TRUNCATED));
                    // The test constructs an out-of-contract tree; dispose it iteratively.
                    while let Type::Array(inner) = ty {
                        ty = *inner;
                    }
                    assert_eq!(Type::Number.to_string(), "number");
                })
                .unwrap()
                .join()
                .unwrap();
        }
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "deeply_nested_type_rendering_fits_small_native_stacks",
            "--nocapture",
        ])
        .env("SUB633_RENDER_STACK_CHILD", "1")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "renderer child: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("renderer child timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn oversized_message_does_not_hide_an_invalid_note() {
    let (sources, file) = Sources::single("test.ts", "x").unwrap();
    let mut diag = diagnostic(Span::at(file), "primary error ".repeat(10_000));
    diag.notes
        .push((Span::at(FileId(999)), "invalid note".into()));
    assert!(matches!(
        submilli_engine::diagnostics::render_checked(&diag, &sources),
        Err(RenderError::Source(_))
    ));
    diag.notes = vec![(Span::at(file), "valid note".into()); 100_000];
    let rendered = submilli_engine::diagnostics::render_checked(&diag, &sources).unwrap();
    assert!(rendered.truncated);
    assert!(rendered.text.contains("primary error"));
}
