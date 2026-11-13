//! Context for otherwise indistinguishable type names in mismatch diagnostics.

use super::Inferer;
use crate::lower_local_types::display_type_name;
use crate::rendering::RenderError;
use crate::{MangledName, Package, Span, Type};

impl Inferer<'_> {
    pub(super) fn mismatch_message(&self, expected: &Type, got: &Type) -> String {
        self.mismatch_details(expected, got).0
    }

    pub(super) fn error_type_mismatch(
        &mut self,
        span: Span,
        expected: &Type,
        got: &Type,
        help: Vec<String>,
    ) {
        self.error_type_mismatch_with_context(span, "", expected, got, help);
    }

    pub(super) fn error_type_mismatch_with_context(
        &mut self,
        span: Span,
        context: &str,
        expected: &Type,
        got: &Type,
        mut help: Vec<String>,
    ) {
        let (message, collision) = self.mismatch_details(expected, got);
        if let Some((expected_name, got_name)) = collision {
            let expected_shape = self.mismatch_shape(expected_name);
            let got_shape = self.mismatch_shape(got_name);
            let diff = if expected_shape == got_shape {
                None
            } else {
                self.render_optional_help(super::type_diff::format_type_diff(
                    &expected_shape,
                    &got_shape,
                ))
            };
            if let Some(diff) = diff {
                if expected.to_string() == expected_name.to_string() {
                    help.push(diff);
                } else {
                    help.push(format!("difference in `{expected_name}`:\n{diff}"));
                }
            } else if expected_shape.to_string() != got_shape.to_string() {
                help.push(format!(
                    "underlying types: expected `{expected_shape}`, got `{got_shape}`"
                ));
            } else {
                help.push(format!(
                    "expected declaration:\n{}\nactual declaration:\n{}",
                    self.format_definition(expected_name),
                    self.format_definition(got_name)
                ));
            }
        }
        self.error_with_help(span, format!("{context}{message}"), help);
    }

    fn mismatch_details<'t>(
        &self,
        expected: &'t Type,
        got: &'t Type,
    ) -> (String, Option<(&'t Type, &'t Type)>) {
        let expected_text = expected.to_string();
        let got_text = got.to_string();
        let collision = if expected_text == got_text {
            match colliding_names(expected, got) {
                Ok(pair) => pair,
                Err(error) => {
                    self.render_help(Err(error));
                    None
                }
            }
        } else {
            None
        };
        let message = match collision {
            Some((expected_name, got_name)) => format!(
                "expected `{expected_text}`{}, got `{got_text}`{}",
                self.mismatch_origin(&expected_text, expected_name),
                self.mismatch_origin(&got_text, got_name),
            ),
            None => super::diagnostics::mismatch_message(&expected_text, &got_text),
        };
        (message, collision)
    }

    fn mismatch_origin(&self, whole: &str, named: &Type) -> String {
        let origin = self.type_origin(named);
        if whole == named.to_string() {
            origin
        } else {
            format!(" (`{named}`{origin})")
        }
    }

    fn type_origin(&self, ty: &Type) -> String {
        let Some((name, mangled, package)) = named_type(ty) else {
            return String::new();
        };
        if package.0 != self.package_name {
            return format!(" from `{}`", package.0);
        }
        let Some(symbol) = self
            .type_registry
            .lookup(mangled)
            .or_else(|| self.types.lookup_by_mangled(mangled))
        else {
            return format!(" from `{}`", package.0);
        };
        let span = symbol.declaration_span;
        let file = self
            .diagnostic_sources
            .and_then(|sources| sources.get(span.file));
        let source = file.map_or(self.source, crate::source::SourceFile::text);
        let Some(prefix) = source.get(..span.start as usize) else {
            return format!(
                " (`{}` declared at byte {})",
                display_type_name(name),
                span.start
            );
        };
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        match file {
            Some(file) => format!(" declared in `{}` at line {line}", file.path),
            None => format!(" declared at line {line}"),
        }
    }

    fn mismatch_shape(&self, ty: &Type) -> Type {
        match ty.peel() {
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            }
            | Type::ClassRef {
                mangled,
                name,
                args,
                ..
            } => self.structural_form(mangled, name, args).map_or_else(
                || ty.clone(),
                |fields| {
                    let (index, more_indexes) =
                        crate::split_index_signatures(self.resolver().index_signatures(ty.peel()));
                    Type::Object {
                        more_indexes,
                        fields,
                        index,
                    }
                },
            ),
            shape => shape.clone(),
        }
    }
}

/// Compare names across the two sides, preserving their expected/got order.
/// Walk borrowed nodes with the same bounds used by diagnostic type rendering.
fn colliding_names<'t>(
    expected: &'t Type,
    got: &'t Type,
) -> Result<Option<(&'t Type, &'t Type)>, RenderError> {
    let expected_names = named_types(expected)?;
    let got_names = named_types(got)?;
    for (expected, got) in expected_names.into_iter().zip(got_names) {
        let expected_identity = super::type_diff::named_identity(expected);
        let got_identity = super::type_diff::named_identity(got);
        if let (Some((expected_name, expected_id)), Some((got_name, got_id))) =
            (expected_identity, got_identity)
            && display_type_name(expected_name) == display_type_name(got_name)
            && expected_id != got_id
        {
            return Ok(Some((expected, got)));
        }
    }
    Ok(None)
}

fn named_types(ty: &Type) -> Result<Vec<&Type>, RenderError> {
    crate::type_rendering::check_for_copy(ty)?;
    let mut names = Vec::new();
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if named_type(ty).is_some() {
            names.try_reserve(1).map_err(|_| RenderError::Allocation)?;
            names.push(ty);
        }
        pending
            .try_reserve(crate::type_size::child_count(ty))
            .map_err(|_| RenderError::Allocation)?;
        match ty {
            Type::Alias { args, .. }
            | Type::AliasRef { args, .. }
            | Type::InterfaceRef { args, .. }
            | Type::ClassRef { args, .. } => {
                pending.extend(args);
            }
            _ => crate::type_size::for_each_child(ty, |child| pending.push(child)),
        }
    }
    Ok(names)
}

fn named_type(ty: &Type) -> Option<(&str, &MangledName, &Package)> {
    match ty {
        Type::Alias {
            name,
            mangled,
            package,
            ..
        }
        | Type::AliasRef {
            name,
            mangled,
            package,
            ..
        }
        | Type::InterfaceRef {
            name,
            mangled,
            package,
            ..
        }
        | Type::ClassRef {
            name,
            mangled,
            package,
            ..
        }
        | Type::NumberEnum {
            name,
            mangled,
            package,
            ..
        }
        | Type::StringEnum {
            name,
            mangled,
            package,
            ..
        } => Some((name, mangled, package)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    fn diagnostics(source: &'static str) -> Vec<crate::Diagnostic> {
        crate::type_size::tests::on_compiler_stack(move || {
            crate::compile::compile_script_checked(source, "names.ts", crate::FileId(0), &[], &[])
                .unwrap_err()
                .diagnostics
        })
    }

    #[test]
    fn same_name_diagnostic_names_declarations_and_field_difference() {
        let diagnostics = diagnostics(
            "interface T { value: number }\nfunction main(): void {\ninterface T { value: string }\nconst local: T = { value: \"x\" }; take(local);\n}\nfunction take(value: T): void {}",
        );
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(
            diagnostics[0].message,
            "expected `T` declared at line 1, got `T` declared at line 3"
        );
        assert!(
            diagnostics[0]
                .help
                .iter()
                .any(|help| help.contains("`value`: expected `number`, got `string`")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn already_distinct_names_keep_their_message() {
        let diagnostics = diagnostics(
            "interface Expected { x: number }\ninterface Actual { y: string }\nfunction main(): void { const actual: Actual = { y: \"x\" }; take(actual); }\nfunction take(value: Expected): void {}",
        );
        assert_eq!(diagnostics[0].message, "expected `Expected`, got `Actual`");
    }

    #[test]
    fn repeated_names_in_a_tuple_keep_each_position() {
        let outer = crate::Type::interface_ref(
            crate::Package("main".into()),
            "T",
            crate::mangle::package_symbol("main", "T"),
            vec![],
        );
        let inner = crate::Type::interface_ref(
            crate::Package("main".into()),
            "T%1%0",
            crate::mangle::package_symbol("main", "T%1%0"),
            vec![],
        );
        let expected = crate::Type::Tuple(crate::types::TupleType {
            elements: vec![outer.clone(), outer.clone()],
            optional: 0,
        });
        let got = crate::Type::Tuple(crate::types::TupleType {
            elements: vec![inner.clone(), outer],
            optional: 0,
        });
        let (expected_name, got_name) = super::colliding_names(&expected, &got).unwrap().unwrap();
        assert_eq!(got_name, &inner);
        assert_ne!(expected_name, got_name);
    }

    #[test]
    fn same_named_aliases_explain_their_underlying_types() {
        let diagnostics = diagnostics(
            "type T = number;\nfunction take(value: T): void {}\nfunction main(): void {\ntype T = string;\nconst local: T = \"x\"; take(local);\n}",
        );
        assert_eq!(
            diagnostics[0].message,
            "expected `T` declared at line 1, got `T` declared at line 4"
        );
        assert!(
            diagnostics[0]
                .help
                .iter()
                .any(|help| help.contains("underlying types: expected `number`, got `string`")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn module_origins_use_the_declaring_files() {
        crate::type_size::tests::on_compiler_stack(|| {
            let errors = crate::compile::compile_package("cards", "lib".into(), &[
                crate::PackageSourceModule { path: "lib".into(), source: "import { card } from \"./other\";\ninterface T { id: string }\nconst local: T = card();\n" },
                crate::PackageSourceModule { path: "other".into(), source: "/** A card from the other module. */\nexport interface T {\n/** Its value. */\nvalue: number;\n}\n/** Make a card.\n * @returns A card.\n */\nexport function card(): T { return { value: 1 }; }\n" },
            ], &[]).unwrap_err();
            let error = errors
                .iter()
                .find(|diagnostic| diagnostic.severity == crate::Severity::Error)
                .unwrap();
            assert_eq!(
                error.message,
                "expected `T` declared in `lib` at line 2, got `T` declared in `other` at line 2"
            );
            assert!(
                error
                    .help
                    .iter()
                    .any(|help| help.contains("missing field(s): `id`")),
                "{error:?}"
            );
        });
    }

    #[test]
    fn index_signature_difference_is_retained() {
        let diagnostics = diagnostics(
            "interface T { [key: string]: string }\nfunction take(value: T): void {}\nfunction main(): void {\ninterface T { [key: string]: number }\nconst local: T = { value: 1 }; take(local);\n}",
        );
        assert_eq!(
            diagnostics[0].message,
            "expected `T` declared at line 1, got `T` declared at line 4"
        );
        let help = diagnostics[0].help.join("\n");
        assert!(
            help.contains("expected `{ [key: string]: string }`, got `{ [key: string]: number }`"),
            "{help}"
        );
        assert!(!help.contains("no structural difference"), "{help}");
    }

    #[test]
    fn two_block_scoped_types_name_their_lines() {
        let diagnostics = diagnostics(
            "function main(): void {\ninterface T { id: string }\nfunction take(value: T): void {}\n{\ninterface T { value: number }\nconst local: T = { value: 1 }; take(local);\n}\n}",
        );
        assert_eq!(
            diagnostics[0].message,
            "expected `T` declared at line 2, got `T` declared at line 5"
        );
    }

    #[test]
    fn nested_type_origins_do_not_misidentify_the_wrapper() {
        let diagnostics = diagnostics(
            "interface Wrapper<U> { value: U }\ninterface T { x: number }\nfunction take(value: Wrapper<T>): void {}\nfunction main(): void {\ninterface T { y: string }\nconst local: Wrapper<T> = { value: { y: \"x\" } }; take(local);\n}",
        );
        assert_eq!(
            diagnostics[0].message,
            "expected `Wrapper<T>` (`T` declared at line 2), got `Wrapper<T>` (`T` declared at line 5)"
        );
        assert!(
            diagnostics[0]
                .help
                .iter()
                .any(|help| help.contains("difference in `T`:")
                    && help.contains("missing field(s): `x`")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn package_mismatch_explains_missing_field() {
        crate::type_size::tests::on_compiler_stack(|| {
            let venmo = crate::compile::compile_package(
                "@appworld/venmo",
                "lib".into(),
                &[crate::PackageSourceModule {
                    path: "lib".into(),
                    source: include_str!(
                        "../../../tests/fixtures/same_name_package_origins/venmo/lib.ts"
                    ),
                }],
                &[],
            )
            .unwrap();
            let supervisor = crate::compile::compile_package(
                "@appworld/supervisor",
                "lib".into(),
                &[crate::PackageSourceModule {
                    path: "lib".into(),
                    source: include_str!(
                        "../../../tests/fixtures/same_name_package_origins/supervisor/lib.ts"
                    ),
                }],
                &[],
            )
            .unwrap();
            let error = crate::compile::compile_script_checked(
                include_str!("../../../tests/fixtures/same_name_package_origins/app/main.ts"),
                "main.ts",
                crate::FileId(0),
                &[&venmo.declaration, &supervisor.declaration],
                &[],
            )
            .unwrap_err();
            assert_eq!(error.diagnostics.len(), 1, "{error:?}");
            assert_eq!(
                error.diagnostics[0].message,
                "expected `PaymentCard` from `@appworld/venmo`, got `PaymentCard` from `@appworld/supervisor`"
            );
            assert!(
                error.diagnostics[0]
                    .help
                    .iter()
                    .any(|help| help.contains("missing field(s): `card_id`")),
                "{error:?}"
            );
        });
    }
}
