use interpreter::FileId;
use interpreter::compile::compile_script_checked;

#[test]
fn invalid_source_has_a_bounded_diagnostic_set_and_recovery_is_healthy() {
    let source = "@".repeat(100_000);
    let error = compile_script_checked(&source, "bad.ts", FileId(0), &[], &[])
        .expect_err("invalid source must not compile");
    assert_eq!(
        error
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message.contains("unexpected character"))
            .count(),
        20
    );
    // Parsing EOF may add its own diagnostic after the independent lexer cap.
    assert_eq!(error.diagnostics.len(), 21);
    assert!(error.fatal.is_none());

    let compiled = compile_script_checked(
        "export function main(): number { return 1; }",
        "healthy.ts",
        FileId(0),
        &[],
        &[],
    );
    assert!(compiled.is_ok());
}
