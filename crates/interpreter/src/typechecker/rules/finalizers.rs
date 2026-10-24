//! Package cleanup contracts are checked after type inference.
use super::body_walk::{self, Visitor};
use crate::compiler_error::{CompilerFailure, CompilerStage};
use crate::typechecker::arena_failure;
use crate::typechecker::infer::assignable::{TypeResolver, assignable};
use crate::{
    Diagnostic, PackageDeclaration, Severity, Span, Type, TypedAst, TypedExprKind, ValueKind,
};

/// The type of the value a valid `finalizer(key: string, value: T): void`
/// receives, or `None` when the signature is not one.
pub(crate) fn value_type(kind: &ValueKind) -> Option<&Type> {
    match kind {
        ValueKind::Function {
            generics,
            params,
            ret,
            ..
        } if generics.is_empty()
            && params.len() == 2
            && params[0].ty == Type::String
            && params
                .iter()
                .all(|p| !p.optional && !p.rest && p.default.is_none())
            && *ret == Type::Void =>
        {
            Some(&params[1].ty)
        }
        _ => None,
    }
}

pub(super) fn run(
    typed_ast: &TypedAst,
    declaration: &PackageDeclaration,
    resolver: TypeResolver<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), CompilerFailure> {
    let finalizer = declaration.values.get("finalizer");
    let value_type = finalizer.and_then(|symbol| {
        let ty = value_type(&symbol.kind);
        if ty.is_none() {
            error(
                diagnostics,
                symbol.declaration_span,
                "invalid package finalizer signature".into(),
                vec![
                    "export function finalizer(key: string, value: YourSerializableType): void"
                        .into(),
                ],
            );
        }
        ty
    });
    let register = crate::mangle::package_symbol("submilli:session", "registerFinalizer");
    let mut references = RegistrationReferences {
        register: &register,
        found: Vec::new(),
    };
    body_walk::walk_program(typed_ast, &mut references)?;
    for id in references.found {
        let span = typed_ast.try_expr(id).map_err(arena_failure)?.span;
        error(diagnostics, span, "registerFinalizer must be called directly".into(),
            vec!["call session.registerFinalizer(key, value) so the compiler can validate value against this package's finalizer parameter".into()]);
    }
    for id in typed_ast.expr_ids().map_err(arena_failure)? {
        let expression = typed_ast.try_expr(id).map_err(arena_failure)?;
        let TypedExprKind::Call { mangled, args, .. } = &expression.kind else {
            continue;
        };
        if *mangled != register {
            continue;
        }
        let span = expression.span;
        let Some(expected) = value_type else {
            error(
                diagnostics,
                span,
                "registerFinalizer requires this package to export finalizer(key, value)".into(),
                vec![
                    "export function finalizer(key: string, value: YourSerializableType): void"
                        .into(),
                ],
            );
            continue;
        };
        if let Some(value) = args.get(1) {
            let actual = &typed_ast.try_expr(*value).map_err(arena_failure)?.ty;
            let matches = assignable(actual, expected, resolver);
            resolver
                .limits
                .take()
                .map_err(|limit| limit.into_failure(CompilerStage::Infer, Some(span)))?;
            if !matches {
                error(
                    diagnostics,
                    span,
                    format!(
                        "registered value `{actual}` does not match finalizer value parameter `{expected}`"
                    ),
                    vec![],
                );
            }
        }
    }
    Ok(())
}

fn error(diagnostics: &mut Vec<Diagnostic>, span: Span, message: String, help: Vec<String>) {
    diagnostics.push(Diagnostic {
        severity: Severity::Error,
        span,
        message,
        help,
        notes: vec![],
    });
}

struct RegistrationReferences<'a> {
    register: &'a crate::MangledName,
    found: Vec<crate::ExprId>,
}
impl Visitor for RegistrationReferences<'_> {
    fn visit_expr(
        &mut self,
        id: crate::ExprId,
        kind: &TypedExprKind,
    ) -> Result<(), CompilerFailure> {
        if matches!(kind, TypedExprKind::FunctionRef { mangled, .. } if mangled == self.register) {
            self.found.push(id);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{ModulePath, PackageSourceModule, compile_package};
    fn errors(source: &str) -> String {
        let diagnostics = compile_package(
            "@test/resource",
            ModulePath::from("lib"),
            &[PackageSourceModule {
                path: ModulePath::from("lib"),
                source,
            }],
            &[],
        )
        .unwrap_err();
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn missing_or_invalid_entry_point_is_rejected() {
        assert!(errors(r#"import * as session from "submilli:session"; export function open(): void { session.registerFinalizer("k", 1); }"#).contains("requires this package"));
        assert!(
            errors(r#"export function finalizer(value: string): void {}"#).contains("signature")
        );
    }
    #[test]
    fn argument_type_and_indirect_registration_are_rejected() {
        assert!(errors(r#"import * as session from "submilli:session"; export function open(): void { session.registerFinalizer("k", 1); } export function finalizer(key: string, value: string): void {}"#).contains("does not match"));
        let diagnostic = errors(
            r#"import { registerFinalizer } from "submilli:session"; export function open(): void { const register = registerFinalizer; register("k", 1); } export function finalizer(key: string, value: number): void {}"#,
        );
        assert!(diagnostic.contains("called directly"), "{diagnostic}");
    }
}
