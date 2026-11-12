//! `localeCompare` charges fuel and admits native memory for collation before
//! each step. Long strings are compared by sort keys, so a long shared prefix
//! costs one pass; runs of combining marks cost the square of their length;
//! and a key, or the level buffers ICU4X fills while writing a tertiary key,
//! that would not fit the memory cap is refused before ICU4X writes it.

use submilli_engine::{compile_script, runtime::RuntimeConfig};

fn run(body: &str, cfg: RuntimeConfig) -> wasmtime::Result<()> {
    let source = format!("function main(): void {{\n{body}\n}}\n");
    let compiled = compile_script(&source, "test.ts", submilli_engine::FileId(0), &[], &[])
        .unwrap_or_else(|diagnostics| panic!("compile failed: {diagnostics:#?}"));
    pollster::block_on(cfg.run_compiled(&compiled)).map(|_| ())
}

fn with_fuel(fuel: u64) -> RuntimeConfig {
    RuntimeConfig {
        fuel,
        ..RuntimeConfig::default()
    }
}

#[test]
fn a_long_shared_prefix_costs_one_pass() {
    // U+F900 makes ICU4X's direct comparison step back over the whole prefix.
    run(
        r#"const run: string = "豈".repeat(20000);
console.log(String((run + "a").localeCompare(run + "b")));"#,
        with_fuel(100_000_000),
    )
    .expect("sort keys read the prefix once");
}

#[test]
fn a_run_of_combining_marks_is_charged_its_square_before_the_work() {
    // U+0F73 decomposes to two marks: a run of 40,000, about 1.6e9 units a
    // pass, past the 1e8 budget.
    let err = run(
        r#"const run: string = "ཱི".repeat(20000);
console.log(String(("a" + run).localeCompare("b" + run)));"#,
        with_fuel(100_000_000),
    )
    .expect_err("the quadratic run is charged");
    assert_eq!(
        err.downcast_ref::<wasmtime::Trap>(),
        Some(&wasmtime::Trap::OutOfFuel),
        "{err:#}"
    );
}

#[test]
fn a_sort_key_past_the_memory_cap_is_refused_before_it_is_written() {
    let cfg = RuntimeConfig {
        max_store_bytes: 4 * 1024 * 1024,
        ..RuntimeConfig::default()
    };
    // U+FDFA has 18 primary weights, about 24 key bytes: 200,000 of them need
    // about 4.8 MB a key.
    let err = run(
        r#"const run: string = "ﷺ".repeat(200000);
console.log(String(("a" + run).localeCompare("b" + run)));"#,
        cfg,
    )
    .expect_err("the key is admitted first");
    assert!(
        submilli_engine::runtime::limits::is_memory_exhausted(&err),
        "{err:#}"
    );
}

#[test]
fn long_text_that_differs_early_fits_the_default_memory_cap() {
    run(
        r#"const text: string = "한국어".repeat(100000);
console.log(String(("가" + text).localeCompare("나" + text)));"#,
        RuntimeConfig::default(),
    )
    .expect("two 300,000-character keys of about 0.8 MB each fit");
}

#[test]
fn level_buffers_past_the_memory_cap_are_refused_before_they_are_filled() {
    let cfg = RuntimeConfig {
        max_store_bytes: 6 * 1024 * 1024,
        ..RuntimeConfig::default()
    };
    // U+FC5E has no primary weight, so the primary keys are empty and agree.
    // It decomposes to three characters: 750,000 a string, so 6 MB of level
    // buffers (8 bytes a character) on top of the 0.6 MB string, past the
    // 6 MiB cap before ICU4X writes the first tertiary key.
    let err = run(
        r#"const run: string = "\uFC5E".repeat(250000);
console.log(String(run.localeCompare(run)));"#,
        cfg,
    )
    .expect_err("the level buffers are admitted first");
    assert!(
        submilli_engine::runtime::limits::is_memory_exhausted(&err),
        "{err:#}"
    );
}
