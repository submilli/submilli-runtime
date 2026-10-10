//! Compiling the same source twice must produce byte-identical Wasm.
//!
//! Repeated compiles in one process see different `HashMap` seeds — `RandomState`
//! bumps its key per instance — so a hash-order leak into codegen shows up here as
//! a byte diff. Detection is probabilistic per compile: a two-entry map still agrees
//! with itself half the time, which is what `COMPILES_PER_SOURCE` is sized against.
//!
//! The sweep is opt-in for nightly and release verification. Set
//! `SUBMILLI_TEST_NIGHTLY_ONLY=1` to run it; `SUBMILLI_FULL_TEST` does not enable it.

use std::fs;
use std::path::{Path, PathBuf};

use submilli_engine::{
    CompiledPackage, Diagnostic, FileId, ModulePath, PackageSourceModule, compile_package,
    compile_script,
};

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
const COMPILES_PER_SOURCE: usize = 16;
/// The fixture corpus is ~870 standalone-compilable files. A floor near that catches
/// a harness or collector regression that quietly stops compiling most of them.
const MIN_COMPILED_FIXTURES: usize = 800;

#[test]
fn scripts_compile_deterministically() {
    if !determinism_requested() {
        eprintln!("script determinism: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
        return;
    }
    let mut paths = Vec::new();
    collect_source_files(Path::new(FIXTURE_DIR), &mut paths);
    paths.sort();

    let report = check_in_parallel(paths);

    assert!(
        report.compiled >= MIN_COMPILED_FIXTURES,
        "only {} fixture(s) compiled — the corpus or the harness is broken",
        report.compiled,
    );
    assert!(
        report.unstable.is_empty(),
        "{} of {} fixture(s) compile nondeterministically:\n{}",
        report.unstable.len(),
        report.compiled,
        report.unstable.join("\n"),
    );
}

/// The package front end (`compile_package`, cross-module declarations) is a separate
/// path from `compile_script`, and no fixture package narrows enough to catch an order
/// leak — hence a purpose-built one here. Every part is deliberate: `narrowSiblings`
/// and `narrowFields` name their bindings and fields so source order disagrees with
/// sorted order, and the `leaf` module exists so a second module's declarations are in
/// play while `lib` compiles.
#[test]
fn packages_compile_deterministically() {
    if !determinism_requested() {
        eprintln!("package determinism: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
        return;
    }
    let first = compile_narrowing_package().expect("package fixture compiles");
    for round in 1..COMPILES_PER_SOURCE {
        let again = compile_narrowing_package().unwrap_or_else(|d| {
            panic!("package compiled once, then failed on round {round}: {d:?}")
        });
        assert!(
            again.wasm == first.wasm,
            "package Wasm differs between compiles (round {round})",
        );
        assert!(
            again.declaration == first.declaration,
            "package declaration differs between compiles (round {round})",
        );
    }
}

#[test]
fn determinism_requires_explicit_opt_in() {
    for value in ["", "0", "false", "no", "off", "typo", "2"] {
        assert!(!determinism_enabled(value), "unexpected opt-in: {value}");
    }
}

#[test]
fn determinism_accepts_documented_true_values() {
    for value in ["1", "true", "yes", "on", "TRUE", "On"] {
        assert!(determinism_enabled(value), "missing opt-in: {value}");
    }
}

fn determinism_requested() -> bool {
    std::env::var("SUBMILLI_TEST_NIGHTLY_ONLY").is_ok_and(|value| determinism_enabled(&value))
}

fn determinism_enabled(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn compile_narrowing_package() -> Result<CompiledPackage, Vec<Diagnostic>> {
    const LEAF: &str = r#"
export function pick(zed: string | null, apple: string | null): string {
  if (apple !== null && zed !== null) {
    return apple + zed + apple.length.toString();
  }
  return "?";
}
"#;
    const LIB: &str = r#"
import { pick } from "./leaf";

export interface Pair { zed: string | null; alpha: string | null; }

export function narrowSiblings(z: string | null, m: string | null, a: string | null): string {
  if (z !== null && m !== null && a !== null) {
    return z + m + a + pick(z, a);
  }
  return "-";
}

export function narrowFields(p: Pair | null): string {
  if (p !== null && p.zed !== null && p.alpha !== null) {
    return p.alpha + p.zed;
  }
  return "-";
}
"#;

    compile_package(
        "determinism",
        ModulePath::from("lib"),
        &[
            PackageSourceModule {
                path: ModulePath::from("leaf"),
                source: LEAF,
            },
            PackageSourceModule {
                path: ModulePath::from("lib"),
                source: LIB,
            },
        ],
        &[],
    )
}

struct Report {
    compiled: usize,
    unstable: Vec<String>,
}

fn check_in_parallel(paths: Vec<PathBuf>) -> Report {
    let worker_count = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(paths.len().max(1));
    let mut chunks = vec![Vec::new(); worker_count];
    for (i, path) in paths.into_iter().enumerate() {
        chunks[i % worker_count].push(path);
    }

    let handles: Vec<_> = chunks
        .into_iter()
        .map(|chunk| {
            std::thread::Builder::new()
                .stack_size(submilli_engine::compiler_limits::COMPILER_STACK_BYTES)
                .spawn(move || find_unstable(&chunk))
                .expect("spawn compile worker")
        })
        .collect();

    let mut report = Report {
        compiled: 0,
        unstable: Vec::new(),
    };
    for handle in handles {
        let chunk = handle.join().expect("determinism worker panicked");
        report.compiled += chunk.compiled;
        report.unstable.extend(chunk.unstable);
    }
    report.unstable.sort();
    report
}

fn find_unstable(paths: &[PathBuf]) -> Report {
    let mut report = Report {
        compiled: 0,
        unstable: Vec::new(),
    };
    for path in paths {
        let Ok(source) = fs::read_to_string(path) else {
            continue;
        };
        let name = fixture_name(path);
        // Multi-file and `expect-error` fixtures don't compile standalone; they fail
        // the same way every time and are simply not covered here.
        let Ok(first) = compile_script(&source, &name, FileId(0), &[], &[]) else {
            continue;
        };
        report.compiled += 1;
        // A later compile that *errors* is the loudest form of the flakiness this
        // test exists to catch, so it counts as unstable rather than being skipped.
        let stable = (1..COMPILES_PER_SOURCE).all(|_| {
            compile_script(&source, &name, FileId(0), &[], &[])
                .is_ok_and(|again| again.wasm == first.wasm)
        });
        if !stable {
            report.unstable.push(name);
        }
    }
    report
}

fn fixture_name(path: &Path) -> String {
    path.strip_prefix(FIXTURE_DIR)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_source_files(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("ts" | "subm")
        ) {
            out.push(path);
        }
    }
}
