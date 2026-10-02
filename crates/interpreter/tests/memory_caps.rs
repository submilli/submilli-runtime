use std::sync::Arc;

use interpreter::{
    compile_script, dispatch_main_async,
    runtime::{
        MemoryExhausted, RuntimeConfig, StoreData, Vfs, install_runtime_async,
        install_tenant_limits, is_memory_exhausted,
    },
    stdlib::http::transport::{
        Decompression, DownloadMeta, HttpClient, HttpError, HttpRequest, HttpResponse,
        stream_to_writer,
    },
};
use wasmtime::{Linker, Module};

fn cap_run(src: &str, max_store_bytes: u64) -> wasmtime::Result<()> {
    let compiled = compile_script(src, "test.subm", interpreter::FileId(0), &[], &[])
        .map_err(|d| wasmtime::Error::msg(format!("compile failed: {d:#?}")))?;
    // One engine may serve tenants with smaller caps than its own defaults.
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine()?;
    let data = StoreData::with_vfs_and_cap(Vfs::tempdir()?, max_store_bytes);
    let mut store = cfg.store(&engine, data)?;
    install_tenant_limits(&mut store);
    let module = Module::new(&engine, &compiled.wasm)?;
    let mut linker = Linker::<StoreData>::new(&engine);
    pollster::block_on(async {
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("runtime initialization fits within cap");
        let instance = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("script globals fit within cap");
        dispatch_main_async(&mut store, &instance).await.map(|_| ())
    })
}

/// Asserts `src` ends at the cap rather than in its own `catch`: each program
/// below returns normally from its handler, so reaching one is an `Ok`.
fn assert_cap_ends_the_run(src: &str, max_store_bytes: u64) {
    let err = cap_run(src, max_store_bytes)
        .expect_err("the run should end at the cap, not continue in the handler");
    assert!(
        err.is::<MemoryExhausted>(),
        "expected memory exhaustion, got: {err:#}"
    );
    assert!(is_memory_exhausted(&err));
    assert_eq!(err.to_string(), "memory exhausted");
}

#[test]
fn a_guest_allocation_past_the_cap_is_not_catchable() {
    let src = r#"
class Node {
  next: Node | null;
  constructor(next: Node | null) { this.next = next; }
}
function main(): void {
  let head: Node | null = null;
  try {
    while (true) {
      head = new Node(head);
    }
  } catch (e) {
    head = null;
  } finally {
    head = null;
  }
}
"#;
    assert_cap_ends_the_run(src, 256 * 1024);
}

#[test]
fn a_host_allocation_past_the_cap_is_not_catchable() {
    let src = r#"
function main(): void {
  let text: string = "x";
  try {
    while (true) {
      text = text + text;
    }
  } catch (e) {
    text = "";
  }
}
"#;
    assert_cap_ends_the_run(src, 256 * 1024);
}

#[test]
fn an_allocation_past_the_cap_inside_a_callback_is_not_catchable() {
    let src = r#"
function main(): void {
  const kept: number[][] = [];
  try {
    [1, 2, 3].forEach((seed: number) => {
      while (true) {
        kept.push([seed, seed, seed, seed]);
      }
    });
  } catch (e) {
    kept.pop();
  }
}
"#;
    assert_cap_ends_the_run(src, 256 * 1024);
}

#[test]
fn a_regex_compile_past_the_cap_is_not_catchable() {
    let src = r#"
function main(): void {
  const kept: RegExp[] = [];
  try {
    let i: number = 0;
    while (i < 10) {
      kept.push(new RegExp("abc" + i.toString(), ""));
      i = i + 1;
    }
  } catch (e) {
    kept.pop();
  }
}
"#;
    assert_cap_ends_the_run(src, 128 * 1024);
}

#[test]
fn a_file_handle_past_the_cap_is_not_catchable() {
    let src = r#"
import { lines, writeText } from "submilli:fs";
function main(): void {
  writeText("/data.txt", "alpha\nbeta\ngamma\n");
  const held: Iterator<string>[] = [];
  try {
    let i: number = 0;
    while (i < 500) {
      held.push(lines("/data.txt"));
      i = i + 1;
    }
  } catch (e) {
    held.pop();
  }
}
"#;
    assert_cap_ends_the_run(src, 1024 * 1024);
}

/// Building the error for an ordinary host failure can itself be what reaches
/// the cap: the message for a path that does not exist repeats the path. The
/// run then ends as out of memory, not as a host failure, and not in `catch`.
#[test]
fn an_error_that_cannot_be_built_at_the_cap_ends_the_run_as_memory_exhausted() {
    // 5.6 MB of filler and a 1.5 MB path fit an 8 MiB cap; the message does not.
    let src = r#"
import { lines } from "submilli:fs";
function main(): void {
  const filler = "f".repeat(2800000);
  const path = "/a".repeat(375000);
  try {
    lines(path);
  } catch (e) {
    assert(filler.length > 0, "filler stays live past the failing call");
  }
}
"#;
    assert_cap_ends_the_run(src, 8 * 1024 * 1024);
}

/// The same failure is an ordinary error the program can catch when there is
/// room to build it.
#[test]
fn a_host_error_that_fits_under_the_cap_stays_catchable() {
    let src = r#"
import { lines } from "submilli:fs";
function main(): void {
  const path = "/a".repeat(375000);
  let caught = false;
  try {
    lines(path);
  } catch (e) {
    caught = true;
  }
  assert(caught, "a missing file is an ordinary error");
}
"#;
    cap_run(src, 8 * 1024 * 1024).expect("the error fits and is caught");
}

#[test]
fn tenant_cap_traps_on_excess_growth() {
    let src = r#"
function main(): void {
  let arr: number[] = [];
  let i: number = 0;
  while (i < 10000) {
    arr.push(i);
    i = i + 1;
  }
}
"#;

    // 64 KB: well under a single array-push's doubled backing allocation.
    let err = cap_run(src, 64 * 1024).expect_err("cap should reject growth");
    let msg = format!("{err:#}");
    assert!(
        msg.to_lowercase().contains("memory")
            || msg.to_lowercase().contains("oom")
            || msg.to_lowercase().contains("grow"),
        "expected OOM / memory-growth language in trap, got: {msg}"
    );
}

#[test]
fn gc_heap_capacity_returns_sensible_value() {
    let src = r#"
function main(): void {
  let arr: number[] = [];
  let i: number = 0;
  while (i < 1000) {
    arr.push(i);
    i = i + 1;
  }
}
"#;

    let cfg = RuntimeConfig::default();
    let compiled =
        compile_script(src, "test.subm", interpreter::FileId(0), &[], &[]).expect("compile");
    let engine = cfg.engine().expect("engine");
    let data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
    let mut store = cfg.store(&engine, data).expect("store");
    install_tenant_limits(&mut store);
    let module = Module::new(&engine, &compiled.wasm).expect("module");
    let mut linker = Linker::<StoreData>::new(&engine);
    let instance = pollster::block_on(async {
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install runtime");
        linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate")
    });
    pollster::block_on(dispatch_main_async(&mut store, &instance)).expect("run");

    let capacity = store.gc_heap_capacity() as u64;
    let observed = store.data().tenant_limits.observed_bytes();
    assert!(
        observed >= capacity,
        "GC allocations must be charged: {observed} < {capacity}"
    );
    assert!(
        capacity > 0,
        "expected non-zero gc_heap_capacity after allocation"
    );
    assert!(
        capacity <= cfg.max_store_bytes,
        "expected capacity ({capacity}) <= max_store_bytes ({})",
        cfg.max_store_bytes,
    );
}

#[test]
fn small_tenant_can_run_within_its_cap() {
    cap_run(
        "function main(): void { const xs = [1, 2, 3]; assert(xs[2] === 3, \"value\"); }",
        64 * 1024,
    )
    .expect("small live heap fits");
}

#[test]
fn gc_reservation_counts_toward_host_allocations() {
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine().expect("engine");
    let data = StoreData::with_vfs_and_cap(Vfs::tempdir().expect("vfs"), 128 * 1024);
    let mut store = cfg.store(&engine, data).expect("store");
    install_tenant_limits(&mut store);
    let mut linker = Linker::<StoreData>::new(&engine);
    pollster::block_on(install_runtime_async(&mut linker, &mut store)).expect("runtime");
    let limits = &store.data().tenant_limits;
    let observed = limits.observed_bytes();
    assert!(
        observed > 0,
        "initial runtime GC reservation must be charged"
    );
    let remaining = limits.max_total_bytes - observed - limits.host_attached_bytes();
    limits
        .charge_host_bytes(remaining)
        .expect("exact aggregate cap");
    assert!(
        limits.charge_host_bytes(1).is_err(),
        "GC plus host bytes exceed cap"
    );
}

/// Emits `remaining` copies of `byte`, then EOF — never materialises the body in a `Vec`.
struct CountingReader {
    remaining: u64,
    byte: u8,
}

impl std::io::Read for CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let n = std::cmp::min(buf.len() as u64, self.remaining) as usize;
        buf[..n].fill(self.byte);
        self.remaining -= n as u64;
        Ok(n)
    }
}

struct StreamingByteClient {
    body_size: u64,
}

#[async_trait::async_trait]
impl HttpClient for StreamingByteClient {
    async fn send(&self, _req: &HttpRequest) -> Result<HttpResponse, HttpError> {
        panic!("download must NOT fall back to send")
    }

    async fn download(
        &self,
        req: &HttpRequest,
        writer: &mut (dyn std::io::Write + Send),
    ) -> Result<DownloadMeta, HttpError> {
        let reader = CountingReader {
            remaining: self.body_size,
            byte: b'x',
        };
        let bytes_written =
            stream_to_writer(reader, writer, Decompression::None, req.max_response_size)?;
        Ok(DownloadMeta {
            status: 200,
            status_text: "OK".into(),
            headers: vec![("content-type".into(), "application/octet-stream".into())],
            final_url: req.url.clone(),
            bytes_written,
        })
    }
}

/// Body flows host-side (download → stream_to_writer → disk), not through the Wasm GC heap.
/// Regression to buffering would trip the cap and `panic!` from `StreamingByteClient::send`.
#[test]
fn download_does_not_count_against_gc_heap_cap() {
    // 256 KB: enough for DownloadResult + working set, well under the body.
    const CAP_BYTES: u64 = 256 * 1024;
    // 4 MB body — 16x the cap; any byte routed to GC heap trips the limiter.
    const BODY_BYTES: u64 = 4 * 1024 * 1024;

    let src = r#"
import { download, DownloadResult } from "submilli:http";
function main(): void {
  const r: DownloadResult = download(
    "https://example.test/big.bin",
    "/big.bin",
    { maxBytes: 8388608 }
  );
  assert(r.status === 200, "status 200");
  assert(r.bytesWritten === 4194304, "4 MB written");
  assert(r.path === "/big.bin", "path echoed");
}
"#;

    let compiled = compile_script(src, "memory_caps.subm", interpreter::FileId(0), &[], &[])
        .expect("compile clean");
    let cfg = RuntimeConfig {
        max_store_bytes: CAP_BYTES,
        ..RuntimeConfig::default()
    };
    let engine = cfg.engine().expect("engine");
    let tmp = tempfile::tempdir().expect("tempdir");
    let vfs = Vfs::external(tmp.path().to_path_buf()).expect("external vfs");
    let mut data = StoreData::with_vfs_and_cap(vfs, CAP_BYTES);
    data.http_client = Arc::new(StreamingByteClient {
        body_size: BODY_BYTES,
    });
    let mut store = cfg.store(&engine, data).expect("store");
    install_tenant_limits(&mut store);
    let module = Module::new(&engine, &compiled.wasm).expect("module");
    let mut linker = Linker::<StoreData>::new(&engine);
    let instance = pollster::block_on(async {
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install runtime");
        linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate")
    });
    pollster::block_on(dispatch_main_async(&mut store, &instance)).expect("main ran without OOM");

    let written = std::fs::metadata(tmp.path().join("big.bin"))
        .expect("file written")
        .len();
    assert_eq!(written, BODY_BYTES, "on-disk file size");

    let observed = store.data().tenant_limits.observed_bytes();
    eprintln!(
        "download_does_not_count_against_gc_heap_cap: \
         body={BODY_BYTES} bytes, observed GC growth={observed} bytes ({}x body)",
        observed as f64 / BODY_BYTES as f64,
    );
    assert!(
        observed < BODY_BYTES,
        "observed GC growth {observed} ≥ body size {BODY_BYTES} — \
         download bytes appear to be flowing through the GC heap"
    );
}

/// `submilli:regex.compile` charges `estimate_regex_bytes()` against `TenantLimits.host_attached_bytes` per compile; exceeding the cap traps.
#[test]
fn regex_host_bytes_cap_traps_on_excess_compiles() {
    // Reserve room for runtime GC; 10 regexes then exceed the remaining host budget.
    let src = r#"
function main(): void {
  let i: number = 0;
  while (i < 10) {
    const re: RegExp = new RegExp("abc" + i.toString(), "");
    assert(re.test("abc" + i.toString()), "should match");
    i = i + 1;
  }
}
"#;

    let err = cap_run(src, 128 * 1024).expect_err("cap should reject compile");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("memory cap exceeded") && msg.contains("host bytes"),
        "expected host allocation cap failure, got: {msg}"
    );
}

/// Each `submilli:fs` reader charges its in-process buffer against
/// `TenantLimits.host_attached_bytes` and refunds it on close. Opening + closing many
/// readers in sequence keeps only one charged at a time, so a tight cap that the
/// accumulated total would blow is never tripped — the old leak-until-`Store::drop`
/// table could not make this guarantee.
#[test]
fn fs_handles_recovered_under_tight_cap() {
    let src = r#"
import { writeText, lines } from "submilli:fs";
function main(): void {
  writeText("/data.txt", "alpha\nbeta\ngamma\n");
  let total: number = 0;
  let round: number = 0;
  while (round < 500) {
    for (const line of lines("/data.txt")) {
      total = total + line.length;
    }
    round = round + 1;
  }
  assert(total === 7000, "counted every line across all rounds");
}
"#;

    // 500 readers × 8 KB ≈ 4 MB if leaked; one-at-a-time it never exceeds the 2 MB cap.
    cap_run(src, 2 * 1024 * 1024).expect("sequential open/close stays under the cap");
}

/// The flip side: readers held open (never closed, kept reachable) keep their bytes
/// charged, so enough of them trip the cap — proving the charge is real, not free.
#[test]
fn fs_handles_held_open_trip_the_cap() {
    let src = r#"
import { lines, writeText } from "submilli:fs";
function main(): void {
  writeText("/data.txt", "alpha\nbeta\ngamma\n");
  const held: Iterator<string>[] = [];
  let i: number = 0;
  while (i < 500) {
    held.push(lines("/data.txt"));
    i = i + 1;
  }
  assert(held.length === 500, "kept every reader open");
}
"#;

    let err = cap_run(src, 1024 * 1024).expect_err("held-open readers should trip the cap");
    let msg = format!("{err:#}");
    assert!(
        msg.to_lowercase().contains("memory") || msg.to_lowercase().contains("cap"),
        "expected memory/cap language in trap, got: {msg}"
    );
}

/// A download stops at the VFS's `size_limit`: the program can catch the refusal,
/// the partial file is gone, and the space it held is free again.
#[test]
fn download_stops_at_the_vfs_size_limit() {
    const LIMIT: u64 = 1024 * 1024;
    let src = r#"
import { download } from "submilli:http";
import { exists, writeText } from "submilli:fs";
function main(): void {
  let refused = false;
  try {
    download("https://example.test/big.bin", "/big.bin", { maxBytes: 8388608 });
  } catch (e) {
    refused = e instanceof QuotaExceededError && e instanceof Error && !((e as unknown) instanceof RangeError);
  }
  assert(refused, "a 4 MB download under a 1 MB limit is refused");
  assert(!exists("/big.bin"), "nothing is left behind");
  writeText("/after.txt", "x".repeat(1000000));
}
"#;

    let compiled = compile_script(src, "size_limit.subm", interpreter::FileId(0), &[], &[])
        .expect("compile clean");
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine().expect("engine");
    let vfs = Vfs::tempdir().expect("tempdir").with_size_limit(LIMIT);
    let mut data = StoreData::with_vfs(vfs);
    data.http_client = Arc::new(StreamingByteClient {
        body_size: 4 * 1024 * 1024,
    });
    let mut store = cfg.store(&engine, data).expect("store");
    let module = Module::new(&engine, &compiled.wasm).expect("module");
    let mut linker = Linker::<StoreData>::new(&engine);
    let instance = pollster::block_on(async {
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install runtime");
        linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate")
    });
    pollster::block_on(dispatch_main_async(&mut store, &instance)).expect("main asserts hold");
}

/// A download that overwrites a larger file is counted by its growth over it, and
/// frees the difference, so the space is there for the next write.
#[test]
fn a_download_overwrite_is_counted_by_its_growth() {
    const LIMIT: u64 = 1024 * 1024;
    let src = r#"
import { download } from "submilli:http";
import { writeText } from "submilli:fs";
function main(): void {
  download("https://example.test/big.bin", "/big.bin", { maxBytes: 8388608, overwrite: true });
  writeText("/after.txt", "x".repeat(500000));
}
"#;

    let compiled = compile_script(src, "overwrite.subm", interpreter::FileId(0), &[], &[])
        .expect("compile clean");
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine().expect("engine");
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("big.bin"), vec![b'o'; 900 * 1024]).expect("seed");
    let vfs = Vfs::external(dir.path().to_path_buf())
        .expect("external")
        .with_size_limit(LIMIT);
    let mut data = StoreData::with_vfs(vfs);
    data.http_client = Arc::new(StreamingByteClient {
        body_size: 400 * 1024,
    });
    let mut store = cfg.store(&engine, data).expect("store");
    let module = Module::new(&engine, &compiled.wasm).expect("module");
    let mut linker = Linker::<StoreData>::new(&engine);
    let instance = pollster::block_on(async {
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install runtime");
        linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate")
    });
    pollster::block_on(dispatch_main_async(&mut store, &instance))
        .expect("the overwrite freed 500 KB for the next write");
}
