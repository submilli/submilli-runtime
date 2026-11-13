//! A `switch` without `default` lists every value its discriminant can hold,
//! when those values can be listed: when the discriminant's type is made of
//! literals alone. TypeScript lets such a `switch` fall through; Submilli
//! reports the values it misses.
//!
//! The front end records what each `switch` leaves unmatched
//! ([`crate::UnmatchedCases`]), since that depends on the narrowing that holds
//! where it runs. This rule decides whether the `switch` had to match it.

use crate::compiler_error::CompilerFailure;
use crate::typechecker::infer::narrowing;
use crate::typechecker::rules::body_walk::{self, Visitor};
use crate::{Diagnostic, Severity, Type, TypedAst, TypedStmt, TypedStmtKind, UnmatchedCases};

pub(super) fn run(ta: &TypedAst, diagnostics: &mut Vec<Diagnostic>) -> Result<(), CompilerFailure> {
    body_walk::walk_program(ta, &mut Rule { diagnostics })
}

struct Rule<'a> {
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl Visitor for Rule<'_> {
    fn visit_stmt(&mut self, stmt: &TypedStmt) -> Result<(), CompilerFailure> {
        if let TypedStmtKind::Switch {
            unmatched: Some(unmatched),
            ..
        } = &stmt.kind
            // A `typeof` value's type lists every tag whatever the operand
            // is, even once narrowing removes some, so a switch on it need
            // list only the tags its operand can have.
            && !unmatched.reads_typeof
            && requires_every_case(&unmatched.discriminant_ty)
        {
            self.diagnostics.push(non_exhaustive(unmatched, stmt.span));
        }
        Ok(())
    }
}

/// Whether a `switch` without `default` must list every value of its
/// discriminant: when its type is made of literals alone, which cases can
/// cover. One with a member such as `string`, `null` or `undefined`, even in
/// a single member of a discriminated union, can't be listed out, so the
/// switch may simply fall through.
fn requires_every_case(disc_ty: &Type) -> bool {
    narrowing::union_members(disc_ty).into_iter().all(|member| {
        narrowing::unit_literal_value(member).is_some() || matches!(member.peel(), Type::Boolean)
    })
}

fn non_exhaustive(unmatched: &UnmatchedCases, span: crate::Span) -> Diagnostic {
    let missing = format_missing(&unmatched.values);
    let suffix = unmatched
        .discriminant_key
        .as_ref()
        .map(|key| format!(" for discriminant `.{key}`"))
        .unwrap_or_default();
    let help = if missing.is_empty() {
        "add a `default:` clause".to_string()
    } else {
        format!("add a `default:` clause or cases for: {missing}")
    };
    let message = if missing.is_empty() {
        format!("non-exhaustive `switch`{suffix}")
    } else {
        format!("non-exhaustive `switch`: missing case(s) {missing}{suffix}")
    };
    Diagnostic {
        severity: Severity::Error,
        span,
        message,
        help: vec![help],
        notes: vec![],
    }
}

fn format_missing(values: &Type) -> String {
    let parts: Vec<String> = match values.peel() {
        Type::Union(members) => members.iter().filter_map(format_one_literal).collect(),
        single => format_one_literal(single).into_iter().collect(),
    };
    parts.join(", ")
}

fn format_one_literal(ty: &Type) -> Option<String> {
    match ty.peel() {
        Type::StringLiteral(s) => Some(format!("\"{}\"", crate::types::escape_string_literal(s))),
        Type::NumberLiteral(n) => Some(format!("{}", n.0)),
        Type::BooleanLiteral(b) => Some(b.to_string()),
        Type::Null => Some("null".to_string()),
        Type::Undefined => Some("undefined".to_string()),
        Type::Object { fields, .. } => {
            // Discriminated-union residuals are object variants; find the discriminant field's
            // literal. Only a direct literal field can be the discriminant, so nested objects
            // are skipped.
            for field in fields.values() {
                if !matches!(field.ty.peel(), Type::Object { .. })
                    && let Some(lit) = format_one_literal(&field.ty)
                {
                    return Some(lit);
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_util::run_lines;

    fn non_exhaustive(source: &str) -> Vec<(String, usize)> {
        run_lines(source)
            .into_iter()
            .filter(|(message, _)| message.starts_with("non-exhaustive"))
            .collect()
    }

    #[test]
    fn each_form_reports_what_its_cases_miss() {
        let source = r#"type Shape = { kind: "a" } | { kind: "b" };
function make(): Shape { return { kind: "b" }; }
function plain(x: "a" | "b" | "c"): number {
    switch (x) { case "a": return 1; }
    return 0;
}
function tagged(shape: Shape): number {
    switch (shape.kind) { case "a": return 1; }
    return 0;
}
function anonymous(): number {
    switch (make().kind) { case "a": return 1; }
    return 0;
}
function main(): void {}
"#;
        assert_eq!(
            non_exhaustive(source),
            [
                (
                    "non-exhaustive `switch`: missing case(s) \"b\", \"c\"".to_string(),
                    4
                ),
                (
                    "non-exhaustive `switch`: missing case(s) \"b\" for discriminant `.kind`"
                        .to_string(),
                    8
                ),
                (
                    "non-exhaustive `switch`: missing case(s) \"b\"".to_string(),
                    12
                ),
            ]
        );
    }

    #[test]
    fn a_switch_on_a_typeof_value_or_a_wide_type_need_not_list_every_value() {
        let source = r#"function tag(x: string | number): number {
    switch (typeof x) { case "string": return 1; case "number": return 2; }
    return 0;
}
function wide(x: "a" | string): number {
    switch (x) { case "a": return 1; }
    return 0;
}
function main(): void {}
"#;
        assert_eq!(non_exhaustive(source), []);
    }

    #[test]
    fn a_switch_is_reported_once_wherever_it_is() {
        let switch = r#"switch (k) { case "a": break; }"#;
        let source = format!(
            r#"type K = "a" | "b";
let k: K = "a";
class C {{
    field: number = (() => {{ {switch} return 1; }})();
    method(): void {{ {switch} }}
}}
function loops(): void {{
    for (let i = 0; i < 2; i++) {{ {switch} }}
    while (k === "a") {{ {switch} k = "b"; }}
}}
function guarded(): void {{
    try {{ {switch} }} finally {{ {switch} }}
}}
function hoisting(): void {{
    switch (k) {{
        case "a":
            function inner(): void {{}}
            inner();
            break;
    }}
}}
function main(): void {{
    [1].map((n) => {{ {switch} return n; }});
}}
    {switch}
"#
        );
        let mut lines: Vec<usize> = non_exhaustive(&source)
            .into_iter()
            .map(|(_, line)| line)
            .collect();
        lines.sort_unstable();
        // Line 12 holds two: the `try` block's and the `finally` block's.
        assert_eq!(lines, [4, 5, 8, 9, 12, 12, 15, 23, 25]);
    }
}
