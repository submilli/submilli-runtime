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
