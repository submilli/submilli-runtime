//! Durable cleanup has a fixed package entry point, not a serialized closure.
use super::{Inferer, arena_failure, assignable::assignable};
use crate::compiler_error::CompilerFailure;
use crate::{PackageDeclaration, Type, TypedExprKind, ValueKind};

impl Inferer<'_> {
    pub(super) fn validate_finalizers(
        &mut self,
        declaration: &PackageDeclaration,
    ) -> Result<(), CompilerFailure> {
        let finalizer = declaration.values.get("finalizer");
        let value_type = match finalizer.map(|symbol| &symbol.kind) {
            Some(ValueKind::Function {
                generics,
                params,
                ret,
                ..
            }) if generics.is_empty()
                && params.len() == 2
                && params[0].ty == Type::String
                && params
                    .iter()
                    .all(|p| !p.optional && !p.rest && p.default.is_none())
                && *ret == Type::Void =>
            {
                Some(params[1].ty.clone())
            }
            Some(_) => {
                if let Some(symbol) = finalizer {
                    self.error_with_help(symbol.declaration_span,
                        "invalid package finalizer signature".into(),
                        vec!["export function finalizer(key: string, value: YourSerializableType): void".into()]);
                }
                None
            }
            None => None,
        };
        let register = crate::mangle::package_symbol("submilli:session", "registerFinalizer");
        let mut references = RegistrationReferences {
            register: &register,
            found: Vec::new(),
        };
        crate::typechecker::rules::body_walk::walk_program(&self.typed_ast, &mut references)?;
        for id in references.found {
            let span = self.typed_ast.try_expr(id).map_err(arena_failure)?.span;
            self.error_with_help(span, "registerFinalizer must be called directly".into(),
                vec!["call session.registerFinalizer(key, value) so the compiler can validate value against this package's finalizer parameter".into()]);
        }
        for id in self.typed_ast.expr_ids().map_err(arena_failure)? {
            let expression = self.typed_ast.try_expr(id).map_err(arena_failure)?;
            let TypedExprKind::Call { mangled, args, .. } = &expression.kind else {
                continue;
            };
            if *mangled != register {
                continue;
            }
            let span = expression.span;
            let Some(expected) = &value_type else {
                self.error_with_help(
                    span,
                    "registerFinalizer requires this package to export finalizer(key, value)"
                        .into(),
                    vec![
                        "export function finalizer(key: string, value: YourSerializableType): void"
                            .into(),
                    ],
                );
                continue;
            };
            if let Some(value) = args.get(1) {
                let actual = self
                    .typed_ast
                    .try_expr(*value)
                    .map_err(arena_failure)?
                    .ty
                    .clone();
                if !assignable(&actual, expected, self.resolver()) {
                    self.error(span, format!("registered value `{actual}` does not match finalizer value parameter `{expected}`"));
                }
            }
        }
        Ok(())
    }
}

struct RegistrationReferences<'a> {
    register: &'a crate::MangledName,
    found: Vec<crate::ExprId>,
}
impl crate::typechecker::rules::body_walk::Visitor for RegistrationReferences<'_> {
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
