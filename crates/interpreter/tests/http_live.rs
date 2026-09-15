//! Live-wire HTTP fixture harness; fixtures reference the mock server via `{{BASE}}`.

use std::fs;
use std::path::{Path, PathBuf};

use httpmock::Method::{HEAD, PATCH};
use httpmock::prelude::*;
use interpreter::{
    BacktraceMode, Diagnostic, RuntimeConfig, Sources, compile_script, diagnostics,
    render_backtrace,
};

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/http_live_fixtures");

#[test]
fn http_live_fixtures() {
    let server = MockServer::start();
    install_routes(&server);
    let base = server.base_url();

    let mut paths = Vec::new();
    collect(Path::new(FIXTURE_DIR), &mut paths);
    paths.sort();
    assert!(
        !paths.is_empty(),
        "no live fixtures discovered under {FIXTURE_DIR}",
    );

    let mut failures: Vec<String> = Vec::new();
    for p in &paths {
        if let Err(msg) = run_one(p, &base) {
            failures.push(format!("--- {} ---\n{msg}", rel(p)));
        }
    }

    if !failures.is_empty() {
        panic!(
            "\n{} live-fixture failure(s) of {}:\n\n{}",
            failures.len(),
            paths.len(),
            failures.join("\n\n"),
        );
    }
}

fn run_one(path: &Path, base: &str) -> Result<(), String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
    let filename = rel(path);
    let src = raw.replace("{{BASE}}", base);

    let compiled = compile_script(&src, &filename, interpreter::FileId(0), &[], &[])
        .map_err(|diags| format!("compile failed:\n{}", render_diags(&diags, &filename, &src)))?;

    // `run` is async (and this fixture hits a real server via reqwest, so a tokio
    // reactor is required); bridge it on a throwaway runtime for the sync harness.
    let outcome = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("tokio runtime: {e}"))?
        .block_on(RuntimeConfig::default().run_compiled(&compiled));
    match outcome {
        Ok(_) => Ok(()),
        Err(err) => {
            let (sources, file) = Sources::single(filename.as_str(), src);
            // The rendered backtrace already carries the `error: …` header.
            Err(
                match render_backtrace(&err, &sources, file, BacktraceMode::Full) {
                    Some(bt) => format!("trapped:\n{bt}"),
                    None => format!("trapped: {err}"),
                },
            )
        }
    }
}

fn install_routes(server: &MockServer) {
    server.mock(|when, then| {
        when.method(GET).path("/status/200");
        then.status(200)
            .header("content-type", "text/plain")
            .body("hello");
    });

    server.mock(|when, then| {
        when.method(GET).path("/status/404");
        then.status(404)
            .header("content-type", "text/plain")
            .body("not found");
    });

    server.mock(|when, then| {
        when.method(GET).path("/text");
        then.status(200)
            .header("content-type", "text/plain; charset=utf-8")
            .body("the quick brown fox");
    });

    server.mock(|when, then| {
        when.method(GET).path("/json");
        then.status(200)
            .header("content-type", "application/json")
            .body("{\"hello\":\"world\"}");
    });

    server.mock(|when, then| {
        when.method(GET)
            .path("/requires-auth")
            .header("authorization", "Bearer test-token");
        then.status(200)
            .header("content-type", "text/plain")
            .body("authorized");
    });

    server.mock(|when, then| {
        when.method(POST).path("/verb/post");
        then.status(201);
    });
    server.mock(|when, then| {
        when.method(PUT).path("/verb/put");
        then.status(200);
    });
    server.mock(|when, then| {
        when.method(PATCH).path("/verb/patch");
        then.status(200);
    });
    server.mock(|when, then| {
        when.method(DELETE).path("/verb/delete");
        then.status(204);
    });
    server.mock(|when, then| {
        when.method(HEAD).path("/verb/head");
        then.status(200);
    });
    server.mock(|when, then| {
        when.method(OPTIONS).path("/verb/options");
        then.status(204);
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/echo-string-body")
            .header("content-type", "text/plain; charset=utf-8")
            .body("raw payload");
        then.status(200);
    });

    server.mock(|when, then| {
        when.method(POST).path("/echo-bytes").body("Hi!");
        then.status(200);
    });

    server.mock(|when, then| {
        when.method(POST).path("/post-empty").body("");
        then.status(200);
    });

    server.mock(|when, then| {
        when.method(PUT)
            .path("/put-json")
            .header("content-type", "application/json")
            .body("{\"hello\":\"world\"}");
        then.status(200);
    });

    // Object/array bodies are auto-`JSON.stringify`d and tagged
    // `Content-Type: application/json`. The matcher asserts both the serialized
    // bytes and the defaulted header arrived; the response echoes the body so the
    // guest can round-trip it.
    server.mock(|when, then| {
        when.method(POST)
            .path("/echo-json-object")
            .header("content-type", "application/json")
            .body("{\"name\":\"alice\"}");
        then.status(200)
            .header("content-type", "application/json")
            .body("{\"name\":\"alice\"}");
    });

    server.mock(|when, then| {
        when.method(POST)
            .path("/echo-json-array")
            .header("content-type", "application/json")
            .body("[1,2,3]");
        then.status(200)
            .header("content-type", "application/json")
            .body("[1,2,3]");
    });

    // A user-supplied Content-Type must win over the application/json default.
    // The matcher requires the custom header, so a 200 proves the shim did not
    // override it.
    server.mock(|when, then| {
        when.method(POST)
            .path("/echo-json-custom-ct")
            .header("content-type", "application/vnd.api+json")
            .body("{\"name\":\"alice\"}");
        then.status(200)
            .header("content-type", "application/json")
            .body("{\"name\":\"alice\"}");
    });

    server.mock(|when, then| {
        when.method(GET).path("/dl/small");
        then.status(200)
            .header("content-type", "text/plain")
            .body("hello live");
    });

    server.mock(|when, then| {
        when.method(GET).path("/dl/large");
        then.status(200)
            .header("content-type", "application/octet-stream")
            .body("0123456789abcdef");
    });

    let gzip_body: Vec<u8> = {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(b"hello gzipped live").expect("gzip encode");
        enc.finish().expect("gzip finish")
    };
    server.mock(move |when, then| {
        when.method(GET).path("/dl/gzip");
        then.status(200)
            .header("content-encoding", "gzip")
            .header("content-type", "text/plain")
            .body(&gzip_body);
    });
}

fn render_diags(diags: &[Diagnostic], filename: &str, src: &str) -> String {
    let (sources, _) = Sources::single(filename, src);
    diags
        .iter()
        .map(|d| diagnostics::render(d, &sources))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir({}): {e}", dir.display()));
    for entry in entries {
        let p = entry.expect("dir entry").path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("subm") {
            out.push(p);
        }
    }
}

fn rel(p: &Path) -> String {
    p.strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .unwrap_or(p)
        .display()
        .to_string()
}
