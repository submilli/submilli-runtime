//! Tests that streaming APIs keep host-side memory bounded. `TenantLimits`
//! only tracks Wasm GC heap, so a process-wide `CountingAllocator` measures
//! host allocations independently.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use submilli_engine::{
    compile_script, dispatch_main_async,
    runtime::{RuntimeConfig, StoreData, Vfs, install_runtime_async, install_tenant_limits},
    stdlib::http::transport::{
        Decompression, DownloadMeta, HttpClient, HttpError, HttpRequest, HttpResponse,
        stream_to_writer,
    },
};
use wasmtime::{Linker, Module};

const TEST_MEMORY_RESERVATION: u64 = 128 * 1024;
const HOST_MEMORY_THRESHOLD: usize = 256 * 1024;

struct CountingAllocator;

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn bump_peak(new: usize) {
    let mut peak = PEAK.load(Ordering::Relaxed);
    while peak < new {
        match PEAK.compare_exchange_weak(peak, new, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(actual) => peak = actual,
        }
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let new = ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            bump_peak(new);
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
            let new = ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            bump_peak(new);
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            let old_size = layout.size();
            if new_size > old_size {
                let delta = new_size - old_size;
                let new = ALLOCATED.fetch_add(delta, Ordering::Relaxed) + delta;
                bump_peak(new);
            } else {
                ALLOCATED.fetch_sub(old_size - new_size, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static A: CountingAllocator = CountingAllocator;

fn reset_peak_to_current() {
    PEAK.store(ALLOCATED.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// Serialise tests so the global counters aren't corrupted by other
/// tests running in parallel under cargo test's default scheduler.
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Emits a single fixed byte `remaining` times; produces large bodies without materialising a `Vec`.
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

/// Setup (compile/instantiate) runs before measurement; peak delta is taken across `dispatch_main`.
fn measure_scenario(
    src: &str,
    seed: impl FnOnce(&std::path::Path),
    install_client: impl FnOnce(&mut StoreData),
) -> (usize, tempfile::TempDir) {
    let compiled = compile_script(
        src,
        "host_memory.subm",
        submilli_engine::FileId(0),
        &[],
        &[],
    )
    .expect("compile");
    let cfg = RuntimeConfig {
        memory_reservation: TEST_MEMORY_RESERVATION,
        ..RuntimeConfig::default()
    };
    let engine = cfg.engine().expect("engine");
    let tmp = tempfile::tempdir().expect("tempdir");
    seed(tmp.path());
    let vfs = Vfs::external(tmp.path().to_path_buf()).expect("external vfs");
    let mut data = StoreData::with_vfs(vfs);
    install_client(&mut data);
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

    let baseline = ALLOCATED.load(Ordering::Relaxed);
    reset_peak_to_current();

    pollster::block_on(dispatch_main_async(&mut store, &instance)).expect("main ran");

    let post_peak = PEAK.load(Ordering::Relaxed);
    let delta = post_peak.saturating_sub(baseline);
    (delta, tmp)
}

#[test]
fn download_host_memory_stays_bounded() {
    const BODY_BYTES: u64 = 4 * 1024 * 1024;
    if !nightly_only_requested() {
        eprintln!("host memory: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
        return;
    }
    let _guard = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let src = r#"
import { download, DownloadResult } from "submilli:http";
function main(): void {
  const r: DownloadResult = download(
    "https://example.test/big.bin",
    "/big.bin",
    { maxBytes: 8388608 }
  );
  assert(r.bytesWritten === 4194304, "4 MB written");
}
"#;

    let (delta, tmp) = measure_scenario(
        src,
        |_root| {},
        |data| {
            data.http_client = Arc::new(StreamingByteClient {
                body_size: BODY_BYTES,
            });
        },
    );

    let written = std::fs::metadata(tmp.path().join("big.bin"))
        .expect("file written")
        .len();
    assert_eq!(written, BODY_BYTES, "on-disk file size");
    report("http.download", delta, BODY_BYTES);
    assert!(
        delta < HOST_MEMORY_THRESHOLD,
        "http.download: host-side peak delta {delta} bytes for {BODY_BYTES}-byte body \
         exceeded threshold {HOST_MEMORY_THRESHOLD} — bytes appear to be buffering",
    );
}

#[test]
fn fs_lines_host_memory_stays_bounded() {
    const FILE_BYTES: u64 = 4 * 1024 * 1024;
    const LINE_LEN: usize = 200;
    if !nightly_only_requested() {
        eprintln!("host memory: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
        return;
    }
    let _guard = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let src = r#"
import { lines } from "submilli:fs";
function main(): void {
  let count: number = 0;
  let total: number = 0;
  for (const line of lines("/big.txt")) {
    count = count + 1;
    total = total + line.length;
  }
  assert(count > 0, "saw lines");
  assert(total > 0, "summed lengths");
}
"#;

    let (delta, _tmp) = measure_scenario(
        src,
        |root| {
            // Write the seed file directly from the host. `fs.write`
            // from inside the program would itself buffer the bytes
            // and pollute the measurement.
            let mut body = Vec::with_capacity(FILE_BYTES as usize);
            let line: Vec<u8> = std::iter::repeat_n(b'a', LINE_LEN - 1)
                .chain(std::iter::once(b'\n'))
                .collect();
            while body.len() < FILE_BYTES as usize {
                body.extend_from_slice(&line);
            }
            body.truncate(FILE_BYTES as usize);
            std::fs::write(root.join("big.txt"), body).expect("seed file");
        },
        |_data| {},
    );

    report("fs.lines", delta, FILE_BYTES);
    assert!(
        delta < HOST_MEMORY_THRESHOLD,
        "fs.lines: host-side peak delta {delta} bytes for {FILE_BYTES}-byte file \
         exceeded threshold {HOST_MEMORY_THRESHOLD} — file appears to be buffering",
    );
}

#[test]
fn fs_bytes_host_memory_stays_bounded() {
    const FILE_BYTES: u64 = 4 * 1024 * 1024;
    if !nightly_only_requested() {
        eprintln!("host memory: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
        return;
    }
    let _guard = TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let src = r#"
import { bytes } from "submilli:fs";
function main(): void {
  let chunks: number = 0;
  let total: number = 0;
  for (const chunk of bytes("/big.bin", 8192)) {
    chunks = chunks + 1;
    total = total + chunk.length;
  }
  assert(chunks > 0, "saw chunks");
  assert(total === 4194304, "summed all bytes");
}
"#;

    let (delta, _tmp) = measure_scenario(
        src,
        |root| {
            let body = vec![b'x'; FILE_BYTES as usize];
            std::fs::write(root.join("big.bin"), body).expect("seed file");
        },
        |_data| {},
    );

    report("fs.bytes", delta, FILE_BYTES);
    assert!(
        delta < HOST_MEMORY_THRESHOLD,
        "fs.bytes: host-side peak delta {delta} bytes for {FILE_BYTES}-byte file \
         exceeded threshold {HOST_MEMORY_THRESHOLD} — file appears to be buffering",
    );
}

fn report(api: &str, delta: usize, body_bytes: u64) {
    eprintln!(
        "host_memory[{api}]: peak delta={delta} bytes, body={body_bytes} bytes ({:.4}% of body)",
        delta as f64 / body_bytes as f64 * 100.0,
    );
}

fn nightly_only_requested() -> bool {
    std::env::var("SUBMILLI_TEST_NIGHTLY_ONLY").is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[test]
fn rejected_string_output_does_not_allocate_its_buffer() {
    use submilli_engine::runtime::limits::{MemoryCapExceeded, TenantLimits};
    use submilli_engine::runtime::prelude::string::{Str, concat, pad_end, pad_start, repeat};

    if !nightly_only_requested() {
        eprintln!("host memory: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
        return;
    }
    // A prior panic may have interrupted measurements protected by this lock.
    let _guard = TEST_LOCK
        .lock()
        .expect("host memory measurement lock poisoned");

    let input = Str::from_units(vec![0xD800; 512 * 1024]);
    let pad = Str::from_units(vec![0xDC00]);
    let limits = TenantLimits::new(16 * 1024);
    for operation in 0..4 {
        let baseline = ALLOCATED.load(Ordering::Relaxed);
        reset_peak_to_current();
        let result = match operation {
            0 => repeat(&input, 2.0, &limits),
            1 => pad_start(&input, 1048576.0, &pad, &limits),
            2 => pad_end(&input, 1048576.0, &pad, &limits),
            _ => concat(&input, &input, &limits),
        };
        let delta = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
        let err = result.err().expect("output exceeds the tenant cap");
        assert!(err.is::<MemoryCapExceeded>(), "{err:#}");
        assert!(
            delta < HOST_MEMORY_THRESHOLD,
            "operation {operation} allocated {delta} bytes despite admission refusal"
        );
        assert_eq!(limits.host_attached_bytes(), 0);
        assert_eq!(limits.peak_bytes(), 0);
    }
}
