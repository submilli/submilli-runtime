//! test262 conformance runner. Every case under `cases/` (crate root) is
//! compiled with `harness.ts` (the test262 assertion shim) prepended, then
//! run; pass = no trap, same as the interpreter's fixture harness.
//!
//! Header directives (first comment block of a case):
//! - `// expect-error: <substring>` — the case pins a *by-design* compile
//!   error (documented divergence); compilation must fail and every needle
//!   must match a diagnostic.
//! - `// expect-fail: <reason>` — known gap: the standard says this should
//!   work and it currently doesn't. The case must fail to compile or trap;
//!   if it passes, the runner errors so a fixed gap gets its directive
//!   removed.
//!
//! `rejected/` holds verbatim test262 cases excluded by a design decision
//! (kept for a future revisit). They are never compiled; each must carry a
//! `// rejected: <reason>` header.
//!
//! The full conformance body is opt-in for local runs. Set
//! `SUBMILLI_TEST_NIGHTLY_ONLY=1` in nightly/release checks. `CONFORMANCE_FILTER`
//! narrows an opted-in run; it does not enable conformance on its own.

#[path = "support/conformance_gate.rs"]
mod conformance_gate;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use interpreter::runtime::{
    StoreData, Vfs, install_runtime_host_functions, install_runtime_store_bound,
    install_tenant_limits,
};
use interpreter::{
    BacktraceMode, Diagnostic, RunResult, RuntimeConfig, Sources, compile::CompiledScript,
    compile_script, diagnostics, render_backtrace,
};
use wasmtime::{Engine, Linker, Module};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn conformance() {
    if !conformance_gate::requested() {
        eprintln!("conformance: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
        return;
    }
    let root = Path::new(ROOT);
    let shim = Arc::new(fs::read_to_string(root.join("harness.ts")).expect("read harness.ts"));
    let mut paths = Vec::new();
    collect(&root.join("cases"), &mut paths);
    paths.sort();
    let filter = std::env::var("CONFORMANCE_FILTER").ok();
    if let Some(filter) = filter {
        paths.retain(|p| p.to_string_lossy().contains(&filter));
    }
    assert!(!paths.is_empty(), "no cases discovered under {ROOT}/cases");

    let runtime = Arc::new(PreparedRuntime::new().expect("prepare runtime"));
    let mut outcomes = run_cases_parallel(paths.clone(), shim, runtime);
    outcomes.sort_by(|a, b| a.path.cmp(&b.path));

    let mut failures: Vec<String> = Vec::new();
    let mut known_gaps = 0usize;
    for outcome in outcomes {
        match outcome.result {
            Ok(CaseOutcome::Passed) => {}
            Ok(CaseOutcome::KnownGap) => known_gaps += 1,
            Err(msg) => failures.push(format!("--- {} ---\n{msg}", rel(&outcome.path))),
        }
    }

    eprintln!(
        "conformance: {} case(s), {} known gap(s) (expect-fail)",
        paths.len(),
        known_gaps,
    );
    if !failures.is_empty() {
        panic!(
            "\n{} conformance failure(s) of {}:\n\n{}",
            failures.len(),
            paths.len(),
            failures.join("\n\n"),
        );
    }
}

struct CaseResult {
    path: PathBuf,
    result: Result<CaseOutcome, String>,
}

#[derive(Clone)]
struct PreparedRuntime {
    config: RuntimeConfig,
    engine: Engine,
    base_linker: Linker<StoreData>,
}

impl PreparedRuntime {
    fn new() -> wasmtime::Result<Self> {
        let config = RuntimeConfig::default();
        let engine = config.engine_async()?;
        let mut base_linker = Linker::<StoreData>::new(&engine);
        install_runtime_host_functions(&mut base_linker)?;
        Ok(Self {
            config,
            engine,
            base_linker,
        })
    }

    async fn run(&self, compiled: &CompiledScript) -> wasmtime::Result<RunResult> {
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl Write for Sink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().write(buf)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let buf = Arc::new(Mutex::new(Vec::new()));
        let mut data = StoreData::with_vfs_and_cap(Vfs::tempdir()?, self.config.max_store_bytes);
        data.console = Box::new(Sink(Arc::clone(&buf)));
        data.install_type_info(compiled.type_info.clone());
        let mut store = self.config.store_async(&self.engine, data)?;
        install_tenant_limits(&mut store);
        let module = Module::new(&self.engine, &compiled.wasm)?;
        let mut linker = self.base_linker.clone();
        install_runtime_store_bound(&mut linker, &mut store)?;
        let inst = linker.instantiate_async(&mut store, &module).await?;
        let _watchdog = self.config.arm_timeout(&self.engine)?;
        let value = interpreter::dispatch_main_async(&mut store, &inst).await?;
        let captured = buf.lock().unwrap().clone();
        let console = String::from_utf8(captured)
            .map_err(|e| wasmtime::Error::msg(format!("console output not utf-8: {e}")))?;
        Ok(RunResult { value, console })
    }
}

fn run_cases_parallel(
    paths: Vec<PathBuf>,
    shim: Arc<String>,
    runtime: Arc<PreparedRuntime>,
) -> Vec<CaseResult> {
    let worker_count = worker_count(paths.len());
    let mut chunks = vec![Vec::new(); worker_count];
    for (i, path) in paths.into_iter().enumerate() {
        chunks[i % worker_count].push(path);
    }

    let mut handles = Vec::new();
    for chunk in chunks {
        let shim = Arc::clone(&shim);
        let runtime = Arc::clone(&runtime);
        handles.push(
            std::thread::Builder::new()
                .stack_size(interpreter::compiler_limits::COMPILER_STACK_BYTES)
                .spawn(move || {
                    let tokio = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("tokio runtime");
                    chunk
                        .into_iter()
                        .map(|path| {
                            let result = run_case(&path, &shim, &runtime, &tokio);
                            CaseResult { path, result }
                        })
                        .collect::<Vec<_>>()
                })
                .expect("spawn fixture worker"),
        );
    }

    handles
        .into_iter()
        .flat_map(|handle| handle.join().expect("conformance worker panicked"))
        .collect()
}

fn worker_count(case_count: usize) -> usize {
    let default = std::thread::available_parallelism().map_or(1, usize::from);
    let requested = match std::env::var("SUBMILLI_TEST_PARALLELISM") {
        Ok(value) if is_false(&value) => 1,
        Ok(value) if is_true(&value) => default,
        Ok(value) => value
            .parse::<usize>()
            .ok()
            .filter(|value| *value > 0)
            .unwrap_or(default),
        Err(_) => default,
    };
    requested.min(case_count.max(1))
}

fn is_true(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn is_false(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
}

#[test]
fn rejected_cases_carry_reasons() {
    let rejected = Path::new(ROOT).join("rejected");
    if !rejected.exists() {
        return;
    }
    let mut paths = Vec::new();
    collect_any(&rejected, &mut paths);
    paths.sort();

    let mut missing: Vec<String> = Vec::new();
    for p in &paths {
        let src = fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
        let has_reason = src.lines().take(10).any(|l| {
            l.trim_start()
                .strip_prefix("// rejected:")
                .is_some_and(|r| !r.trim().is_empty())
        });
        if !has_reason {
            missing.push(rel(p));
        }
    }
    assert!(
        missing.is_empty(),
        "rejected case(s) without a `// rejected: <reason>` header:\n{}",
        missing.join("\n"),
    );
}

enum CaseOutcome {
    Passed,
    KnownGap,
}

fn run_case(
    path: &Path,
    shim: &str,
    runtime: &PreparedRuntime,
    tokio: &tokio::runtime::Runtime,
) -> Result<CaseOutcome, String> {
    let case_src = fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
    let directives = parse_directives(&case_src);
    if !directives.expect_errors.is_empty() && directives.expect_fail.is_some() {
        return Err("a case can carry `expect-error` or `expect-fail`, not both".into());
    }

    let src = format!("{shim}\n{case_src}");
    let filename = rel(path);

    match compile_script(&src, &filename, interpreter::FileId(0), &[], &[]) {
        Err(diags) => {
            if directives.expect_fail.is_some() {
                return Ok(CaseOutcome::KnownGap);
            }
            if directives.expect_errors.is_empty() {
                return Err(format!(
                    "compile failed:\n{}",
                    render_diags(&diags, &filename, &src),
                ));
            }
            assert_diagnostics(&diags, &directives.expect_errors, &filename, &src)?;
            Ok(CaseOutcome::Passed)
        }
        Ok(_) if !directives.expect_errors.is_empty() => Err(format!(
            "expected compile error(s) {:?}, but compilation succeeded",
            directives.expect_errors,
        )),
        Ok(compiled) => {
            let outcome = tokio.block_on(runtime.run(&compiled));
            match (outcome, directives.expect_fail) {
                (Ok(_), None) => Ok(CaseOutcome::Passed),
                (Ok(_), Some(reason)) => Err(format!(
                    "marked `expect-fail: {reason}` but the case passed — \
                     the gap is closed; remove the directive",
                )),
                (Err(_), Some(_)) => Ok(CaseOutcome::KnownGap),
                (Err(err), None) => {
                    let (sources, file) = Sources::single(filename.as_str(), &src).unwrap();
                    // The rendered backtrace already carries the `error: …` header.
                    Err(
                        match render_backtrace(&err, &sources, file, BacktraceMode::Full) {
                            Some(bt) => format!("trapped:\n{bt}"),
                            None => format!("trapped: {err}"),
                        },
                    )
                }
            }
        }
    }
}

#[derive(Default)]
struct Directives {
    expect_errors: Vec<String>,
    expect_fail: Option<String>,
}

fn parse_directives(src: &str) -> Directives {
    let mut directives = Directives::default();
    for line in src.lines().take_while(|l| {
        let t = l.trim_start();
        t.is_empty() || t.starts_with("//")
    }) {
        if let Some((_, s)) = line.split_once("// expect-error:") {
            let needle = s.trim();
            if !needle.is_empty() {
                directives.expect_errors.push(needle.to_string());
            }
        }
        if let Some((_, s)) = line.split_once("// expect-fail:") {
            let reason = s.trim();
            if !reason.is_empty() {
                directives.expect_fail = Some(reason.to_string());
            }
        }
    }
    directives
}

fn assert_diagnostics(
    diags: &[Diagnostic],
    needles: &[String],
    filename: &str,
    src: &str,
) -> Result<(), String> {
    for needle in needles {
        let matched = diags.iter().any(|d| {
            d.message.contains(needle)
                || d.help.iter().any(|h| h.contains(needle))
                || d.notes.iter().any(|(_, text)| text.contains(needle))
        });
        if !matched {
            return Err(format!(
                "expected diagnostic substring {needle:?} not found.\nactual:\n{}",
                render_diags(diags, filename, src),
            ));
        }
    }
    Ok(())
}

fn render_diags(diags: &[Diagnostic], filename: &str, src: &str) -> String {
    let (sources, _) = Sources::single(filename, src).unwrap();
    diags
        .iter()
        .map(|d| diagnostics::render(d, &sources))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir({}): {e}", dir.display()));
    for entry in entries {
        let p = entry.expect("dir entry").path();
        if p.is_dir() {
            collect(&p, out);
        } else if matches!(p.extension().and_then(|s| s.to_str()), Some("ts" | "subm")) {
            out.push(p);
        }
    }
}

fn collect_any(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir({}): {e}", dir.display()));
    for entry in entries {
        let p = entry.expect("dir entry").path();
        if p.is_dir() {
            collect_any(&p, out);
        } else if matches!(
            p.extension().and_then(|s| s.to_str()),
            Some("ts" | "subm" | "js")
        ) {
            out.push(p);
        }
    }
}

fn rel(p: &Path) -> String {
    p.strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .unwrap_or(p)
        .display()
        .to_string()
}
