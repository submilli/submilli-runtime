//! Keep native-stack regressions isolated from the test runner.
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn inheritance_alias_depth_is_bounded() {
    if std::env::var_os("SUBMILLI_RECORD_LIMIT_CHILD").is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(check_alias_limits)
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "inheritance_alias_depth_is_bounded",
            "--nocapture",
        ])
        .env("SUBMILLI_RECORD_LIMIT_CHILD", "1")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "alias-limit child failed: {status}");
            return;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("alias-limit child timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn check_alias_limits() {
    for depth in [8, 64, 10_000] {
        let mut source = String::from("interface AChild extends Alias0 {}\n");
        for index in 0..depth {
            source.push_str(&format!("type Alias{index} = Alias{};\n", index + 1));
        }
        source.push_str(&format!("type Alias{depth} = ZBase;\ninterface ZBase {{ [key: string]: number; }}\nfunction main(): void {{}}"));
        let result = submilli_engine::compile_script(
            &source,
            "record-limit.ts",
            submilli_engine::FileId(0),
            &[],
            &[],
        );
        if depth == 8 {
            result.expect("short alias chain compiles");
        } else {
            let diagnostics = result.expect_err("deep alias chain must be rejected");
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.message == "interface inheritance alias limit exceeded"),
                "{diagnostics:?}"
            );
        }
        submilli_engine::compile_script(
            "function main(): number { return 42; }",
            "healthy.ts",
            submilli_engine::FileId(0),
            &[],
            &[],
        )
        .expect("healthy follow-up compiles");
    }
}
