//! Prints our errors on a TypeScript conformance case, for
//! `typescript-baselines/prune-case.cjs`. One line each:
//! `<line>\t<column>\t<file>\t<type|unsupported|unclear>\t<message>`, with the column in UTF-16
//! code units, as `tsc` counts them, and the file the index of the case's file the error is in
//! (0 for a single file).
//!
//! Usage: cargo run --release -p conformance --example typescript_case_errors -- <case.ts>

#[path = "../tests/support/case_errors.rs"]
mod case_errors;
#[path = "../tests/support/case_files.rs"]
mod case_files;

use case_errors::Support;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: typescript_case_errors <case.ts>");
    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let (_, errors) = case_errors::case_errors(&source);
    for e in errors {
        let kind = match e.support {
            Support::Supported => "type",
            Support::Lacking => "unsupported",
            Support::Unclear => "unclear",
        };
        let message = e.message.replace(['\n', '\t'], " ");
        println!("{}\t{}\t{}\t{kind}\t{message}", e.line, e.column, e.file);
    }
}
