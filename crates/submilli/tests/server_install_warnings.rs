//! Selected localhost coverage for install request and warning response encoding.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn deny_warnings_remote_install_forwards_policy_and_renders_response() {
    for (status, body, denied) in [
        (
            "200 OK",
            r#"{"sha":"0000000000000000000000000000000000000000","installed":["@pied-piper/pkg"],"up_to_date":[],"warnings":["warning: capability mismatch\n"]}"#,
            false,
        ),
        (
            "400 Bad Request",
            r#"{"error":"warnings_denied","message":"1 warning(s) treated as errors (--deny-warnings)","warnings":["warning: capability mismatch\n"]}"#,
            true,
        ),
    ] {
        for environment in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                loop {
                    let count = socket.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                    let text = String::from_utf8_lossy(&request);
                    if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|length| length.trim().parse().unwrap())
                            })
                            .unwrap();
                        if body.len() >= length {
                            let value: serde_json::Value = serde_json::from_str(body).unwrap();
                            assert_eq!(value["deny_warnings"], true);
                            break;
                        }
                    }
                }
                write!(socket, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            });
            let home = tempfile::tempdir().unwrap();
            let mut command = Command::new(env!("CARGO_BIN_EXE_submilli"));
            command
                .args([
                    "server",
                    "packages",
                    "install",
                    "pied-piper/pkg",
                    "--server",
                    &format!("http://{address}"),
                ])
                .env("SUBMILLI_HOME", home.path())
                .env_remove("SUBMILLI_SERVER_TOKEN")
                .env_remove("SUBMILLI_SERVER_TOKEN_FILE");
            if environment {
                command.env("SUBMILLI_DENY_WARNINGS", "1");
            } else {
                command
                    .arg("--deny-warnings")
                    .env_remove("SUBMILLI_DENY_WARNINGS");
            }
            let output = command.output().unwrap();
            server.join().unwrap();
            assert_eq!(output.status.success(), !denied);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("warning: capability mismatch"), "{stderr}");
            if denied {
                assert!(
                    stderr.contains("error: 1 warning(s) treated as errors"),
                    "{stderr}"
                );
            }
        }
    }
}

#[test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
fn package_config_remote_install_preserves_inputs_and_dependency_target() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = socket.read(&mut buffer).unwrap();
            assert!(count > 0);
            request.extend_from_slice(&buffer[..count]);
            let text = String::from_utf8_lossy(&request);
            let Some((headers, body)) = text.split_once("\r\n\r\n") else {
                continue;
            };
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|length| length.trim().parse().unwrap())
                })
                .unwrap();
            if body.len() < length {
                continue;
            }
            let value: serde_json::Value = serde_json::from_str(body).unwrap();
            assert_eq!(value["package"], "@pied-piper/app");
            assert_eq!(value["config"]["baseUrl"], "https://app.test/?a=b");
            assert_eq!(value["reset_config"], serde_json::json!(["timeoutMs"]));
            break;
        }
        let body = serde_json::json!({
            "error": "configuration_required", "message": "install the dependency first", "warnings": [],
            "configuration": { "package": "@pied-piper/service", "parameters": {"baseUrl":"string"}, "command": "submilli server packages install pied-piper/service@aaaaaaaa @pied-piper/service --config 'baseUrl=<string>'" }
        }).to_string();
        write!(socket, "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    });
    let home = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args([
            "server",
            "packages",
            "install",
            "pied-piper/app",
            "@pied-piper/app",
            "--config",
            "baseUrl=https://app.test/?a=b",
            "--reset-config",
            "timeoutMs",
            "--server",
            &format!("http://{address}"),
        ])
        .env("SUBMILLI_HOME", home.path())
        .env_remove("SUBMILLI_SERVER_TOKEN")
        .env_remove("SUBMILLI_SERVER_TOKEN_FILE")
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("@pied-piper/service --config 'baseUrl=<string>'"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("--server 'http://{address}'")),
        "{stderr}"
    );
}
