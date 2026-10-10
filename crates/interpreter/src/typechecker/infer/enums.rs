use crate::compiler_error::CompilerFailure;

use crate::{Diagnostic, Ident, Severity, Span, TypeKind, TypeSymbol};

use super::Inferer;

impl<'a> Inferer<'a> {
    pub(super) fn bind_enum(
        &mut self,
        name: Ident,
        members: Vec<crate::EnumMember>,
        doc: Option<crate::DocComment>,
    ) -> Result<(), CompilerFailure> {
        if self.reject_intrinsic_name(&name) {
            return Ok(());
        }
        if let Some(prev) = self.types.lookup(&name.name) {
            let prev_span = prev.declaration_span;
            self.diagnostics.push(Diagnostic {
                severity: Severity::Error,
                span: name.span,
                message: format!("duplicate declaration of type `{}`", name.name),
                help: Vec::new(),
                notes: vec![(prev_span, "previously declared here".into())],
            });
            return Ok(());
        }

        let is_string = members
            .iter()
            .any(|m| matches!(m.value, Some(crate::EnumInitializer::String { .. })));
        let _: () = if is_string {
            self.bind_string_enum(name, members, doc)?;
        } else {
            self.bind_number_enum(name, members, doc)?;
        };
        Ok(())
    }

    fn bind_number_enum(
        &mut self,
        name: Ident,
        members: Vec<crate::EnumMember>,
        doc: Option<crate::DocComment>,
    ) -> Result<(), CompilerFailure> {
        let mut variants: Vec<(String, f64)> = Vec::with_capacity(members.len());
        let mut seen: std::collections::BTreeMap<String, Span> = std::collections::BTreeMap::new();
        let mut next_implicit: f64 = 0.0;
        let mut typed_members: Vec<crate::TypedNumberEnumMember> =
            Vec::with_capacity(members.len());
        for member in members {
            if let Some(prev_span) = seen.get(&member.name.name) {
                let prev = *prev_span;
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    span: member.name.span,
                    message: format!(
                        "duplicate variant `{}` on enum `{}`",
                        member.name.name, name.name,
                    ),
                    help: Vec::new(),
                    notes: vec![(prev, "previously declared here".into())],
                });
                continue;
            }
            seen.insert(member.name.name.clone(), member.name.span);

            let value: f64 = match &member.value {
                Some(crate::EnumInitializer::Number { value, .. }) => {
                    next_implicit = value + 1.0;
                    *value
                }
                None => {
                    let v = next_implicit;
                    next_implicit += 1.0;
                    v
                }
                // parser already rejected mixed-kind decls; skip defensively.
                Some(crate::EnumInitializer::String { .. }) => continue,
            };
            variants.push((member.name.name.clone(), value));
            typed_members.push(crate::TypedNumberEnumMember {
                name: member.name,
                value,
                doc: member.doc,
            });
        }

        let typed_decl = crate::TypedTypeDecl::NumberEnum(crate::TypedNumberEnumDecl {
            name: name.clone(),
            members: typed_members,
            doc: doc.clone(),
        });
        let mangled = self.mangle_top_symbol(&name.name)?;
        let symbol = TypeSymbol {
            name: name.name.clone(),
            mangled_name: mangled,
            declaration_span: name.span,
            kind: TypeKind::NumberEnum { variants, doc },
        };
        self.add_typed_type_decl(typed_decl, symbol.clone())?;
        self.types
            .insert(name.name.clone(), self.package_name.to_string(), symbol);

        Ok(())
    }

    fn bind_string_enum(
        &mut self,
        name: Ident,
        members: Vec<crate::EnumMember>,
        doc: Option<crate::DocComment>,
    ) -> Result<(), CompilerFailure> {
        let mut variants: Vec<(String, String)> = Vec::with_capacity(members.len());
        let mut seen: std::collections::BTreeMap<String, Span> = std::collections::BTreeMap::new();
        let mut typed_members: Vec<crate::TypedStringEnumMember> =
            Vec::with_capacity(members.len());
        for member in members {
            if let Some(prev_span) = seen.get(&member.name.name) {
                let prev = *prev_span;
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Error,
                    span: member.name.span,
                    message: format!(
                        "duplicate variant `{}` on enum `{}`",
                        member.name.name, name.name,
                    ),
                    help: Vec::new(),
                    notes: vec![(prev, "previously declared here".into())],
                });
                continue;
            }
            seen.insert(member.name.name.clone(), member.name.span);

            let value: String = match &member.value {
                Some(crate::EnumInitializer::String { value, .. }) => value.clone(),
                None => {
                    self.error_with_help(
                        member.name.span,
                        format!(
                            "string enum member `{}` requires an explicit value",
                            member.name.name,
                        ),
                        vec![format!(
                            "add `= \"{}\"` (or any string literal) after the member name",
                            member.name.name,
                        )],
                    );
                    continue;
                }
                // parser already rejected mixed-kind decls; skip defensively.
                Some(crate::EnumInitializer::Number { .. }) => continue,
            };
            variants.push((member.name.name.clone(), value.clone()));
            typed_members.push(crate::TypedStringEnumMember {
                name: member.name,
                value,
                doc: member.doc,
            });
        }

        let typed_decl = crate::TypedTypeDecl::StringEnum(crate::TypedStringEnumDecl {
            name: name.clone(),
            members: typed_members,
            doc: doc.clone(),
        });
        let mangled = self.mangle_top_symbol(&name.name)?;
        let symbol = TypeSymbol {
            name: name.name.clone(),
            mangled_name: mangled,
            declaration_span: name.span,
            kind: TypeKind::StringEnum { variants, doc },
        };
        self.add_typed_type_decl(typed_decl, symbol.clone())?;
        self.types
            .insert(name.name.clone(), self.package_name.to_string(), symbol);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{nth_decl_value_ty, run, run_clean};
    use crate::{Type, TypedAst, TypedExprKind, TypedStmtKind};

    fn find_typed_number_enum<'a>(
        ta: &'a TypedAst,
        name: &str,
    ) -> Option<&'a crate::TypedNumberEnumDecl> {
        ta.types.iter().find_map(|t| match t {
            crate::TypedTypeDecl::NumberEnum(e) if e.name.name == name => Some(e),
            _ => None,
        })
    }

    #[test]
    fn enum_member_access_has_member_type() {
        let ta = run_clean(
            r#"
            enum Direction { Up, Down }
            const d: Direction = Direction.Up;
            "#,
        );
        // `d` is the first top-level binding in the typed AST
        // (interface/enum decls don't push into top_level).
        let t = nth_decl_value_ty(&ta, 0);
        assert_eq!(
            t,
            Type::number_enum(
                crate::Package::user(),
                "Direction",
                crate::mangle::package_symbol(crate::mangle::USER_PACKAGE, "Direction"),
            )
            .with_enum_member(
                "Up",
                crate::types::EnumValue::Number(crate::types::LiteralF64(0.0)),
                2,
            )
        );
    }

    #[test]
    fn enum_auto_numbers_from_zero() {
        let ta = run_clean("enum D { Up, Down }");
        let e = find_typed_number_enum(&ta, "D").expect("enum D registered");
        assert_eq!(e.members.len(), 2);
        assert_eq!(e.members[0].name.name, "Up");
        assert_eq!(e.members[0].value, 0.0);
        assert_eq!(e.members[1].name.name, "Down");
        assert_eq!(e.members[1].value, 1.0);
    }

    #[test]
    fn enum_auto_number_continues_after_explicit() {
        let ta = run_clean("enum D { A, B = 5, C }");
        let e = find_typed_number_enum(&ta, "D").expect("enum D registered");
        let values: Vec<f64> = e.members.iter().map(|m| m.value).collect();
        assert_eq!(values, vec![0.0, 5.0, 6.0]);
    }

    #[test]
    fn string_enum_requires_explicit_value() {
        let (_, diags) = run(r#"enum S { A = "a", B }"#);
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("string enum member `B` requires an explicit value")),
            "expected missing-value diagnostic for `B`, got: {diags:?}",
        );
    }

    #[test]
    fn enums_are_nominal() {
        let (_, diags) = run(r#"
            enum A { X }
            enum B { X }
            const a: A = A.X;
            const b: B = a;
            "#);
        assert!(
            !diags.is_empty(),
            "expected assignment error from cross-enum binding"
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("A") && d.message.contains("B")),
            "expected diagnostic naming both enum types, got: {diags:?}",
        );
    }

    #[test]
    fn enum_unknown_variant_lists_alternatives() {
        let (_, diags) = run(r#"
            enum Direction { Up, Down }
            function main(): void {
                const d: Direction = Direction.Sideways;
            }
            "#);
        let d = diags
            .iter()
            .find(|d| {
                d.message
                    .contains("no variant `Sideways` on enum `Direction`")
            })
            .expect("unknown-variant diagnostic missing");
        let help_text = d.help.join(" ");
        assert!(
            help_text.contains("Direction.Up") && help_text.contains("Direction.Down"),
            "help should list known variants, got: {help_text:?}",
        );
    }

    #[test]
    fn bare_enum_name_used_as_value_diagnoses() {
        let (_, diags) = run(r#"
            enum Direction { Up, Down }
            function main(): void {
                const x = Direction;
            }
            "#);
        let d = diags
            .iter()
            .find(|d| {
                d.message
                    .contains("`Direction` is an enum type, not a value")
            })
            .expect("enum-as-value diagnostic missing");
        let help_text = d.help.join(" ");
        assert!(
            help_text.contains("Direction.Up") && help_text.contains("Direction.Down"),
            "help should list variants, got: {help_text:?}",
        );
    }

    #[test]
    fn enum_member_lowers_to_typed_number_enum_member() {
        let ta = run_clean(
            r#"
            enum D { Up = 1, Down = -1 }
            const d: D = D.Down;
            "#,
        );
        let stmt = ta.try_stmt(ta.top_level_statements[0]).unwrap();
        let value_id = match &stmt.kind {
            TypedStmtKind::AssignGlobal { value, .. } => *value,
            other => panic!("expected AssignGlobal, got {other:?}"),
        };
        match &ta.try_expr(value_id).unwrap().kind {
            TypedExprKind::NumberEnumMember {
                enum_mangled,
                variant,
                value,
            } => {
                assert_eq!(enum_mangled.as_str(), "main#D");
                assert_eq!(variant.name, "Down");
                assert_eq!(*value, -1.0);
            }
            other => panic!("expected NumberEnumMember, got {other:?}"),
        }
    }

    #[test]
    fn string_enum_member_lowers_to_typed_string_enum_member() {
        let ta = run_clean(
            r#"
            enum S { Active = "active", Inactive = "inactive" }
            const s: S = S.Active;
            "#,
        );
        let stmt = ta.try_stmt(ta.top_level_statements[0]).unwrap();
        let value_id = match &stmt.kind {
            TypedStmtKind::AssignGlobal { value, .. } => *value,
            other => panic!("expected AssignGlobal, got {other:?}"),
        };
        match &ta.try_expr(value_id).unwrap().kind {
            TypedExprKind::StringEnumMember {
                enum_mangled,
                variant,
                value,
            } => {
                assert_eq!(enum_mangled.as_str(), "main#S");
                assert_eq!(variant.name, "Active");
                assert_eq!(value, "active");
            }
            other => panic!("expected StringEnumMember, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_enum_decl_diagnoses() {
        let (_, diags) = run(r#"
            enum D { A }
            enum D { B }
            "#);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("duplicate declaration of type `D`")),
            "expected duplicate-decl diagnostic, got: {diags:?}",
        );
    }

    #[test]
    fn duplicate_variant_diagnoses() {
        let (_, diags) = run("enum D { A, A }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("duplicate variant `A`")),
            "expected duplicate-variant diagnostic, got: {diags:?}",
        );
    }
}
