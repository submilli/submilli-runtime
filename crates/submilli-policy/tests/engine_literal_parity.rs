//! The engine writes filters into a package's derived requirements without
//! depending on this crate, so its quoting is a second copy of
//! [`quote_literal`]. These tests keep the two in step, and check that every
//! field name the engine's capability catalog offers can be written in a
//! filter.

use interpreter::capability_derivation::filter_string_literal;
use interpreter::stdlib::capabilities;
use interpreter::{OptionalPackage, Stdlib};
use submilli_policy::filter_syntax::{is_field_name, parse, quote_literal};

const CORPUS: &[&str] = &[
    "",
    "plain",
    "with \"quotes\"",
    "back\\slash",
    "line\nbreak",
    "tab\tand\rreturn",
    "${vars.x}",
    "prefix ${vars.tenant} suffix",
    "${other}",
    "$",
    "{",
    "}",
    "${",
    "${vars",
    "naïve — ünïcödé ✓",
    "/repos/acme/*",
];

#[test]
fn the_engine_quotes_every_value_as_the_policy_does() {
    for value in CORPUS {
        assert_eq!(
            filter_string_literal(value),
            quote_literal(value),
            "{value:?}"
        );
    }
}

#[test]
fn a_quoted_value_reads_back_as_itself() {
    for value in CORPUS {
        let Some(literal) = filter_string_literal(value) else {
            continue;
        };
        let filter = parse(&format!("f == {literal}")).expect("parses");
        assert!(
            filter.matches(&serde_json::json!({ "f": value })),
            "{value:?} as {literal}"
        );
    }
}

#[test]
fn every_cataloged_field_can_be_written_in_a_filter() {
    let every = Stdlib::core()
        .with(OptionalPackage::Agents)
        .with(OptionalPackage::Skills);
    for group in capabilities::catalog_for(every) {
        for capability in group.capabilities {
            for field in capability.field_names() {
                assert!(is_field_name(field), "{}: {field}", capability.name);
            }
        }
    }
}
