use std::future::Future;
use std::process::Command;
use std::time::{Duration, Instant};

/// Run the real handler on a production-sized Tokio stack in an isolated child.
pub fn isolated_worker(test_name: &str, check: impl Future<Output = ()> + Send + 'static) {
    if std::env::var_os("SUB633_SERVER_CHILD").is_some() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_stack_size(2 * 1024 * 1024)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async { tokio::spawn(check).await.unwrap() });
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture"])
        .env("SUB633_SERVER_CHILD", "1")
        .env("SUBMILLI_TELEMETRY", "0")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "handler child failed: {status}");
            return;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("handler child timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub fn nested_source() -> String {
    format!(
        "function main(): number {{ return {}1{}; }}",
        "(".repeat(2048),
        ")".repeat(2048)
    )
}

/// Parsed iteratively, but every later compiler walk recurses over its height.
pub fn flat_chain_source() -> String {
    format!(
        "function main(): number {{ return {}; }}",
        vec!["1"; 10_000].join(" + ")
    )
}

/// Needs more than a 2 MiB worker stack to compile in unoptimized builds.
pub fn near_limit_chain_source() -> String {
    format!(
        "function main(): number {{ return {}; }}",
        vec!["1"; 200].join(" + ")
    )
}

pub const SYNTAX_LIMIT_MESSAGE: &str = "syntax nesting exceeds the compiler limit";

pub fn oversized_closure_source() -> String {
    let params = (0..256)
        .map(|i| format!("a{i}: number"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "function f({params}): number {{ return a255; }} function main(): number {{ return 42; }}"
    )
}

pub fn assert_closure_diagnostic(body: &serde_json::Value) {
    assert_eq!(body["error"]["kind"], "compile_error", "{body}");
    assert!(body["result"].is_null(), "{body}");
    let diagnostics = body["error"]["diagnostics"].as_array().unwrap();
    let diagnostic = diagnostics
        .iter()
        .find(|d| {
            d["message"]
                .as_str()
                .is_some_and(|m| m.contains("256 parameter slots"))
        })
        .expect("arity diagnostic");
    assert!(
        diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("maximum is 255"),
        "{diagnostic}"
    );
    assert_eq!(diagnostic["line"], 1, "{diagnostic}");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(!message.contains("panicked"), "{message}");
    assert!(!message.contains("task join"), "{message}");
}
