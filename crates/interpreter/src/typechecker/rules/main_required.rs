use crate::{Diagnostic, FileId, Severity, Span, TypedAst};

pub(super) fn run(ta: &TypedAst, diags: &mut Vec<Diagnostic>) {
    let Some(main) = ta.functions.iter().find(|f| f.name.name == "main") else {
        // The parser already reported what is wrong with the `main` it dropped.
        if ta.unparsed_main {
            return;
        }
        diags.push(Diagnostic {
            severity: Severity::Error,
            // Missing `main` has no real location; anchor at the start of the
            // (single) script file so the diagnostic renders against its source.
            span: Span::at(FileId(0)),
            message: "missing `main` function".to_string(),
            help: vec!["add an entry point: `function main(): void { … }`".to_string()],
            notes: vec![],
        });
        return;
    };

    // Only the first `main` needs param-checking; duplicates already carry a "duplicate declaration" diagnostic.
    if !main.params.is_empty() {
        diags.push(Diagnostic {
            severity: Severity::Error,
            span: main.name.span,
            message: "`main` must have no parameters".to_string(),
            help: vec![],
            notes: vec![],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::{compile_messages, run_raw};

    #[test]
    fn missing_main_diagnoses() {
        let diags = run_raw("let x: number = 1;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "missing `main` function");
    }

    #[test]
    fn empty_program_missing_main_diagnoses() {
        let diags = run_raw("");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "missing `main` function");
    }

    #[test]
    fn valid_main_compiles() {
        let diags = run_raw("function main(): void { }");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn main_with_number_return_compiles() {
        let diags = run_raw("function main(): number { return 0; }");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn main_with_param_diagnoses() {
        let diags = run_raw("function main(a: number): void { }");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "`main` must have no parameters");
    }

    #[test]
    fn a_main_that_did_not_parse_is_not_also_missing() {
        assert_eq!(
            compile_messages("function main() {\n  return \"x\";\n}\n"),
            ["expected `:` and return type"]
        );
        assert_eq!(
            compile_messages("function main(\n"),
            ["expected parameter name"]
        );
    }

    #[test]
    fn a_nested_main_that_did_not_parse_leaves_main_missing() {
        let messages = compile_messages("function f(): void {\n  function main() {}\n}\n");
        assert!(
            messages.iter().any(|m| m == "missing `main` function"),
            "{messages:?}"
        );
    }

    #[test]
    fn duplicate_main_diagnoses() {
        let diags = run_raw("function main(): void { } function main(): void { }");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "duplicate declaration of `main`");
    }
}
