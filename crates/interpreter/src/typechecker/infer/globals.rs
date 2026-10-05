//! Step 2 — top-level let/const + non-decl statements (source order).

use crate::compiler_error::CompilerFailure;

use crate::{ClassMember, StmtKind, TypedStmt, TypedStmtKind, ValueKind};

use super::Inferer;

impl<'a> Inferer<'a> {
    pub(super) fn infer_global_variables(&mut self) -> Result<(), CompilerFailure> {
        let top_level: Vec<_> = self.ast.top_level.clone();
        // Assignments to module `let`s narrow them for later top-level
        // statements. Function bodies are inferred after this frame is gone.
        self.push_narrow_frame(super::narrowing::NarrowEnv::new());
        for stmt_id in top_level {
            self.keep_only_global_narrowings();
            let stmt = self
                .ast
                .try_stmt(stmt_id)
                .map_err(super::arena_failure)?
                .clone();
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
                    self.infer_static_field_globals(&name, &members)?;
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
                    let hint = ty.as_ref().map(|a| self.resolve_type(a)).transpose()?;
                    self.keeps_literal_types = true;
                    let (typed_value, value_ty) = self.infer_expr(value, hint.as_ref())?;
                    // Reassignable, so a fresh literal widens; see the block-scoped
                    // `Let` arm in `stmt.rs`.
                    let bound = match hint {
                        Some(hint) => hint,
                        None => self.widen_fresh_literals(typed_value, &value_ty)?,
                    };
                    self.bind_top(
                        &name,
                        ValueKind::Let {
                            ty: bound.clone(),
                            doc: doc.clone(),
                        },
                    )?;
                    let mangled = self.mangle_top_symbol(&name.name)?;
                    let origin =
                        self.initializer_literal_origin(ty.is_some(), typed_value, &bound)?;
                    self.record_global_literal_origin(mangled.clone(), origin);
                    self.typed_ast
                        .rebindable_globals
                        .insert(mangled.clone(), name.name.clone());
                    self.add_typed_global(crate::TypedGlobal {
                        name: name.clone(),
                        mangled_name: mangled.clone(),
                        ty: bound.clone(),
                        kind: crate::GlobalKind::Let,
                        doc,
                        span,
                    })?;
                    self.narrow_global_initializer(&name, &mangled, &bound, typed_value, value_ty)?;
                    let assign_id = self
                        .typed_ast
                        .try_push_stmt(TypedStmt {
                            kind: TypedStmtKind::AssignGlobal {
                                ident: name,
                                mangled,
                                target_ty: bound,
                                value: typed_value,
                            },
                            span,
                        })
                        .map_err(crate::typechecker::arena_failure)?;
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
                    // Keeps the literal types its value passes through; see the
                    // block-scoped `Const` arm in `stmt.rs`.
                    let hint = ty.as_ref().map(|a| self.resolve_type(a)).transpose()?;
                    self.keeps_literal_types = hint.is_none();
                    let (typed_value, value_ty) = self.infer_expr(value, hint.as_ref())?;
                    let origin =
                        self.initializer_literal_origin(ty.is_some(), typed_value, &value_ty)?;
                    let bound = hint.unwrap_or(value_ty);
                    self.bind_top(
                        &name,
                        ValueKind::Const {
                            ty: bound.clone(),
                            doc: doc.clone(),
                        },
                    )?;
                    let mangled = self.mangle_top_symbol(&name.name)?;
                    self.record_global_literal_origin(mangled.clone(), origin);
                    self.add_typed_global(crate::TypedGlobal {
                        name: name.clone(),
                        mangled_name: mangled.clone(),
                        ty: bound.clone(),
                        kind: crate::GlobalKind::Const,
                        doc,
                        span,
                    })?;
                    let assign_id = self
                        .typed_ast
                        .try_push_stmt(TypedStmt {
                            kind: TypedStmtKind::AssignGlobal {
                                ident: name,
                                mangled,
                                target_ty: bound,
                                value: typed_value,
                            },
                            span,
                        })
                        .map_err(crate::typechecker::arena_failure)?;
                    self.typed_ast.top_level_statements.push(assign_id);
                }
                _ => {
                    if let Some(typed_id) = self.infer_stmt(stmt_id)? {
                        self.typed_ast.top_level_statements.push(typed_id);
                    }
                }
            }
        }
        self.pop_narrow_frame()?;
        Ok(())
    }

    /// A module `let` declared as a union starts narrowed to its initializer,
    /// as a local one does, unless the initializer was rejected.
    fn narrow_global_initializer(
        &mut self,
        name: &crate::Ident,
        mangled: &crate::MangledName,
        declared: &crate::Type,
        value: crate::ExprId,
        value_ty: crate::Type,
    ) -> Result<(), CompilerFailure> {
        if !matches!(
            declared.peel(),
            crate::Type::Union(_) | crate::Type::Boolean
        ) || matches!(value_ty, crate::Type::Error)
            || value_ty == *declared
            || !super::assignable(&value_ty, declared, self.resolver())
        {
            return Ok(());
        }
        let flow_ty = self.assigned_flow_type(declared, value, value_ty)?;
        let narrowed = self.initializer_narrowed_ty(declared, flow_ty);
        self.renarrow_global_after_write(name, mangled, declared, narrowed)
    }

    /// Top-level statements are not wrapped in narrowing regions, so only a
    /// module variable's own narrowing, read live, carries to the next one.
    fn keep_only_global_narrowings(&mut self) {
        if let Some(env) = self.narrow_scopes.last_mut() {
            env.retain(|path, _| {
                path.chain.is_empty() && matches!(path.root, super::narrowing::BindingId::Global(_))
            });
        }
    }

    /// One module global per static field, keyed `Class#static#name`. Fields
    /// whose signature was rejected in `bind_class` (uninitialized) are absent
    /// from `static_fields` and skipped here.
    fn infer_static_field_globals(
        &mut self,
        class_name: &crate::Ident,
        members: &[ClassMember],
    ) -> Result<(), CompilerFailure> {
        let Some(sym) = self.types.lookup(&class_name.name) else {
            return Ok(());
        };
        let crate::TypeKind::Class { static_fields, .. } = &sym.kind else {
            return Ok(());
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
            let (typed_value, value_ty) = self.infer_expr(*init, Some(&sig.ty))?;
            self.current_static = prev_static;
            if !super::assignable(&value_ty, &sig.ty, self.resolver())
                && !matches!(value_ty, crate::Type::Error)
            {
                let init_span = self.ast.try_expr(*init).map_err(super::arena_failure)?.span;
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
            if !sig.readonly {
                self.typed_ast
                    .rebindable_globals
                    .insert(mangled.clone(), ident.name.clone());
            }
            self.add_typed_global(crate::TypedGlobal {
                name: ident.clone(),
                mangled_name: mangled.clone(),
                ty: sig.ty.clone(),
                kind: crate::GlobalKind::Const,
                doc: doc.clone(),
                span: *span,
            })?;
            let assign_id = self
                .typed_ast
                .try_push_stmt(TypedStmt {
                    kind: TypedStmtKind::AssignGlobal {
                        ident,
                        mangled,
                        target_ty: sig.ty.clone(),
                        value: typed_value,
                    },
                    span: *span,
                })
                .map_err(crate::typechecker::arena_failure)?;
            self.typed_ast.top_level_statements.push(assign_id);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{nth_decl_value_ty, run, run_clean};
    use crate::{PackageDeclaration, Type, ValueKind};

    #[test]
    fn let_without_annotation_infers_initializer_type() {
        let ta = run_clean("let x = 1;");
        // The initializer keeps its literal type; the binding widens it.
        assert_eq!(
            nth_decl_value_ty(&ta, 0),
            Type::NumberLiteral(crate::types::LiteralF64(1.0))
        );
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("x").unwrap().kind {
            ValueKind::Let { ty, .. } => assert_eq!(*ty, Type::Number),
            _ => panic!("expected Let"),
        }
    }

    /// A `const` cannot be reassigned, so the literal type stays true for the whole
    /// program and is kept — as in TypeScript, where this is what lets a `const` be
    /// passed to a literal-union parameter. `let` widens; see
    /// `let_without_annotation_widens_to_the_base_primitive`.
    #[test]
    fn const_without_annotation_infers_the_literal_type() {
        let ta = run_clean(r#"const y = "hi";"#);
        assert_eq!(
            nth_decl_value_ty(&ta, 0),
            Type::StringLiteral("hi".to_string())
        );
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("y").unwrap().kind {
            ValueKind::Const { ty, .. } => {
                assert_eq!(*ty, Type::StringLiteral("hi".to_string()));
            }
            _ => panic!("expected Const"),
        }
    }

    /// The reason the literal is kept: a `const` reaches a literal-union parameter,
    /// which is what `spec.md` lists literal types as being for.
    #[test]
    fn a_const_literal_is_accepted_by_a_literal_union_parameter() {
        let (_ta, diags) = run(
            "function f(t: \"a\" | \"b\"): number { return t === \"a\" ? 1 : 2; } \
             const t = \"a\"; function main(): void { const n: number = f(t); }",
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    /// A literal still reaches everything its base type does: arithmetic, methods,
    /// template interpolation, and a base-typed parameter.
    #[test]
    fn a_const_literal_behaves_as_its_base_type() {
        let (_ta, diags) = run("function g(s: string): number { return s.length; } \
             function main(): void { const n = 1; const m = n + 1; \
             const s = \"hi\"; const r = s.repeat(2); const t = `${n}`; \
             const l: number = g(s); }");
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn let_without_annotation_widens_to_the_base_primitive() {
        let ta = run_clean(r#"let y = "hi";"#);
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("y").unwrap().kind {
            ValueKind::Let { ty, .. } => assert_eq!(*ty, Type::String),
            _ => panic!("expected Let"),
        }
    }

    /// Only a bare literal keeps its type; a computed initializer widens even under
    /// `const`, matching TypeScript (`const a = 1 + 1` is `number`, not `2`).
    #[test]
    fn const_with_a_computed_initializer_widens() {
        let ta = run_clean("const y = 1 + 1;");
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::Number);
    }

    /// The literal survives the `const` but not the `let`: a reassignable binding
    /// takes the base type, or its first write would be a type error. Asserted on the
    /// binding rather than the initializer, since the initializer expression `a` is
    /// still `1` — it is the binding that widens.
    #[test]
    fn let_initialized_from_a_const_literal_widens() {
        let ta = run_clean("const a = 1; let b = a;");
        let reg = PackageDeclaration::from_typed_ast(&ta);
        match &reg.values.get("b").unwrap().kind {
            ValueKind::Let { ty, .. } => assert_eq!(*ty, Type::Number),
            other => panic!("expected Let, got {other:?}"),
        }
    }

    /// Parentheses group, they do not compute, so the literal survives them.
    #[test]
    fn const_bound_to_a_parenthesized_literal_keeps_the_literal_type() {
        let ta = run_clean("const y = (1);");
        assert_eq!(
            nth_decl_value_ty(&ta, 0),
            Type::NumberLiteral(crate::types::LiteralF64(1.0))
        );
    }

    /// Reassigning a top-level `const` reports the reassignment and nothing else.
    /// Inferring the new value against the const's own literal type would always
    /// mismatch, stacking a spurious `expected 1, got 2` beneath the real error.
    #[test]
    fn reassigning_a_top_level_const_reports_only_the_reassignment() {
        let (_, diags) = run("const a = 1; function main(): void { a = 2; }");
        assert_eq!(diags.len(), 1, "unexpected diagnostics: {diags:?}");
        assert!(
            diags[0].message.contains("cannot assign to const binding"),
            "got: {}",
            diags[0].message
        );
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
