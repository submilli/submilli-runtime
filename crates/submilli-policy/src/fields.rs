//! The fields a filter tests that a capability's check doesn't report. A
//! condition on a field the check doesn't report is false for every call, and
//! true under `not`, whatever the call's arguments.

use std::collections::BTreeSet;

use crate::filter::FilterExpr;

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
