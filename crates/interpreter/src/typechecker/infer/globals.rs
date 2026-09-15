//! Step 2 — top-level let/const + non-decl statements (source order).

use crate::{ClassMember, StmtKind, TypedStmt, TypedStmtKind, ValueKind};

use super::Inferer;

impl<'a> Inferer<'a> {
    pub(super) fn infer_global_variables(&mut self) {
        let top_level: Vec<_> = self.ast.top_level.clone();
        for stmt_id in top_level {
            let stmt = self.ast.stmt(stmt_id).clone();
            let span = stmt.span;
            match stmt.kind {
                // Functions are deferred to step 3 so their bodies see the
                // complete top-level symbol table.
                StmtKind::Function { .. } => {}
                StmtKind::InterfaceDecl { .. }
                | StmtKind::EnumDecl { .. }
                | StmtKind::TypeAliasDecl { .. } => {}
                // Static fields are module globals: their initializers
                // evaluate here, in source order interleaved with `let`/`const` —
                // the TS module-initialization order.
                StmtKind::ClassDecl { name, members, .. } => {
                    self.infer_static_field_globals(&name, &members);
                }
                StmtKind::Import { .. } => {}
                StmtKind::Let {
                    name,
                    ty,
                    value,
                    doc,
                } => {
                    if self.reject_intrinsic_name(&name) {
                        continue;
                    }
                    let hint = ty.as_ref().map(|a| self.resolve_type(a));
                    let (typed_value, value_ty) = self.infer_expr(value, hint.as_ref());
                    let bound = hint.unwrap_or(value_ty);
                    self.bind_top(
                        &name,
                        ValueKind::Let {
                            ty: bound.clone(),
                            doc: doc.clone(),
                        },
                    );
                    let mangled = self.mangle_top_symbol(&name.name);
                    self.add_typed_global(crate::TypedGlobal {
                        name: name.clone(),
                        mangled_name: mangled.clone(),
                        ty: bound.clone(),
                        kind: crate::GlobalKind::Let,
                        doc,
                        span,
                    });
                    let assign_id = self.typed_ast.push_stmt(TypedStmt {
                        kind: TypedStmtKind::AssignGlobal {
                            ident: name,
                            mangled,
                            target_ty: bound,
                            value: typed_value,
                        },
                        span,
                    });
                    self.typed_ast.top_level_statements.push(assign_id);
                }
                StmtKind::Const {
                    name,
                    ty,
                    value,
                    doc,
                } => {
                    if self.reject_intrinsic_name(&name) {
                        continue;
                    }
                    let hint = ty.as_ref().map(|a| self.resolve_type(a));
                    let (typed_value, value_ty) = self.infer_expr(value, hint.as_ref());
                    let bound = hint.unwrap_or(value_ty);
                    self.bind_top(
                        &name,
                        ValueKind::Const {
                            ty: bound.clone(),
                            doc: doc.clone(),
                        },
                    );
                    let mangled = self.mangle_top_symbol(&name.name);
                    self.add_typed_global(crate::TypedGlobal {
                        name: name.clone(),
                        mangled_name: mangled.clone(),
                        ty: bound.clone(),
                        kind: crate::GlobalKind::Const,
                        doc,
                        span,
                    });
                    let assign_id = self.typed_ast.push_stmt(TypedStmt {
                        kind: TypedStmtKind::AssignGlobal {
                            ident: name,
                            mangled,
                            target_ty: bound,
                            value: typed_value,
                        },
                        span,
                    });
                    self.typed_ast.top_level_statements.push(assign_id);
                }
                _ => {
                    if let Some(typed_id) = self.infer_stmt(stmt_id) {
                        self.typed_ast.top_level_statements.push(typed_id);
                    }
                }
            }
        }
    }

    /// One module global per static field, keyed `Class#static#name`. Fields
    /// whose signature was rejected in `bind_class` (uninitialized) are absent
    /// from `static_fields` and skipped here.
    fn infer_static_field_globals(&mut self, class_name: &crate::Ident, members: &[ClassMember]) {
        let Some(sym) = self.types.lookup(&class_name.name) else {
            return;
        };
        let crate::TypeKind::Class { static_fields, .. } = &sym.kind else {
            return;
        };
        let static_fields = static_fields.clone();
        let class_mangled = sym.mangled_name.clone();
        for member in members {
            let ClassMember::Field {
                name,
                modifiers,
                initializer,
                doc,
                span,
                ..
            } = member
            else {
                continue;
            };
            if modifiers.static_span.is_none() {
                continue;
            }
            let (Some(sig), Some(init)) = (static_fields.get(&name.name), initializer) else {
                continue;
            };
            let prev_static = self
                .current_static
                .replace((class_name.name.clone(), name.name.clone()));
            let (typed_value, value_ty) = self.infer_expr(*init, Some(&sig.ty));
            self.current_static = prev_static;
            if !super::assignable(&value_ty, &sig.ty, self.resolver())
                && !matches!(value_ty, crate::Type::Error)
            {
                let init_span = self.ast.expr(*init).span;
                self.error(
                    init_span,
                    format!(
                        "static field `{}` initializer is `{value_ty}`, expected `{}`",
                        name.name, sig.ty
                    ),
                );
            }
            let ident = crate::Ident {
                name: format!("{}.{}", class_name.name, name.name),
                span: name.span,
            };
            let mangled = crate::mangle::static_member(&class_mangled, &name.name);
            self.add_typed_global(crate::TypedGlobal {
                name: ident.clone(),
                mangled_name: mangled.clone(),
                ty: sig.ty.clone(),
                kind: crate::GlobalKind::Const,
                doc: doc.clone(),
                span: *span,
            });
            let assign_id = self.typed_ast.push_stmt(TypedStmt {
                kind: TypedStmtKind::AssignGlobal {
                    ident,
                    mangled,
                    target_ty: sig.ty.clone(),
                    value: typed_value,
                },
                span: *span,
            });
            self.typed_ast.top_level_statements.push(assign_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{nth_decl_value_ty, run, run_clean};
    use crate::{PackageDeclaration, Type, ValueKind};

    #[test]
    fn let_without_annotation_infers_initializer_type() {
        let ta = run_clean("let x = 1;");
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::Number);
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("x").unwrap().kind {
            ValueKind::Let { ty, .. } => assert_eq!(*ty, Type::Number),
            _ => panic!("expected Let"),
        }
    }

    #[test]
    fn const_without_annotation_infers_initializer_type() {
        let ta = run_clean(r#"const y = "hi";"#);
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::String);
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("y").unwrap().kind {
            ValueKind::Const { ty, .. } => assert_eq!(*ty, Type::String),
            _ => panic!("expected Const"),
        }
    }

    #[test]
    fn top_level_forward_reference_in_initializer_diagnoses() {
        let (ta, diags) = run("let a: number = b; let b: number = 1;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unresolved identifier `b`");
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::Error);
    }

    #[test]
    fn top_level_self_reference_diagnoses() {
        let (_, diags) = run("let a: number = a;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unresolved identifier `a`");
    }

    #[test]
    fn function_body_can_reference_later_let() {
        let _ = run_clean("function f(): number { return r; } let r: number = 1;");
    }

    #[test]
    fn let_initializer_can_call_function_declared_later() {
        let _ = run_clean("let a: number = f(); function f(): number { return 1; }");
    }

    #[test]
    fn let_inferred_from_call_propagates() {
        let ta = run_clean(r#"function f(): string { return "x"; } let s = f();"#);
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("s").unwrap().kind {
            ValueKind::Let { ty, .. } => assert_eq!(*ty, Type::String),
            _ => panic!("expected Let"),
        }
    }

    #[test]
    fn let_inferred_used_later() {
        let _ = run_clean("let a = 1; let b: number = a;");
    }

    #[test]
    fn let_inferred_then_misused() {
        let (_, diags) = run("let a = 1; let b: string = a;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "expected `string`, got `number`");
    }

    #[test]
    fn let_annotation_mismatch_inferred_initializer() {
        let (ta, diags) = run("let a: string = 1;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "expected `string`, got `number`");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("a").unwrap().kind {
            ValueKind::Let { ty, .. } => assert_eq!(*ty, Type::String),
            _ => panic!("expected Let"),
        }
    }
}
