//! Which capability names a caller may hold a rule for. The policy engine
//! matches names verbatim, so a rule for a name nothing lists for the caller
//! never matches, except an HTTP method `http.request` gates.

use interpreter::stdlib::capabilities;
use submilli_blueprint::Blueprint;

use super::declared_packages::{self, DeclaredPackages};

/// How a capability name falls outside what the catalog, the declared MCP
/// servers, and the packages a caller can reach list for it.
pub(super) enum UnlistedName<'a> {
    /// Only `dependency`, which no `packages:` entry declares, provides it, so
    /// only `dependents` can call it.
    DependencyOnly {
        dependency: &'a str,
        dependents: Vec<&'a str>,
    },
    /// `http.request` gates any method as `http.<method>`, lowercased, so a
    /// name the catalog lacks still matches a call with that `method`.
    UncatalogedHttpMethod { method: String },
    /// An `http.<method>` name an edit or two from `intended`, a cataloged
    /// HTTP operation: as written it matches only calls with `method`, which
    /// a misspelled `deny` lets through.
    MisspelledHttpOperation {
        intended: &'static str,
        method: String,
    },
    /// Nothing reachable provides it; `near` are known names it may misspell.
    Unknown { near: Vec<String> },
}

/// `None` when the stdlib catalog, a declared MCP server, or a package
/// `caller` can reach provides `name`: for `main`, a declared package; for a
/// package, also a package a declared one depends on.
pub(super) fn unlisted_name<'a>(
    blueprint: &Blueprint,
    packages: &'a DeclaredPackages,
    caller: &str,
    name: &str,
) -> Option<UnlistedName<'a>> {
    let known = known_capabilities(blueprint, packages, caller);
    if known.iter().any(|k| k == name) {
        return None;
    }
    // Ahead of the dependency check: a package may document an `http.<method>`
    // name too, and `main` still reaches it through `http.request`.
    if let Some(method) = capabilities::uncataloged_http_method(name) {
        let method = method.to_ascii_uppercase();
        return Some(match misspelled_http_operation(name) {
            Some(intended) => UnlistedName::MisspelledHttpOperation { intended, method },
            None => UnlistedName::UncatalogedHttpMethod { method },
        });
    }
    if let Some(dependency) = dependency_providing(packages, name) {
        return Some(UnlistedName::DependencyOnly {
            dependency,
            dependents: dependents_of(packages, dependency),
        });
    }
    Some(UnlistedName::Unknown {
        near: suggestions(&known, name),
    })
}

/// Every capability name `caller` may hold a rule for.
fn known_capabilities(
    blueprint: &Blueprint,
    packages: &DeclaredPackages,
    caller: &str,
) -> Vec<String> {
    let mut names: Vec<String> = capabilities::catalog()
        .iter()
        .flat_map(|g| g.capabilities)
        .filter(|c| !c.is_template())
        .map(|c| c.name.to_string())
        .collect();
    names.extend(blueprint.mcp.keys().map(|server| format!("mcp.{server}")));
    // `main` imports only declared packages, so a `main` rule for what only a
    // dependency provides could never match.
    let reaches_dependencies = caller != interpreter::mangle::USER_PACKAGE;
    for (package, artifact) in &packages.artifacts {
        if packages.dependencies.contains(package) && !reaches_dependencies {
            continue;
        }
        names.extend(
            artifact
                .capabilities
                .provides
                .iter()
                .map(|p| p.name.clone()),
        );
    }
    names
}

/// The undeclared dependency that provides `name`, if any.
fn dependency_providing<'a>(packages: &'a DeclaredPackages, name: &str) -> Option<&'a str> {
    packages
        .artifacts
        .iter()
        .filter(|(package, _)| packages.dependencies.contains(*package))
        .find(|(_, artifact)| {
            artifact
                .capabilities
                .provides
                .iter()
                .any(|provided| provided.name == name)
        })
        .map(|(package, _)| package.as_str())
}

/// The loaded packages that depend on `dependency` directly.
fn dependents_of<'a>(packages: &'a DeclaredPackages, dependency: &str) -> Vec<&'a str> {
    packages
        .artifacts
        .iter()
        .filter(|(_, artifact)| declared_packages::depends_on(artifact, dependency))
        .map(|(package, _)| package.as_str())
        .collect()
}

/// Names sharing the input's `module.` prefix, or containing it as a
/// substring — enough to catch a typo such as `fs.raed`.
fn suggestions(known: &[String], input: &str) -> Vec<String> {
    let prefix = input.split('.').next().unwrap_or(input);
    known
        .iter()
        .filter(|k| k.starts_with(&format!("{prefix}.")) || k.contains(input))
        .take(5)
        .cloned()
        .collect()
}

/// What a rule for an `http.<method>` name the catalog lacks matches, worded
/// to follow a reference to the rule.
pub(super) fn http_method_scope(method: &str) -> String {
    format!("matches only `http.request` calls with method `{method}`, through `http.<method>`")
}

/// Why a rule for a name [`UnlistedName::MisspelledHttpOperation`] reports is
/// suspect, worded to follow a reference to the rule.
pub(super) fn http_misspelling(intended: &str, method: &str) -> String {
    format!(
        "looks like a misspelling of `{intended}`; as written it {}",
        http_method_scope(method)
    )
}

/// The cataloged HTTP operation `name` is closest to, when at most two edits
/// separate them. Every registered method the catalog lacks, such as `trace`
/// or `propfind`, is further than that, except HTTP/2's connection-preface
/// `PRI`, which no request sends.
fn misspelled_http_operation(name: &str) -> Option<&'static str> {
    capabilities::catalog()
        .iter()
        .flat_map(|group| group.capabilities)
        .filter(|capability| capability.name.starts_with("http.") && !capability.is_template())
        .map(|capability| (edit_distance(name, capability.name), capability.name))
        .filter(|(distance, _)| *distance <= 2)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, intended)| intended)
}

/// Levenshtein distance over bytes; capability names are ASCII.
fn edit_distance(a: &str, b: &str) -> usize {
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (row, left) in (1..).zip(a.bytes()) {
        let mut current = Vec::with_capacity(previous.len());
        current.push(row);
        for (right, pair) in b.bytes().zip(previous.windows(2)) {
            let &[diagonal, above] = pair else { continue };
            let before = current.last().copied().unwrap_or(row);
            let substitution = diagonal + usize::from(left != right);
            current.push(substitution.min(above + 1).min(before + 1));
        }
        previous = current;
    }
    previous.last().copied().unwrap_or(0)
}

/// A clause offering `near` as corrections, or nothing when there are none.
pub(super) fn did_you_mean(near: &[String]) -> String {
    if near.is_empty() {
        return String::new();
    }
    format!("; did you mean: {}?", near.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_misses_of_cataloged_http_operations_are_misspellings() {
        for (name, intended) in [
            ("http.dlete", "http.delete"),
            ("http.pots", "http.post"),
            ("http.gte", "http.get"),
            ("http.dowload", "http.download"),
        ] {
            assert_eq!(misspelled_http_operation(name), Some(intended), "{name}");
        }
        for name in [
            "http.trace",
            "http.propfind",
            "http.connect",
            "http.purge",
            "http.lock",
        ] {
            assert_eq!(misspelled_http_operation(name), None, "{name}");
        }
    }

    #[test]
    fn edit_distance_counts_insertions_deletions_and_substitutions() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("http.get", "http.get"), 0);
    }
}
