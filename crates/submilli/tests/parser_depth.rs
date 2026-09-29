use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn cli_parser_depth_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("nested.ts");
    std::fs::write(
        &source,
        format!(
            "function main(): number {{ return {}1{}; }}",
            "(".repeat(2048),
            ")".repeat(2048)
        ),
    )
    .unwrap();
    for command in ["check", "run"] {
        let output = bounded_cli(command, &source);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("parser recursion limit exceeded"),
            "{output:?}"
        );
    }
    std::fs::write(&source, "function main(): number { return 42; }").unwrap();
    let output = bounded_cli("run", &source);
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("42"),
        "{output:?}"
    );
}

#[test]
fn cli_compiler_structure_limits_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("chain.ts");
    std::fs::write(
        &source,
        format!(
            "function main(): number {{ return {}; }}",
            vec!["1"; 10_000].join(" + ")
        ),
    )
    .unwrap();
    for command in ["check", "run"] {
        let output = bounded_cli(command, &source);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("syntax nesting exceeds the compiler limit of 256 levels"),
            "{stderr}"
        );
        assert!(stderr.contains("chain.ts:1:"), "{stderr}");
    }
    std::fs::write(
        &source,
        format!(
            "function main(): number {{ return {}; }}",
            vec!["1"; 200].join(" + ")
        ),
    )
    .unwrap();
    let output = bounded_cli("run", &source);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "200");
    // A template at the lowered height limit is the deepest accepted shape.
    std::fs::write(
        &source,
        format!(
            "function main(): number {{ const a = 1; const s = `{}`; return s.length; }}",
            "${a}x".repeat(510)
        ),
    )
    .unwrap();
    for command in ["check", "run"] {
        let output = bounded_cli(command, &source);
        assert!(output.status.success(), "{output:?}");
    }
}

#[test]
fn cli_closure_arity_returns_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("arity.ts");
    let params = (0..256)
        .map(|i| format!("a{i}: number"))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::write(&source, format!("function f({params}): number {{ return a255; }} function main(): number {{ return 42; }}")).unwrap();
    for command in ["check", "run"] {
        let output = bounded_cli(command, &source);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        for expected in ["256 parameter slots", "maximum is 255", "arity.ts:1:"] {
            assert!(stderr.contains(expected), "{stderr}");
        }
        assert!(!stderr.contains("panicked"), "{stderr}");
        assert!(!stderr.contains("task join"), "{stderr}");
    }
    std::fs::write(&source, "function main(): number { return 42; }").unwrap();
    for command in ["check", "run"] {
        let output = bounded_cli(command, &source);
        assert!(output.status.success(), "{output:?}");
        if command == "run" {
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
        }
    }
}

fn bounded_cli(command: &str, source: &std::path::Path) -> std::process::Output {
    // File-backed output cannot fill a pipe while the parent waits for exit.
    let stdout = tempfile::tempfile().unwrap();
    let stderr = tempfile::tempfile().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_submilli"))
        .arg(command)
        .arg(source)
        .env("SUBMILLI_TELEMETRY", "0")
        .stdout(Stdio::from(stdout.try_clone().unwrap()))
        .stderr(Stdio::from(stderr.try_clone().unwrap()))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            use std::io::{Read, Seek};
            let mut stdout = stdout;
            let mut stderr = stderr;
            stdout.rewind().unwrap();
            stderr.rewind().unwrap();
            let mut output = std::process::Output {
                status,
                stdout: Vec::new(),
                stderr: Vec::new(),
            };
            stdout.read_to_end(&mut output.stdout).unwrap();
            stderr.read_to_end(&mut output.stderr).unwrap();
            return output;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("CLI child timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
