//! A blueprint with a filter past the nesting limit fails to parse on a
//! request-sized stack, rather than overflowing it. The limit itself is tested
//! in `submilli-policy`; this covers the whole blueprint parse around it.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn deep_blueprint_filter_on_request_stack() {
    if std::env::var_os("SUBMILLI_BLUEPRINT_FILTER_LIMIT_CHILD").is_some() {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(check_deep_blueprint_filter)
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    // Stack overflows abort; isolate the regression and bound its running time.
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "deep_blueprint_filter_on_request_stack",
            "--nocapture",
        ])
        .env("SUBMILLI_BLUEPRINT_FILTER_LIMIT_CHILD", "1")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "blueprint filter child failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("blueprint filter child timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn check_deep_blueprint_filter() {
    let yaml = format!(
        "name: x\npermissions:\n  main:\n    - capability: c\n      filter: {}x == 1{}\n      action: allow\n",
        "(".repeat(3_000),
        ")".repeat(3_000)
    );
    assert!(
        submilli_blueprint::parse(&yaml)
            .unwrap_err()
            .to_string()
            .contains("operators/groups")
    );
    assert!(submilli_blueprint::parse("name: healthy").is_ok());
}
