//! Server defaults use the server/ directory and leave legacy state untouched.

#![cfg(unix)]

mod common;

use std::path::Path;
use std::process::Child;
use std::time::Duration;

use common::{free_port, signal, spawn_server, wait_for_exit, wait_ready};
use submilli_blueprint::Blueprint;
use submilli_server::blueprint::{BlueprintStore, FileBlueprintStore};

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn legacy_state_is_left_untouched_when_server_directory_is_missing() {
    let home = tempfile::tempdir().expect("temp home");
    seed_blueprint(&home.path().join("blueprints"), "legacy");
    let port = free_port();

    let mut server = spawn_server(home.path(), port, 1, &[]);
    wait_ready(port);
    let names = blueprint_names(port);
    stop(&mut server);

    assert!(names.is_empty(), "legacy blueprints must not be imported");
    assert!(home.path().join("server/db/submilli.db").is_file());
    assert!(
        home.path().join("blueprints").is_dir(),
        "legacy dir stays untouched"
    );
    assert!(!home.path().join("server.migrating").exists());
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn an_explicitly_configured_directory_is_imported_then_archived() {
    let home = tempfile::tempdir().expect("temp home");
    let explicit = home.path().join("blueprints");
    seed_blueprint(&explicit, "pinned");
    let port = free_port();

    let mut server = spawn_server(
        home.path(),
        port,
        1,
        &[("SUBMILLI_BLUEPRINT_DIR", explicit.to_str().unwrap())],
    );
    wait_ready(port);
    let names = blueprint_names(port);
    stop(&mut server);

    assert_eq!(names, vec!["pinned".to_string()]);
    assert!(!explicit.exists(), "the whole source directory is archived");
    assert!(home.path().join("archive/blueprints/index.json").is_file());
    assert!(!home.path().join("server/blueprints").exists());
}

fn seed_blueprint(dir: &Path, name: &str) {
    let store = FileBlueprintStore::new(dir.to_path_buf()).expect("file blueprint store");
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(store.add(Blueprint {
            name: name.into(),
            ..Default::default()
        }))
        .expect("seed blueprint");
}

fn blueprint_names(port: u16) -> Vec<String> {
    let body: serde_json::Value = ureq::get(&format!("http://127.0.0.1:{port}/v1/blueprints"))
        .call()
        .expect("list blueprints")
        .body_mut()
        .read_json()
        .expect("json body");
    body["blueprints"]
        .as_array()
        .expect("blueprints array")
        .iter()
        .map(|b| b["name"].as_str().expect("name").to_string())
        .collect()
}

fn stop(server: &mut Child) {
    signal(server, libc::SIGTERM);
    wait_for_exit(server, Duration::from_secs(30));
}
