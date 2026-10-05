//! Measures the native memory `submilli:git` really uses per operation.
//! `TenantLimits` only sees what Git charges, so a process-wide
//! `CountingAllocator` measures every host allocation independently, including
//! gix's on the blocking worker. Pack files gix memory-maps are file-backed and
//! not counted; they're paged from disk, not held by the process.
//!
//! Set `SUBMILLI_TEST_NIGHTLY_ONLY=1` for nightly, release, or Git stdlib checks.
//! `SUBMILLI_FULL_TEST` does not enable these measurements.

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use interpreter::stdlib::git::GitConfig;
use interpreter::stdlib::http::transport::{
    DownloadMeta, HttpClient, HttpError, HttpRequest, HttpResponse,
};
use interpreter::{
    compile_script, dispatch_main_async,
    runtime::{RuntimeConfig, StoreData, Vfs, install_runtime_async, install_tenant_limits},
};
use wasmtime::{Linker, Module};

struct CountingAllocator;

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn bump_peak(new: usize) {
    PEAK.fetch_max(new, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            bump_peak(ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed) + layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        ALLOCATED.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: caller guarantees `ptr` was allocated by us with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            bump_peak(ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed) + layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            let old_size = layout.size();
            if new_size > old_size {
                let delta = new_size - old_size;
                bump_peak(ALLOCATED.fetch_add(delta, Ordering::Relaxed) + delta);
            } else {
                ALLOCATED.fetch_sub(old_size - new_size, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static A: CountingAllocator = CountingAllocator;

/// Serialise measurements so the global counters see one run at a time.
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Generous, so the measurement is of what Git uses, not of what it's refused.
const MEASURING_CAP: u64 = 4 * 1024 * 1024 * 1024;

fn native(repo: &Path, args: &[&str]) -> Vec<u8> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Seed")
        .env("GIT_AUTHOR_EMAIL", "seed@example.com")
        .env("GIT_COMMITTER_NAME", "Seed")
        .env("GIT_COMMITTER_EMAIL", "seed@example.com")
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

/// The shape of a generated repository.
#[derive(Clone, Copy, Debug)]
struct Shape {
    files: usize,
    file_bytes: usize,
    /// Files per directory.
    fanout: usize,
}

impl Shape {
    fn total_bytes(self) -> usize {
        self.files * self.file_bytes
    }
}

/// Deterministic, poorly compressible file contents, so pack size tracks
/// repository size.
fn contents(seed: usize, len: usize) -> Vec<u8> {
    let mut state = (seed as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            b'a' + (state % 26) as u8
        })
        .collect()
}

/// A committed, packed repository at `dir` with a `topic` branch that changes
/// one file and a staged and unstaged change on `main`.
fn seed_repository(dir: &Path, shape: Shape) {
    std::fs::create_dir_all(dir).expect("repository directory");
    native(dir, &["init", "-q", "-b", "main"]);
    for file in 0..shape.files {
        let path = dir.join(format!("d{:04}/f{file:06}", file / shape.fanout));
        std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        std::fs::write(path, contents(file, shape.file_bytes)).expect("file");
    }
    native(dir, &["add", "-A"]);
    native(dir, &["commit", "-q", "-m", "initial"]);
    native(dir, &["switch", "-q", "-c", "topic"]);
    std::fs::write(dir.join("d0000/f000000"), "topic\n").expect("topic change");
    native(dir, &["commit", "-q", "-am", "topic"]);
    native(dir, &["switch", "-q", "main"]);
    native(dir, &["gc", "-q", "--aggressive"]);
}

/// Serves `repo` over smart HTTP by running native `git upload-pack`.
struct UploadPack {
    repo: std::path::PathBuf,
}

#[async_trait::async_trait]
impl HttpClient for UploadPack {
    async fn send(&self, _: &HttpRequest) -> Result<HttpResponse, HttpError> {
        panic!("Git must not use a redirect-following transport")
    }

    /// Streams `upload-pack`'s output, so the server holds none of it: the
    /// measurement is of Git alone.
    async fn send_without_redirects_to(
        &self,
        request: &HttpRequest,
        body: &mut (dyn std::io::Write + Send),
    ) -> Result<HttpResponse, HttpError> {
        use std::io::Write;
        if request.method == "GET" {
            body.write_all(b"001e# service=git-upload-pack\n0000")
                .expect("write body");
            body.write_all(&native(
                &self.repo,
                &["upload-pack", "--stateless-rpc", "--advertise-refs", "."],
            ))
            .expect("write body");
            return Ok(HttpResponse {
                status: 200,
                status_text: "OK".into(),
                headers: vec![(
                    "content-type".into(),
                    "application/x-git-upload-pack-advertisement".into(),
                )],
                body: Vec::new(),
                final_url: request.url.clone(),
            });
        }
        let mut child = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(["upload-pack", "--stateless-rpc", "."])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("spawn upload-pack");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(&request.body)
            .expect("request body");
        let mut stdout = child.stdout.take().expect("stdout");
        let mut chunk = [0u8; 16 * 1024];
        loop {
            let read = std::io::Read::read(&mut stdout, &mut chunk).expect("read upload-pack");
            if read == 0 {
                break;
            }
            body.write_all(&chunk[..read]).expect("write body");
        }
        assert!(child.wait().expect("upload-pack").success());
        Ok(HttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![(
                "content-type".into(),
                "application/x-git-upload-pack-result".into(),
            )],
            body: Vec::new(),
            final_url: request.url.clone(),
        })
    }

    async fn download(
        &self,
        _: &HttpRequest,
        _: &mut (dyn std::io::Write + Send),
    ) -> Result<DownloadMeta, HttpError> {
        unreachable!("Git never downloads")
    }
}

/// What one run used.
#[derive(Debug)]
struct Measured {
    /// Peak bytes allocated during the run, over what was allocated before it.
    peak: usize,
    /// The run's peak under its memory cap: engine memory plus charged host bytes.
    charged: u64,
}

/// Runs `src` against a VFS holding what `seed` put there. Compiling and
/// instantiating happen before the measurement starts.
fn measure(
    src: &str,
    cap: u64,
    seed: impl FnOnce(&Path),
    configure: impl FnOnce(&mut StoreData),
) -> Result<Measured, String> {
    let compiled = compile_script(src, "git_memory.ts", interpreter::FileId(0), &[], &[])
        .unwrap_or_else(|error| panic!("{error:#?}"));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine().expect("engine");
    let tmp = tempfile::tempdir().expect("tempdir");
    seed(tmp.path());
    let vfs = Vfs::external(tmp.path().to_path_buf()).expect("external vfs");
    let mut data = StoreData::with_vfs_and_cap(vfs, cap);
    data.git = Some(GitConfig {
        name: "Agent".into(),
        email: "agent@example.com".into(),
        username: None,
    });
    configure(&mut data);
    data.install_type_info(compiled.type_info.clone());
    let mut store = cfg.store(&engine, data).expect("store");
    install_tenant_limits(&mut store);
    let module = Module::new(&engine, &compiled.wasm).expect("module");
    let mut linker = Linker::<StoreData>::new(&engine);
    let instance = runtime.block_on(async {
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install runtime");
        linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate")
    });

    let baseline = ALLOCATED.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let outcome = runtime.block_on(dispatch_main_async(&mut store, &instance));
    let peak = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    let charged = store.data().tenant_limits.peak_bytes();
    drop(store);
    drop(runtime);
    outcome
        .map(|_| Measured { peak, charged })
        .map_err(|error| format!("{error:#}"))
}

/// One operation to measure, with what it needs prepared beforehand.
struct Operation {
    name: &'static str,
    /// The body of `main`, with `repo` already opened at `/repo` unless the
    /// operation creates it.
    body: &'static str,
    prepare: fn(&Path),
    remote: bool,
}

fn no_preparation(_: &Path) {}

fn modify_one_file(repo: &Path) {
    std::fs::write(repo.join("d0000/f000001"), "changed\n").expect("change");
}

fn stage_one_file(repo: &Path) {
    modify_one_file(repo);
    native(repo, &["add", "d0000/f000001"]);
}

const OPERATIONS: &[Operation] = &[
    Operation {
        name: "open",
        body: "",
        prepare: no_preparation,
        remote: false,
    },
    Operation {
        name: "status",
        body: "repo.status();",
        prepare: modify_one_file,
        remote: false,
    },
    Operation {
        name: "diff working",
        body: r#"repo.diff({ mode: "working" });"#,
        prepare: modify_one_file,
        remote: false,
    },
    Operation {
        name: "diff staged",
        body: r#"repo.diff({ mode: "staged" });"#,
        prepare: stage_one_file,
        remote: false,
    },
    Operation {
        name: "diff refs",
        body: r#"repo.diff({ mode: "refs", from: "main", to: "topic" });"#,
        prepare: no_preparation,
        remote: false,
    },
    Operation {
        name: "show",
        body: r#"repo.show("HEAD", "d0000/f000002");"#,
        prepare: no_preparation,
        remote: false,
    },
    Operation {
        name: "add",
        body: r#"repo.add(["d0000/f000001"]);"#,
        prepare: modify_one_file,
        remote: false,
    },
    Operation {
        name: "commit",
        body: r#"repo.commit("one file");"#,
        prepare: stage_one_file,
        remote: false,
    },
    Operation {
        name: "switch",
        body: r#"repo.switchBranch("topic");"#,
        prepare: no_preparation,
        remote: false,
    },
    Operation {
        name: "log",
        body: "repo.log();",
        prepare: no_preparation,
        remote: false,
    },
    Operation {
        name: "branches",
        body: "repo.branches();",
        prepare: no_preparation,
        remote: false,
    },
    Operation {
        name: "remotes",
        body: "repo.remotes();",
        prepare: no_preparation,
        remote: false,
    },
    Operation {
        name: "addRemote",
        body: r#"repo.addRemote("mirror", "https://example.com/mirror.git");"#,
        prepare: no_preparation,
        remote: false,
    },
    Operation {
        name: "fetch",
        body: r#"repo.fetch("origin", "topic");"#,
        prepare: no_preparation,
        remote: true,
    },
    Operation {
        name: "clone",
        body: "",
        prepare: no_preparation,
        remote: true,
    },
];

/// Measures `operation` on a repository of `shape`, under `cap`.
fn measure_operation(operation: &Operation, shape: Shape, cap: u64) -> Result<Measured, String> {
    let upstream = tempfile::tempdir().expect("upstream");
    seed_repository(upstream.path(), shape);
    let clone = operation.name == "clone";
    let src = if clone {
        r#"
        import { Repository } from "submilli:git";
        function main(): void {
            Repository.clone("https://example.com/upstream.git", "/repo");
        }"#
        .to_owned()
    } else {
        format!(
            r#"
        import {{ Repository }} from "submilli:git";
        function main(): void {{
            const repo = Repository.open("/repo");
            {}
        }}"#,
            operation.body
        )
    };
    let upstream_path = upstream.path().to_path_buf();
    measure(
        &src,
        cap,
        |root| {
            if clone {
                return;
            }
            let repo = root.join("repo");
            seed_repository(&repo, shape);
            if operation.remote {
                native(
                    &repo,
                    &[
                        "remote",
                        "add",
                        "origin",
                        "https://example.com/upstream.git",
                    ],
                );
                // A fetch that brings something new: the upstream's topic
                // branch moves past what the local copy has.
                std::fs::write(upstream_path.join("d0000/f000003"), "upstream\n")
                    .expect("upstream change");
                native(&upstream_path, &["switch", "-q", "topic"]);
                native(&upstream_path, &["commit", "-q", "-am", "upstream"]);
                native(&upstream_path, &["switch", "-q", "main"]);
            }
            (operation.prepare)(&repo);
        },
        |data| {
            data.http_client = Arc::new(UploadPack {
                repo: upstream_path.clone(),
            });
        },
    )
}

/// Prints each operation's peak native memory against repository size.
/// Calibration for the per-operation estimates: run with
/// `SUBMILLI_TEST_NIGHTLY_ONLY=1 cargo test -p interpreter --test git_memory -- --ignored --nocapture`.
#[test]
#[ignore = "calibration report; slow"]
fn report_peak_memory_per_operation() {
    if !nightly_only_requested() {
        eprintln!("Git memory calibration: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
        return;
    }
    let _guard = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let shapes = [
        Shape {
            files: 100,
            file_bytes: 10_000,
            fanout: 20,
        },
        Shape {
            files: 1_000,
            file_bytes: 10_000,
            fanout: 50,
        },
        Shape {
            files: 100,
            file_bytes: 200_000,
            fanout: 20,
        },
    ];
    for shape in shapes {
        eprintln!(
            "\nshape {shape:?}: {} bytes of content",
            shape.total_bytes()
        );
        // `GIT_MEMORY_OPERATIONS=fetch,clone` measures only those.
        let only = std::env::var("GIT_MEMORY_OPERATIONS").ok();
        for operation in OPERATIONS.iter().filter(|operation| {
            only.as_deref()
                .is_none_or(|only| only.split(',').any(|name| name == operation.name))
        }) {
            match measure_operation(operation, shape, MEASURING_CAP) {
                Ok(measured) => eprintln!(
                    "  {:<14} peak {:>12} bytes ({:>6.2}× content), charged peak {:>12}",
                    operation.name,
                    measured.peak,
                    measured.peak as f64 / shape.total_bytes() as f64,
                    measured.charged,
                ),
                Err(error) => eprintln!("  {:<14} failed: {error}", operation.name),
            }
        }
    }
}

/// The memory limit a run gets unless the blueprint sets one.
const DEFAULT_CAP: u64 = interpreter::runtime::limits::DEFAULT_MAX_STORE_BYTES;

/// A repository at `dir` holding `files`, each `(path, bytes)`, committed on
/// `main`.
fn seed_files(dir: &Path, files: &[(String, usize)]) {
    std::fs::create_dir_all(dir).expect("repository directory");
    native(dir, &["init", "-q", "-b", "main"]);
    for (number, (path, len)) in files.iter().enumerate() {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        std::fs::write(path, contents(number, *len)).expect("file");
    }
    native(dir, &["add", "-A"]);
    native(dir, &["commit", "-q", "-m", "initial"]);
    native(dir, &["gc", "-q"]);
}

/// Clones the upstream repository seeded with `files` under the default memory
/// limit, then checks it and commits a change on a new branch.
fn clone_and_work(files: &[(String, usize)]) -> Result<Measured, String> {
    let upstream = tempfile::tempdir().expect("upstream");
    seed_files(upstream.path(), files);
    let upstream_path = upstream.path().to_path_buf();
    measure(
        r#"
        import { Repository } from "submilli:git";
        import { writeText } from "submilli:fs";
        function main(): void {
            const repo = Repository.clone("https://example.com/upstream.git", "/repo");
            assert(repo.status().clean, "clean after clone");
            repo.createBranch("work");
            repo.switchBranch("work");
            writeText("/repo/notes.txt", "notes\n");
            repo.add(["notes.txt"]);
            repo.commit("notes");
            assert(repo.status().clean, "clean after commit");
            repo.switchBranch("main");
            assert(repo.status().clean, "clean after switching back");
        }"#,
        DEFAULT_CAP,
        |_| {},
        |data| {
            data.http_client = Arc::new(UploadPack {
                repo: upstream_path,
            });
        },
    )
}

/// Under the default memory limit, a 10 MB repository of 1,000 files is
/// cloned, checked, committed to and switched, and the run's real peak memory
/// stays within what it was charged.
#[test]
fn a_large_repository_works_end_to_end_under_the_default_limit() {
    if !nightly_only_requested() {
        eprintln!("Git repository memory: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
        return;
    }
    let _guard = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let files: Vec<_> = (0..1_000)
        .map(|number| (format!("d{:02}/f{number:04}", number / 50), 10_000))
        .collect();
    let measured = clone_and_work(&files).expect("a 10 MB repository fits");
    assert!(
        (measured.peak as u64) < measured.charged,
        "{measured:?}: Git used more than it was charged"
    );
}

/// A file that fits in the memory Git has is checked out; one that doesn't is
/// refused with a message naming what to raise.
#[test]
fn a_file_larger_than_git_may_hold_is_refused_clearly() {
    if !nightly_only_requested() {
        eprintln!("Git file memory: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
        return;
    }
    let _guard = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let measured =
        clone_and_work(&[("large".into(), 6_000_000)]).expect("a 6 MB file fits under 50 MB");
    assert!((measured.peak as u64) < measured.charged, "{measured:?}");
    let error = clone_and_work(&[("large".into(), 12_000_000)])
        .expect_err("a 12 MB file needs more than Git has under 50 MB");
    assert!(error.contains("raise max_execution_memory"), "{error}");
}

fn nightly_only_requested() -> bool {
    std::env::var("SUBMILLI_TEST_NIGHTLY_ONLY").is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}
