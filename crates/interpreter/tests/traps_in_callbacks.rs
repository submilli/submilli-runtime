//! A trap ends the run wherever the program reaches it. Under a host function
//! that re-entered guest code it behaves as it does without the callback: a
//! surrounding `try`/`catch` must not see it.

use std::time::Duration;

use interpreter::{compile_script, runtime::RuntimeConfig};
use wasmtime::{Trap, WasmBacktrace};

/// Every program below returns normally from its handler, so a run that reaches
/// one is an `Ok`.
fn run(raise: &str, body: &str, cfg: RuntimeConfig) -> wasmtime::Result<()> {
    let source =
        format!("{PRELUDE}\nfunction main(): string {{\n{body}\n}}\n").replace("RAISE", raise);
    let compiled = compile_script(&source, "test.ts", interpreter::FileId(0), &[], &[])
        .map_err(|diagnostics| wasmtime::Error::msg(format!("compile failed: {diagnostics:#?}")))?;
    // Re-entry nests interpreter frames on the native stack, so the thread is
    // sized for the config as the CLI and the server size theirs.
    let thread = std::thread::Builder::new()
        .stack_size(cfg.native_stack_size())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(wasmtime::Error::new)?;
            runtime.block_on(cfg.run_compiled(&compiled)).map(|_| ())
        })
        .map_err(wasmtime::Error::new)?;
    thread
        .join()
        .map_err(|_| wasmtime::Error::msg("the run panicked"))?
}

const PRELUDE: &str = r#"
function recurse(depth: number): number { return recurse(depth + 1) + 1; }
function spin(): number { let n = 0; while (true) { n = n + 1; } return n; }
class ViaGetter {
  // The `if` keeps the `return` reachable when RAISE is a `throw`.
  get value(): number { if (true) { RAISE; } return 0; }
}
"#;

/// The ways a program hands a closure or getter to a host function, each
/// wrapped in a `try`/`catch` whose handler returns normally. `RAISE` stands
/// for the call that reaches the limit, here and in the prelude.
const CALLBACK_SITES: &[(&str, &str)] = &[
    (
        "direct call",
        r#"try { RAISE; } catch (e) { return "caught"; } return "done";"#,
    ),
    (
        "Array.forEach",
        r#"try { [1].forEach((x: number) => { RAISE; }); } catch (e) { return "caught"; }
           return "done";"#,
    ),
    (
        "Array.map",
        r#"try { [1].map((x: number): number => RAISE); } catch (e) { return "caught"; }
           return "done";"#,
    ),
    (
        "Array.filter",
        r#"try { [1].filter((x: number): boolean => RAISE > 0); } catch (e) { return "caught"; }
           return "done";"#,
    ),
    (
        "Array.sort",
        r#"try { [2, 1].sort((a: number, b: number): number => RAISE); }
           catch (e) { return "caught"; }
           return "done";"#,
    ),
    (
        "Map.forEach",
        r#"const entries = new Map<string, number>();
           entries.set("a", 1);
           try { entries.forEach((value: number, key: string) => { RAISE; }); }
           catch (e) { return "caught"; }
           return "done";"#,
    ),
    (
        "Set.forEach",
        r#"const members = new Set<number>();
           members.add(1);
           try { members.forEach((value: number) => { RAISE; }); }
           catch (e) { return "caught"; }
           return "done";"#,
    ),
    (
        "a callback under a callback",
        r#"try { [1].forEach((x: number) => { [1].map((y: number): number => RAISE); }); }
           catch (e) { return "caught"; }
           return "done";"#,
    ),
    (
        "a getter under JSON.stringify",
        r#"try { return JSON.stringify(new ViaGetter()); } catch (e) { return "caught"; }"#,
    ),
    (
        "a finally around the callback",
        r#"try { [1].forEach((x: number) => { RAISE; }); } finally { return "cleaned up"; }"#,
    ),
    (
        "a handler inside the callback",
        r#"[1].forEach((x: number) => { try { RAISE; } catch (e) { } });
           return "done";"#,
    ),
];

fn assert_trap_ends_the_run(raise: &str, cfg: &RuntimeConfig, expected: Trap) {
    for (name, body) in CALLBACK_SITES {
        let err = run(raise, body, cfg.clone()).expect_err(&format!(
            "{name}: the run should end, not continue in the handler"
        ));
        assert_eq!(
            err.downcast_ref::<Trap>(),
            Some(&expected),
            "{name}: expected {expected:?}, got: {err:#}"
        );
        if expected == Trap::OutOfFuel {
            // A small budget must reach the callback, rather than expire in setup.
            let backtrace = err.downcast_ref::<WasmBacktrace>().expect("trap backtrace");
            assert!(
                backtrace.frames().iter().any(|frame| {
                    frame
                        .symbols()
                        .iter()
                        .any(|symbol| symbol.name() == Some("spin"))
                }),
                "{name}: fuel must run out in spin, got: {err:#}"
            );
        }
    }
}

#[test]
fn stack_exhaustion_under_a_callback_is_not_catchable() {
    assert_trap_ends_the_run("recurse(0)", &RuntimeConfig::default(), Trap::StackOverflow);
}

#[test]
fn fuel_exhaustion_under_a_callback_is_not_catchable() {
    let cfg = RuntimeConfig {
        fuel: 20_000,
        ..RuntimeConfig::default()
    };
    assert_trap_ends_the_run("spin()", &cfg, Trap::OutOfFuel);
}

#[test]
fn a_timeout_under_a_callback_is_not_catchable() {
    // Fuel is unbounded so the deadline, not fuel, is what the loop reaches.
    let cfg = RuntimeConfig {
        fuel: u64::MAX,
        timeout: Some(Duration::from_millis(50)),
        ..RuntimeConfig::default()
    };
    assert_trap_ends_the_run("spin()", &cfg, Trap::Interrupt);
}

/// Top-level statements run while the module is instantiated, which the
/// one-shot runner has to bound as it bounds `main`.
#[test]
fn a_timeout_in_top_level_statements_ends_the_run() {
    let source = "let spins = 0;\nwhile (true) { spins = spins + 1; }\nfunction main(): void { }\n";
    let compiled = compile_script(source, "test.ts", interpreter::FileId(0), &[], &[])
        .unwrap_or_else(|diagnostics| panic!("compile failed: {diagnostics:#?}"));
    let cfg = RuntimeConfig {
        fuel: u64::MAX,
        timeout: Some(Duration::from_millis(50)),
        ..RuntimeConfig::default()
    };
    let err = pollster::block_on(cfg.run_compiled(&compiled))
        .expect_err("the top-level loop should be interrupted");
    assert_eq!(
        err.downcast_ref::<Trap>(),
        Some(&Trap::Interrupt),
        "{err:#}"
    );
}

#[test]
fn ordinary_errors_under_a_callback_are_still_catchable() {
    let raised_in_the_callback = [
        r#"throw new Error("from the callback")"#,
        // A host function's own failure, as opposed to a trap it passes on.
        r#""x".repeat(-1)"#,
        r#"JSON.parse("{")"#,
    ];
    for raise in raised_in_the_callback {
        let body = r#"
            try { [1].forEach((x: number) => { RAISE; }); }
            catch (e) { return "caught"; }
            throw new Error("the handler did not run");"#;
        run(raise, body, RuntimeConfig::default())
            .unwrap_or_else(|err| panic!("`{raise}` should be caught, got: {err:#}"));
    }
}
