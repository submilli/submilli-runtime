//! End-to-end tests for the offline discovery trio: `submilli docs`,
//! `submilli builtins`, and `submilli search`.
//!
//! These three had no coverage, so the parity claim they exist to protect --
//! that the CLI reaches the same resolution decisions as MCP and REST -- was
//! asserted rather than checked.

use std::path::PathBuf;
use std::process::{Command, Output};

fn submilli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_submilli"))
}

fn run(args: &[&str]) -> Output {
    Command::new(submilli_bin())
        .args(args)
        .output()
        .expect("invoke submilli")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn characterize_docs_on_a_stdlib_package() {
    let out = run(&["docs", "submilli:http"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("submilli:http"), "got: {text}");
    assert!(text.contains("function get("), "got: {text}");
}

#[test]
fn docs_on_an_unknown_name_lists_the_catalog_and_points_at_builtins() {
    let out = run(&["docs", "definitelynotapackage"]);
    assert!(!out.status.success());
    let text = stderr(&out);
    assert!(text.contains("unknown package"), "got: {text}");
    // The CLI already listed the catalog on a miss -- the behavior the server
    // lacked, and the reason this slice is about parity rather than novelty.
    assert!(text.contains("submilli:http"), "got: {text}");
    // New: the same builtins pointer every other surface emits.
    assert!(text.contains("submilli builtins"), "got: {text}");
}

#[test]
fn docs_serves_a_builtin_and_says_it_needs_no_import() {
    let out = run(&["docs", "Temporal"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("namespace Temporal {"), "got: {text}");
    // The JSON surfaces carry `source: "builtin"`; the text surface has to say
    // the same thing in words.
    assert!(text.contains("import"), "got: {text}");
    assert!(text.contains("built-in"), "got: {text}");
}

#[test]
fn docs_suggests_the_closest_name() {
    let out = run(&["docs", "Temporel"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("Temporal"), "got: {}", stderr(&out));

    // The same name produces the same candidate the server produces: both call
    // one resolver, so there is nothing to drift.
    let out = run(&["docs", "http"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("submilli:http"),
        "got: {}",
        stderr(&out)
    );
}

#[test]
fn characterize_builtins_listing_and_one_name() {
    let listing = run(&["builtins"]);
    assert!(listing.status.success());
    let text = stdout(&listing);
    assert!(text.contains("Types:"), "got: {text}");
    assert!(text.contains("Temporal"), "got: {text}");

    let one = run(&["builtins", "Array"]);
    assert!(one.status.success());
    assert!(
        stdout(&one).contains("interface Array<"),
        "got: {}",
        stdout(&one)
    );
}

#[test]
fn builtins_on_an_unknown_name_still_reports_it() {
    let out = run(&["builtins", "Bogus"]);
    assert!(!out.status.success());
    let text = stderr(&out);
    assert!(text.contains("unknown built-in"), "got: {text}");
    assert!(text.contains("submilli builtins"), "got: {text}");
}

#[test]
fn builtins_resolves_a_dotted_member_path() {
    let out = run(&["builtins", "Temporal.Instant"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let slice = stdout(&out);
    assert!(
        slice.contains("interface InstantConstructor {"),
        "got: {slice}"
    );
    assert!(!slice.contains("interface ZonedDateTime {"), "got: {slice}");

    let full = run(&["builtins", "Temporal"]);
    assert!(
        slice.len() * 4 < stdout(&full).len(),
        "the slice is meant to be much smaller than the namespace"
    );
}

#[test]
fn builtins_on_a_bad_member_lists_the_members_that_exist() {
    let out = run(&["builtins", "Temporal.Foo"]);
    assert!(!out.status.success());
    let text = stderr(&out);
    assert!(text.contains("Instant"), "got: {text}");
    assert!(text.contains("Now"), "got: {text}");
}

#[test]
fn builtins_on_a_package_name_names_the_docs_command() {
    let out = run(&["builtins", "submilli:http"]);
    assert!(!out.status.success());
    let text = stderr(&out);
    // The correcting call, not the declarations: a package needs an import.
    assert!(text.contains("submilli docs submilli:http"), "got: {text}");
    assert!(text.contains("import"), "got: {text}");
    assert!(
        !stdout(&out).contains("function get("),
        "got: {}",
        stdout(&out)
    );
}

#[test]
fn a_batch_reports_each_name_on_its_own_terms() {
    let out = run(&["builtins", "Array", "submilli:http", "Bogus"]);
    assert!(!out.status.success());
    assert!(
        stdout(&out).contains("interface Array<"),
        "got: {}",
        stdout(&out)
    );
    let text = stderr(&out);
    assert!(text.contains("submilli docs submilli:http"), "got: {text}");
    assert!(text.contains("unknown built-in: Bogus"), "got: {text}");
}

#[test]
fn the_listing_hint_is_printed_once_however_many_names_miss() {
    let out = run(&["builtins", "Array", "Bogus", "Alsobogus"]);
    assert!(!out.status.success());
    let text = stderr(&out);
    assert_eq!(
        text.matches("Run `submilli builtins`").count(),
        1,
        "the catalog hint belongs to the run, not to each bad name: {text}"
    );
    assert!(text.contains("unknown built-in: Bogus"), "got: {text}");
    assert!(text.contains("unknown built-in: Alsobogus"), "got: {text}");
}

#[test]
fn characterize_search_hit() {
    let out = run(&["search", "sha256"]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("submilli:crypto"),
        "got: {}",
        stdout(&out)
    );
}

#[test]
fn search_on_a_miss_lists_the_catalog_and_points_at_builtins() {
    let out = run(&["search", "nothingmatchesthis"]);
    assert!(out.status.success());
    let text = stderr(&out);
    assert!(text.contains("no packages match"), "got: {text}");
    // Previously the whole output. `docs` listed the catalog on a miss and
    // `search` did not -- the CLI's inconsistency with itself.
    assert!(text.contains("submilli:http"), "got: {text}");
    assert!(text.contains("submilli builtins"), "got: {text}");
}

#[test]
fn no_discovery_command_carries_its_own_catalog_dump() {
    // The hand-rolled recovery is replaced, not left alongside the shared
    // path. One implementation, in `interpreter::packages`.
    let sources = ["docs.rs", "search.rs", "builtins.rs"];
    for file in sources {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/commands")
                .join(file),
        )
        .expect("read command source");
        assert!(
            !text.contains("packages::search(\"\")"),
            "{file} still dumps the catalog itself"
        );
    }
}
