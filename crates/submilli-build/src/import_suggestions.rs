//! Lazy diagnostic lookup. Construction does no I/O; successful programs keep
//! their selective package loading. Only error diagnostics consult the store.

use std::collections::BTreeSet;

use interpreter::{Diagnostic, import_suggestions::ImportSuggestions};

use crate::artifact::{ArtifactReadBudget, ArtifactReadLimits};
use crate::{PackageStore, PackageStoreError};

#[derive(Clone, Debug)]
pub struct ImportSuggestionSource {
    store: PackageStore,
    packages: BTreeSet<String>,
    git_enabled: bool,
}

impl ImportSuggestionSource {
    pub fn new(store: PackageStore, packages: BTreeSet<String>, git_enabled: bool) -> Self {
        Self {
            store,
            packages,
            git_enabled,
        }
    }

    /// Unavailable artifacts cannot supply hints. Preserve the original compile
    /// errors and return lookup failures for the embedder's operator log.
    pub fn enrich(&self, source: &str, diagnostics: &mut [Diagnostic]) -> Vec<PackageStoreError> {
        if !ImportSuggestions::needed(diagnostics) {
            return Vec::new();
        }
        let mut suggestions = ImportSuggestions::default();
        for declaration in interpreter::runtime::stdlib_package_declarations() {
            if matches!(
                declaration.package_name.as_str(),
                "submilli:secrets" | "submilli:variables"
            ) || (!self.git_enabled && declaration.package_name == "submilli:git")
            {
                continue;
            }
            suggestions.add_package(&declaration);
        }
        let mut budget = ArtifactReadBudget::new(ArtifactReadLimits::default());
        let mut failures = Vec::new();
        for name in &self.packages {
            match self.store.load_with_budget(name, &mut budget) {
                Ok(artifact) => suggestions.add_package(&artifact.package_declaration),
                Err(error) => failures.push(error),
            }
        }
        suggestions.enrich(source, diagnostics);
        failures
    }
}
