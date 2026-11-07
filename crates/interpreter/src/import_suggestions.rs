//! Import help can be enriched after compilation, when the embedder can lazily
//! consult packages it deliberately left out of the compiler's import table.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Diagnostic, PackageDeclaration, Severity};

#[derive(Default)]
pub struct ImportSuggestions {
    values: BTreeMap<String, BTreeSet<String>>,
    types: BTreeMap<String, BTreeSet<String>>,
    namespaces: BTreeMap<String, BTreeSet<String>>,
}

impl ImportSuggestions {
    pub fn add_package(&mut self, package: &PackageDeclaration) {
        let module = package.package_name.as_str();
        if !module.starts_with("submilli:") && !module.starts_with('@') {
            return;
        }
        for name in package.values.keys().filter(|name| *name != "finalizer") {
            self.values
                .entry(name.clone())
                .or_default()
                .insert(module.into());
        }
        for name in package.types.keys() {
            self.types
                .entry(name.clone())
                .or_default()
                .insert(module.into());
        }
        if let Some(name) = module.rsplit(['/', ':']).next() {
            self.namespaces
                .entry(name.into())
                .or_default()
                .insert(module.into());
        }
    }

    /// These are the compiler's unresolved-name diagnostics, not arbitrary
    /// backtick-quoted names (which also occur in unrelated errors).
    pub fn needed(diagnostics: &[Diagnostic]) -> bool {
        diagnostics
            .iter()
            .any(|diagnostic| unresolved_name(diagnostic).is_some())
    }

    pub fn enrich(&self, source: &str, diagnostics: &mut [Diagnostic]) {
        for diagnostic in diagnostics {
            let Some((name, is_type)) = unresolved_name(diagnostic) else {
                continue;
            };
            let namespace = !is_type
                && source
                    .get(diagnostic.span.end as usize..)
                    .is_some_and(|rest| {
                        let rest = rest.trim_start();
                        rest.starts_with('.') || rest.starts_with("?.")
                    });
            let namespaces = namespace.then(|| self.namespaces.get(name)).flatten();
            let exports = if is_type {
                self.types.get(name)
            } else {
                self.values.get(name)
            };
            let mut help = Vec::new();
            if let Some(packages) = namespaces {
                for package in packages {
                    help.push(namespace_import_help(name, package));
                }
            } else if let Some(packages) = exports {
                for package in packages {
                    help.push(named_import_help(name, package));
                }
            }
            diagnostic.help.retain(|hint| !help.contains(hint));
            diagnostic.help.extend(help);
        }
    }
}

fn unresolved_name(diagnostic: &Diagnostic) -> Option<(&str, bool)> {
    if diagnostic.severity != Severity::Error {
        return None;
    }
    let (rest, is_type) =
        if let Some(rest) = diagnostic.message.strip_prefix("unresolved identifier `") {
            (rest, false)
        } else {
            (diagnostic.message.strip_prefix("unknown type `")?, true)
        };
    Some((rest.strip_suffix('`')?, is_type))
}

pub(crate) fn named_import_help(name: &str, package: &str) -> String {
    format!("`{name}` is exported by `{package}`\n  add `import {{ {name} }} from \"{package}\";`")
}

pub(crate) fn namespace_import_help(name: &str, package: &str) -> String {
    format!("`{name}` is a module; import it first:\n  import {name} from \"{package}\";")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileId, Span};

    fn unresolved(name: &str, source: &str) -> Diagnostic {
        let start = source.find(name).unwrap() as u32;
        Diagnostic {
            severity: Severity::Error,
            span: Span::new(FileId(0), start, start + name.len() as u32).unwrap(),
            message: format!("unresolved identifier `{name}`"),
            help: Vec::new(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn missing_import_hints_merge_in_package_order_without_repeating_existing_help() {
        let mut suggestions = ImportSuggestions::default();
        for name in ["@z/uuid", "@a/uuid"] {
            let mut package = crate::stdlib::uuid::package_declaration();
            package.package_name = name.into();
            suggestions.add_package(&package);
        }
        let source = "v4()";
        let mut diagnostic = unresolved("v4", source);
        diagnostic.help = vec![
            "did you mean `v5`?".into(),
            named_import_help("v4", "@z/uuid"),
        ];
        let mut diagnostics = vec![diagnostic];
        suggestions.enrich(source, &mut diagnostics);
        suggestions.enrich(source, &mut diagnostics);
        assert_eq!(
            diagnostics[0].help,
            vec![
                "did you mean `v5`?".into(),
                named_import_help("v4", "@a/uuid"),
                named_import_help("v4", "@z/uuid"),
            ]
        );
    }

    #[test]
    fn missing_import_hints_only_suggest_namespaces_for_member_access() {
        let mut suggestions = ImportSuggestions::default();
        suggestions.add_package(&crate::stdlib::uuid::package_declaration());
        for source in ["uuid.v4()", "uuid ?.v4()"] {
            let mut diagnostics = vec![unresolved("uuid", source)];
            suggestions.enrich(source, &mut diagnostics);
            assert_eq!(
                diagnostics[0].help,
                vec![namespace_import_help("uuid", "submilli:uuid")]
            );
        }
        let mut diagnostics = vec![unresolved("uuid", "uuid")];
        suggestions.enrich("uuid", &mut diagnostics);
        assert!(diagnostics[0].help.is_empty());
    }

    #[test]
    fn unrelated_errors_and_warnings_do_not_trigger_import_lookup() {
        let mut diagnostic = unresolved("v4", "v4");
        diagnostic.severity = Severity::Warning;
        assert!(!ImportSuggestions::needed(&[diagnostic.clone()]));
        diagnostic.severity = Severity::Error;
        diagnostic.message = "cannot assign `v4`".into();
        assert!(!ImportSuggestions::needed(&[diagnostic]));
    }
}
