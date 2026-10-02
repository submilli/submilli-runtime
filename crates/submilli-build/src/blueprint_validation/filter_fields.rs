//! The context fields a capability's check reports, and the fields a filter
//! tests that it doesn't. A condition on a field the check doesn't report is
//! false for every call, and true under `not`, whatever the call's arguments.

use std::collections::{BTreeMap, BTreeSet};

use crate::Artifact;
use interpreter::stdlib::capabilities::{self, Capability};
use submilli_blueprint::{Blueprint, FilterExpr};

/// The context fields `capability`'s check reports: from the catalog for the
/// standard library, a declared MCP server, or an HTTP method only
/// `http.request` takes; and from `capabilities.yaml` for each loaded package
/// that provides it. A package may check a standard library name itself, with
/// its own context, so the two sources combine.
/// `None` when nothing known provides `capability`.
pub fn reported_fields<'a>(
    blueprint: &Blueprint,
    artifacts: &'a BTreeMap<String, Artifact>,
    capability: &str,
) -> Option<BTreeSet<&'a str>> {
    let is_mcp_server = capability
        .strip_prefix("mcp.")
        .is_some_and(|server| blueprint.mcp.contains_key(server));
    let catalog_name = if is_mcp_server {
        "mcp.<server>"
    } else {
        capability
    };
    let cataloged = capabilities::find_gating(catalog_name);
    let providers: Vec<_> = artifacts
        .values()
        .flat_map(|artifact| &artifact.capabilities.provides)
        .filter(|entry| entry.name == capability)
        .collect();
    if cataloged.is_none() && providers.is_empty() {
        return None;
    }
    let mut fields: BTreeSet<&str> = cataloged
        .into_iter()
        .flat_map(Capability::field_names)
        .collect();
    fields.extend(
        providers
            .into_iter()
            .flat_map(|entry| entry.fields.keys().map(String::as_str)),
    );
    Some(fields)
}

/// The fields `filter` tests that are not in `reported`, once each, in source
/// order. Only the first segment of a field path is checked: the catalog and
/// `capabilities.yaml` don't describe what an object field contains.
pub fn unreported_fields<'f>(filter: &'f FilterExpr, reported: &BTreeSet<&str>) -> Vec<&'f str> {
    let mut unreported = Vec::new();
    for field in filter.top_level_fields() {
        if !reported.contains(field) && !unreported.contains(&field) {
            unreported.push(field);
        }
    }
    unreported
}

/// What is wrong with a filter testing `field`, which is not in `reported`,
/// worded to follow the rule it belongs to.
pub fn unreported_field_problem(field: &str, reported: &BTreeSet<&str>) -> String {
    format!(
        "tests `{field}`, which the operation doesn't report, so a condition on it is false \
         for every call, and true under `not`; {}",
        describe_fields(reported)
    )
}

/// A clause listing `fields`, sorted, or saying there are none.
fn describe_fields(fields: &BTreeSet<&str>) -> String {
    if fields.is_empty() {
        return "it reports no fields".to_string();
    }
    let names: Vec<&str> = fields.iter().copied().collect();
    format!("its fields are: {}", names.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreported_fields_are_listed_once_in_source_order() {
        let filter: FilterExpr = "b == 1 and not (a.c == 2 or b < 3 or kept == 4)"
            .parse()
            .unwrap();
        let reported = BTreeSet::from(["kept"]);
        assert_eq!(unreported_fields(&filter, &reported), ["b", "a"]);
    }

    #[test]
    fn describe_fields_lists_sorted_names_or_says_none() {
        assert_eq!(describe_fields(&BTreeSet::new()), "it reports no fields");
        assert_eq!(
            describe_fields(&BTreeSet::from(["b", "a"])),
            "its fields are: a, b"
        );
    }
}
