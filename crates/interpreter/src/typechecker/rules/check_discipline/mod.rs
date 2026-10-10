//! The value `check()` approves must be the value the package goes on to use.
//!
//! A caller can hand a package an object whose getter answers differently on
//! each read, or change data while its own code runs. So `check()` belongs in
//! the functions a caller reaches, and a body that calls it reads each
//! caller-supplied value that reaches `check()` once.

mod bodies;
mod checked;
mod findings;
mod flow;
mod origin;
mod package;
mod scope;
mod stability;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use super::check_calls::{self, CheckCall, SearchRoot};
use crate::compiler_error::CompilerFailure;
use crate::{Diagnostic, ExportKind, ExprId, Span, TypedAst, TypedExprKind};
use bodies::{Body, Exposure};
use findings::{Findings, warning};
use package::PackageFacts;

pub(super) fn run(ta: &TypedAst, diags: &mut Vec<Diagnostic>) -> Result<(), CompilerFailure> {
    if !ta
        .imported_packages
        .contains(crate::stdlib::security::MODULE_NAME)
    {
        return Ok(());
    }
    let package = PackageFacts {
        ta,
        exported_globals: bodies::exported(ta, ExportKind::Global),
    };
    let mut found = Vec::new();
    let bodies = bodies::of(ta, &package.exported_globals)?;
    for body in &bodies {
        check_body(&package, body, &mut found)?;
    }
    let const_functions = bodies.iter().filter_map(|body| body.function_value);
    check_outside_functions(ta, &const_functions.collect(), &mut found)?;
    found.sort_by_key(|diagnostic| {
        let Span { file, start, end } = diagnostic.span;
        (file.0, start, end)
    });
    diags.extend(found);
    Ok(())
}

fn check_body(
    package: &PackageFacts<'_>,
    body: &Body<'_>,
    found: &mut Vec<Diagnostic>,
) -> Result<(), CompilerFailure> {
    let ta = package.ta;
    let mut checks = Vec::new();
    check_calls::collect(ta, body.root, &mut checks)?;
    let Some(first) = checks.first() else {
        return Ok(());
    };
    check_placement(ta, body, &checks, found)?;
    // A nested function that calls `check()` itself takes the caller's
    // values as its parameters.
    let checking_closures = checks
        .iter()
        .filter_map(|call| call.closures.last().copied())
        .collect();
    let checked = checked::analyse(package, body, &checks, &checking_closures)?;
    let mut findings = Findings::new(&body.label, anchor(ta, first)?, &checked);
    flow::analyse(package, body, &checking_closures, &mut findings)?;
    findings.finish(found);
    Ok(())
}

/// One warning for the body's own checks, and one for each nested function
/// that holds any.
fn check_placement(
    ta: &TypedAst,
    body: &Body<'_>,
    checks: &[CheckCall],
    found: &mut Vec<Diagnostic>,
) -> Result<(), CompilerFailure> {
    let label = &body.label;
    let own_check = checks.iter().find(|check| check.closures.is_empty());
    if let Some(check) = own_check
        && let Some(diagnostic) = non_public_body_warning(body, anchor(ta, check)?)
    {
        found.push(diagnostic);
    }
    let mut reported_closures = BTreeSet::new();
    for check in checks {
        let Some(closure) = check.closures.last() else {
            continue;
        };
        if !reported_closures.insert(*closure) {
            continue;
        }
        found.push(warning(
            anchor(ta, check)?,
            format!("`check()` is called inside a nested function in `{label}`"),
            &format!(
                "Call `check()` directly in the body of `{label}`; a nested function may run later, repeatedly, or never"
            ),
            vec![nested_function_note(ta, *closure)?],
        ));
    }
    Ok(())
}

fn non_public_body_warning(body: &Body<'_>, anchor: Span) -> Option<Diagnostic> {
    let label = &body.label;
    let (note, help) = match &body.exposure {
        Exposure::Public => return None,
        Exposure::Unexported { symbol } => (
            format!("`{symbol}` is not exported from the package root"),
            format!(
                "Move the `check()` into each exported function that reaches `{label}`, or export `{symbol}` from the package root"
            ),
        ),
        Exposure::PrivateMember => (
            format!("`{label}` is private"),
            format!(
                "Move the `check()` into each public member that reaches `{label}`, or make `{label}` public"
            ),
        ),
    };
    Some(warning(
        anchor,
        format!("`check()` is called in `{label}`, which is not part of the package's public API"),
        &help,
        vec![(body.label_span, note)],
    ))
}

fn nested_function_note(ta: &TypedAst, closure: ExprId) -> Result<(Span, String), CompilerFailure> {
    let span = ta
        .try_expr(closure)
        .map_err(crate::typechecker::arena_failure)?
        .span;
    let name = ta
        .nested_function_names
        .get(&closure)
        .or_else(|| ta.closure_names.get(&closure));
    Ok(match name {
        Some(name) => (
            name.span,
            format!("the nested function `{}` is declared here", name.name),
        ),
        None => (span, "the nested function starts here".to_string()),
    })
}

/// The checks no body holds. Module-level statements and field initializers
/// run while the package loads, before a caller has supplied anything: only
/// where the check sits is wrong. A check under one of `const_functions`
/// belongs to that body.
fn check_outside_functions(
    ta: &TypedAst,
    const_functions: &BTreeSet<ExprId>,
    found: &mut Vec<Diagnostic>,
) -> Result<(), CompilerFailure> {
    let mut checks = Vec::new();
    for statement in &ta.top_level_statements {
        check_calls::collect(ta, SearchRoot::Stmt(*statement), &mut checks)?;
    }
    for initializer in ta.class_field_initializers() {
        check_calls::collect(ta, SearchRoot::Expr(initializer), &mut checks)?;
    }
    for check in &checks {
        let diagnostic = match check.closures.first() {
            None => warning(
                anchor(ta, check)?,
                "`check()` is called outside a function".to_string(),
                "Call `check()` in the body of an exported function",
                Vec::new(),
            ),
            Some(outermost) if const_functions.contains(outermost) => continue,
            Some(outermost) => warning(
                anchor(ta, check)?,
                "`check()` is called in a function value that no exported function or exported constant names"
                    .to_string(),
                "Call `check()` directly in the body of an exported function",
                vec![unnamed_function_note(ta, *outermost)?],
            ),
        };
        found.push(diagnostic);
    }
    Ok(())
}

fn unnamed_function_note(
    ta: &TypedAst,
    function: ExprId,
) -> Result<(Span, String), CompilerFailure> {
    let span = ta
        .try_expr(function)
        .map_err(crate::typechecker::arena_failure)?
        .span;
    Ok((span, "the function value starts here".to_string()))
}

/// The capability argument, which names the check in the source.
fn anchor(ta: &TypedAst, check: &CheckCall) -> Result<Span, CompilerFailure> {
    let Some(capability) = check.args.first() else {
        return Ok(check.span);
    };
    let capability = ta
        .try_expr(*capability)
        .map_err(crate::typechecker::arena_failure)?;
    Ok(match capability.kind {
        TypedExprKind::String(_) => capability.span,
        _ => check.span,
    })
}
