//! Measures a new Submilli sandbox up to the first instruction in its program
//! module. For a short check: `cargo run --release --locked -p submilli
//! --example bench_sandbox_start -- warm ephemeral 1 20`.
//!
//! The first output line is deliberately emitted before argument parsing or any
//! runtime preparation. A coordinating process can therefore measure process
//! launch through `ready` independently from the samples printed at the end.
//!
//! Each sample creates a new VFS, `StoreData`, store, resource limiter, full
//! runtime linker, and a real blueprint-backed `PolicyCheck`. The precompiled
//! program module has a start function whose first instruction calls the
//! benchmark marker import. The marker records the elapsed monotonic time and
//! proves that the program module reached that instruction exactly once.
//!
//! `Module::new` happens once per process before any sample in both modes, so
//! samples exclude engine creation and program-module preparation. The runtime has
//! no prelude Wasm module: its prelude is installed as Rust host functions.
//! The `cold` label means the driver starts a fresh process with no warmups;
//! it is the first sandbox after that process has prepared its engine and module.
//! Teardown is after the marker and outside the measured interval. The marker
//! includes the dispatch of that first `call` and entry into its host function;
//! this is a conservative upper bound, not an instruction-fetch timestamp.
//! The worker's native thread stack and async executor are process infrastructure
//! prepared once. Per-store interpreter stack/memory allocation is timed.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use interpreter::RuntimeConfig;
use interpreter::runtime::{
    StoreData, Vfs, install_runtime_async, install_tenant_limits, instantiate_program_async,
};
use submilli_shared::PolicyCheck;
use wasmtime::{Linker, Module};

/// A minimal valid Wasm module with one page of memory and a start function.
/// Its function body is exactly `call $benchmark_marker; end`, so the marker is
/// the first program-module instruction. The import exists only in this
/// benchmark linker, in a namespace separate from production imports.
const MARKER_MODULE: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // header
    0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type: () -> ()
    0x02, 0x1b, 0x01, 0x12, b's', b'u', b'b', b'm', b'i', b'l', b'l', b'i', b':', b'b', b'e', b'n',
    b'c', b'h', b'm', b'a', b'r', b'k', 0x04, b'm', b'a', b'r', b'k', 0x00,
    0x00, // import function 0
    0x03, 0x02, 0x01, 0x00, // function 1 has type 0
    0x05, 0x03, 0x01, 0x00, 0x01, // one minimum-size memory
    0x08, 0x01, 0x01, // start function 1
    0x0a, 0x06, 0x01, 0x04, 0x00, 0x10, 0x00, 0x0b, // body: call 0; end
];

const UNRECORDED: u64 = u64::MAX;

#[derive(Clone, Copy)]
enum VfsChoice {
    Ephemeral,
    None,
}

impl VfsChoice {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "ephemeral" => Ok(Self::Ephemeral),
            "none" => Ok(Self::None),
            _ => bail!("VFS must be `ephemeral` or `none`, got `{value}`"),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Ephemeral => "ephemeral",
            Self::None => "none",
        }
    }

    fn create(self) -> io::Result<Vfs> {
        match self {
            Self::Ephemeral => Vfs::tempdir(),
            Self::None => Ok(Vfs::none()),
        }
    }
}

struct Options {
    mode: String,
    vfs: VfsChoice,
    warmups: usize,
    samples: usize,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let mode = args.next().context("missing mode (`warm` or `cold`)")?;
        if !matches!(mode.as_str(), "warm" | "cold") {
            bail!("mode must be `warm` or `cold`, got `{mode}`");
        }
        let vfs = VfsChoice::parse(&args.next().context("missing VFS mode")?)?;
        let warmups = parse_count(
            &args.next().context("missing warmup count")?,
            "warmup count",
        )?;
        let samples = parse_count(
            &args.next().context("missing sample count")?,
            "sample count",
        )?;
        if args.next().is_some() {
            bail!("usage: bench_sandbox_start <warm|cold> <ephemeral|none> <warmups> <samples>");
        }
        if samples == 0 {
            bail!("sample count must be greater than zero");
        }
        if mode == "cold" && (warmups != 0 || samples != 1) {
            bail!("cold requires zero warmups and exactly one sample per process");
        }
        Ok(Self {
            mode,
            vfs,
            warmups,
            samples,
        })
    }
}

struct Marker {
    started: Instant,
    elapsed_ns: AtomicU64,
    calls: AtomicUsize,
}

impl Marker {
    fn new(started: Instant) -> Self {
        Self {
            started,
            elapsed_ns: AtomicU64::new(UNRECORDED),
            calls: AtomicUsize::new(0),
        }
    }

    fn mark(&self) {
        let elapsed = self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.elapsed_ns.store(elapsed, Ordering::Release);
    }

    fn elapsed_ns(&self) -> Result<u64> {
        let calls = self.calls.load(Ordering::Acquire);
        if calls != 1 {
            bail!("benchmark marker was called {calls} times, expected exactly once");
        }
        let elapsed = self.elapsed_ns.load(Ordering::Acquire);
        if elapsed == UNRECORDED {
            bail!("benchmark marker did not record an elapsed time");
        }
        Ok(elapsed)
    }
}

fn main() -> Result<()> {
    // Keep this as main's first action: the driver measures process launch to
    // this flush separately from runtime construction and samples.
    let mut stdout = io::stdout().lock();
    stdout.write_all(b"{\"event\":\"ready\"}\n")?;
    stdout.flush()?;
    drop(stdout);

    let options = Options::parse()?;
    let config = RuntimeConfig::default();
    let stack_size = config.native_stack_size();
    let result = std::thread::Builder::new()
        .name("sandbox-start-benchmark".into())
        .stack_size(stack_size)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .context("creating benchmark async runtime")?;
            runtime.block_on(run(options, config))
        })
        .context("creating benchmark worker thread")?
        .join()
        .map_err(|_| anyhow::anyhow!("benchmark worker thread panicked"))??;

    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

async fn run(options: Options, config: RuntimeConfig) -> Result<serde_json::Value> {
    let engine = config
        .engine_async()
        .map_err(anyhow::Error::from)
        .context("creating runtime engine")?;
    // This is intentionally before all samples. It gives both modes a compiled
    // program module while retaining per-sample instantiation and start work.
    let module = Module::new(&engine, MARKER_MODULE)
        .map_err(anyhow::Error::from)
        .context("preparing marker module")?;
    let blueprint = Arc::new(
        submilli_blueprint::parse(&format!(
            "name: sandbox-start\nvfs:\n  mode: {}\n",
            options.vfs.name()
        ))
        .context("parsing benchmark blueprint")?,
    );
    let variables: Arc<BTreeMap<String, String>> = Arc::default();

    for _ in 0..options.warmups {
        measure_start(
            &config,
            &engine,
            &module,
            options.vfs,
            &blueprint,
            &variables,
        )
        .await?;
    }

    let mut samples_ns = Vec::new();
    samples_ns
        .try_reserve_exact(options.samples)
        .context("reserving sample storage")?;
    for _ in 0..options.samples {
        samples_ns.push(
            measure_start(
                &config,
                &engine,
                &module,
                options.vfs,
                &blueprint,
                &variables,
            )
            .await?,
        );
    }

    Ok(serde_json::json!({
        "mode": options.mode,
        "vfs": options.vfs.name(),
        "warmups": options.warmups,
        "samples_ns": samples_ns,
        "measurement": "new VFS/store/runtime linker through host entry from first program-module Wasm instruction",
        "module_prepared_before_measurement": true,
        "program_marker_first_instruction": true,
        "native_thread_stack_bytes": config.native_stack_size(),
        "max_wasm_stack_bytes": config.max_wasm_stack,
        "max_store_bytes": config.max_store_bytes,
        "fuel": config.fuel,
        "async_yield_fuel": config.async_yield_fuel,
        "guest_linear_memory_bytes": 65536,
        // Probe after all samples so this cannot warm up a cold start. This is
        // observed clock-read granularity/overhead, not a calibrated resolution.
        "instant_probe_min_nonzero_ns": (0..1000)
            .map(|_| Instant::now().elapsed().as_nanos())
            .filter(|&ns| ns > 0)
            .min(),
    }))
}

async fn measure_start(
    config: &RuntimeConfig,
    engine: &wasmtime::Engine,
    module: &Module,
    vfs_choice: VfsChoice,
    blueprint: &Arc<submilli_blueprint::Blueprint>,
    variables: &Arc<BTreeMap<String, String>>,
) -> Result<u64> {
    let started = Instant::now();
    let marker = Arc::new(Marker::new(started));
    let vfs = vfs_choice.create().context("creating benchmark VFS")?;
    let mut data = StoreData::with_vfs_and_cap(
        interpreter::runtime::durable::RunId::generate().context("creating execution run ID")?,
        vfs,
        config.max_store_bytes,
    );
    data.variables = Arc::clone(variables);
    data.security_check = Arc::new(PolicyCheck::with_variables(
        Arc::clone(blueprint),
        Arc::clone(variables),
    ));
    let mut store = config
        .store_async(engine, data)
        .map_err(anyhow::Error::from)
        .context("creating execution store")?;
    install_tenant_limits(&mut store);
    let mut linker = Linker::<StoreData>::new(engine);
    install_runtime_async(&mut linker, &mut store)
        .await
        .map_err(anyhow::Error::from)
        .context("installing runtime imports")?;
    let marker_for_import = Arc::clone(&marker);
    linker
        .func_wrap("submilli:benchmark", "mark", move || {
            marker_for_import.mark();
        })
        .map_err(anyhow::Error::from)
        .context("installing first-instruction marker")?;

    let _instance = instantiate_program_async(&linker, &mut store, module)
        .await
        .map_err(anyhow::Error::from)
        .context("instantiating marker program")?;
    marker.elapsed_ns()
}

fn parse_count(value: &str, name: &str) -> Result<usize> {
    value
        .parse()
        .with_context(|| format!("{name} must be a non-negative integer, got `{value}`"))
}
