//! Run in a child process so a native stack overflow is reported as a failure.
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use submilli_engine::runtime::RuntimeConfig;
use submilli_engine::{FileId, compile_script, compiler_limits::COMPILER_STACK_BYTES};
use wasmtime::Trap;

const CHILD: &str = "SUBMILLI_NATIVE_STACK_TEST_CHILD";
const SOURCE: &str = r#"
function depth(n: number): number {
    if (n === 0) { return 0; }
    return [n].map((x: number): number => depth(x - 1))[0] + 1;
}
function main(): number {
    try { return depth(200000); } catch (e) { return -1; }
}
"#;

#[test]
fn callback_recursion_traps_on_documented_stacks() {
    if std::env::var_os(CHILD).is_some() {
        exercise_runners();
        return;
    }
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "callback_recursion_traps_on_documented_stacks",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start stack test subprocess");
    let deadline = Instant::now() + Duration::from_secs(120);
    while child.try_wait().expect("poll child").is_none() {
        if Instant::now() >= deadline {
            child.kill().expect("stop timed out child");
            child.wait().expect("reap timed out child");
            panic!("stack test exceeded 120 seconds");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().expect("read child output");
    assert!(
        output.status.success(),
        "stack test exited {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn exercise_runners() {
    let compiled = std::thread::Builder::new()
        .stack_size(COMPILER_STACK_BYTES)
        .spawn(|| compile_script(SOURCE, "reentry.ts", FileId(0), &[], &[]).expect("compile"))
        .expect("compiler thread")
        .join()
        .expect("compiler did not panic");
    // Cover the minimum reservation, default budget, and a raised budget.
    for budget in [64 * 1024, 512 * 1024, 1024 * 1024] {
        let cfg = RuntimeConfig {
            max_wasm_stack: budget,
            ..RuntimeConfig::default()
        };
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(cfg.native_stack_size())
                .spawn_scoped(scope, || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("runtime");
                    for compiled_runner in [false, true] {
                        let result = runtime.block_on(async {
                            if compiled_runner {
                                cfg.run_compiled(&compiled).await
                            } else {
                                cfg.run(&compiled.wasm).await
                            }
                        });
                        let error = result.expect_err("recursion must terminate the run");
                        assert_eq!(
                            error.downcast_ref::<Trap>(),
                            Some(&Trap::StackOverflow),
                            "budget={budget}, run_compiled={compiled_runner}: {error:#}"
                        );
                    }
                })
                .expect("execution thread")
                .join()
                .expect("execution did not panic");
        });
    }
}
