//! A raised `max_execution_stack`, as the real binary runs it.
//!
//! A host call that re-enters Wasm nests frames on the runtime thread's native
//! stack. The server sizes those threads from the setting; if it didn't, deep
//! re-entry would overflow a worker and abort the whole process, which only a
//! test outside the process can see.

#![cfg(unix)]

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use common::{free_port, spawn_server, wait_ready};

const REENTRY: &str = "function depth(n: number): number { if (n === 0) { return 0; } return [n].map((x: number) => depth(x - 1))[0] + 1; } function main(): number { return depth(200000); }";

fn post(port: u16, path: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(300)))
        .expect("read timeout");
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("write request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    response
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn deep_reentry_under_a_raised_stack_leaves_the_server_running() {
    let home = tempfile::tempdir().expect("temp home");
    let seeds = tempfile::tempdir().expect("seed dir");
    std::fs::write(seeds.path().join("t.yaml"), "kind: blueprint\nname: t\n").expect("seed");
    let port = free_port();
    let mut server = spawn_server(
        home.path(),
        port,
        1,
        &[
            ("SUBMILLI_MAX_EXECUTION_STACK", "1024"),
            (
                "SUBMILLI_BLUEPRINT_SEED_DIR",
                seeds.path().to_str().expect("utf-8 path"),
            ),
        ],
    );
    wait_ready(port);

    let body = serde_json::json!({ "blueprint": "t", "code": REENTRY }).to_string();
    let response = post(port, "/v1/execute", &body);

    let still_running = server.try_wait().expect("try_wait").is_none();
    let _ = server.kill();
    let _ = server.wait();
    assert!(still_running, "the server process died");
    assert!(
        response.contains("call stack exhausted"),
        "expected the run to end at the stack limit, got: {response}"
    );
}
