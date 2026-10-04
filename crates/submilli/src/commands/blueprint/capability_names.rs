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

/// Nearby names, ignoring case; the module prefix breaks equal-distance ties.
fn suggestions(known: &[String], input: &str) -> Vec<String> {
    let input = input.to_lowercase();
    if input.is_empty() {
        return Vec::new();
    }
    let prefix = format!("{}.", input.split('.').next().unwrap_or(&input));
    let mut ranked: Vec<_> = known
        .iter()
        .filter_map(|name| {
            let normalized = name.to_lowercase();
            let distance = nearby_edit_distance(&input, &normalized)?;
            Some((distance, !normalized.starts_with(&prefix), name))
        })
        .collect();
    ranked.sort_unstable();
    ranked.dedup();
    ranked
        .into_iter()
        .take(5)
        .map(|(_, _, name)| name.clone())
        .collect()
}

/// Distances above three are not useful spelling hints. Saturating cells at
/// four also keeps arithmetic bounded regardless of capability-name length.
fn nearby_edit_distance(input: &str, candidate: &str) -> Option<usize> {
    const LIMIT: usize = 3;
    let input_len = input.chars().count();
    let candidate_len = candidate.chars().count();
    if input_len.abs_diff(candidate_len) > LIMIT {
        return None;
    }
    let mut row: Vec<_> = (0..=candidate_len).map(|n| n.min(LIMIT + 1)).collect();
    for (position, input_char) in input.chars().enumerate() {
        let mut cells = row.iter_mut();
        let first = cells.next()?;
        let mut diagonal = *first;
        *first = position.saturating_add(1).min(LIMIT + 1);
        let mut left = *first;
        let mut minimum = left;
        for (cell, candidate_char) in cells.zip(candidate.chars()) {
            let above = *cell;
            *cell = (diagonal + usize::from(input_char != candidate_char))
                .min(above + 1)
                .min(left + 1)
                .min(LIMIT + 1);
            diagonal = above;
            left = *cell;
            minimum = minimum.min(left);
        }
        if minimum > LIMIT {
            return None;
        }
    }
    row.last().copied().filter(|distance| *distance <= LIMIT)
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
        .filter_map(|capability| {
            nearby_edit_distance(name, capability.name).map(|distance| (distance, capability.name))
        })
        .filter(|(distance, _)| *distance <= 2)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, intended)| intended)
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
    fn suggestions_rank_catalog_typos_first() {
        let blueprint = submilli_blueprint::parse("name: t\n").unwrap();
        let packages =
            declared_packages::load(&blueprint, &submilli_build::PackageStore::default());
        let known = known_capabilities(&blueprint, &packages, "main");
        for (input, expected) in [
            ("fs.cpy", "fs.copy"),
            ("http.downlod", "http.download"),
            ("FS.Write", "fs.write"),
            ("htp.download", "http.download"),
            ("fs.raed", "fs.read"),
        ] {
            let hints = suggestions(&known, input);
            assert_eq!(hints.first().map(String::as_str), Some(expected), "{input}");
            assert!(hints.len() <= 5);
        }
    }

    #[test]
    fn suggestions_bound_distance_and_break_ties() {
        let known = ["gs.cat", "fs.cbt", "fs.car", "fs.car", "fs.zzzzzzzz"].map(String::from);
        assert_eq!(
            suggestions(&known, "fs.cat"),
            ["fs.car", "fs.cbt", "gs.cat"]
        );
        assert!(suggestions(&known, "").is_empty());
        assert!(suggestions(&known, "unrelated.name").is_empty());
        assert!(suggestions(&[], "fs.cat").is_empty());
        let known = ["fs.ca", "fs.cb", "fs.cc", "fs.cd", "fs.ce", "fs.cf"].map(String::from);
        assert_eq!(suggestions(&known, "fs.cx"), known[..5]);
    }

    #[test]
    fn nearby_distance_handles_threshold_and_unicode() {
        for (input, candidate, expected) in [
            ("", "", Some(0)),
            ("", "abc", Some(3)),
            ("abc", "", Some(3)),
            ("abcd", "", None),
            ("abc", "xyz", Some(3)),
            ("abcd", "wxyz", None),
            ("kitten", "sitting", Some(3)),
            ("mcp.猫", "mcp.犬", Some(1)),
        ] {
            assert_eq!(nearby_edit_distance(input, candidate), expected);
        }
    }
}
