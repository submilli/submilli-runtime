use submilli_engine::{Diagnostic, FileId, Severity, typecheck_to_typed_ast};

fn errors(source: &str) -> Vec<Diagnostic> {
    let (_, diagnostics) = typecheck_to_typed_ast(source, FileId(0));
    diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

#[test]
fn inferred_constraint_violation_has_one_argument_error() {
    let diagnostics = errors(
        "function getId<T extends { id: number }>(value: T): number { return value.id; }\nfunction main(): void { getId(\"x\"); }",
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0]
            .message
            .contains("expected `{ id: number }`, got `string`"),
        "{diagnostics:?}"
    );
}

#[test]
fn explicit_constraint_violation_keeps_the_constraint_error() {
    let diagnostics = errors(
        "function getId<T extends { id: number }>(value: T): number { return value.id; }\nfunction main(): void { getId<string>(\"x\"); }",
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0]
            .message
            .contains("does not satisfy the constraint"),
        "{diagnostics:?}"
    );
}

#[test]
fn unrelated_union_alias_is_preserved_in_member_diagnostics() {
    for access in ["value.toFixed(2)", "value?.toFixed(2)"] {
        let diagnostics = errors(&format!(
            "type Id = string | number;\nfunction read(value: Id): string {{ return {access}; }}\nfunction main(): void {{}}"
        ));
        assert!(
            diagnostics.iter().any(|d| d.message.contains("`Id`")),
            "{diagnostics:?}"
        );
    }
}
