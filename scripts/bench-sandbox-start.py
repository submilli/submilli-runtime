#!/usr/bin/env python3
"""Measure sandbox start with the production runtime's release example (stdlib only).

From the repository root:
  cargo build --release --locked -p submilli --bin submilli --example bench_sandbox_start
  python3 scripts/bench-sandbox-start.py

Defaults: 3 repetitions, each with 1,000 warmups + 10,000 warm starts in one
process and 200 fresh processes with one cold start each, for ephemeral and none.
Also measures 50 fresh `submilli run --report` invocations of `return 0`.
Use --smoke for protocol checks. JSON (including raw samples) goes to /tmp and
Markdown tables to stdout. --host-info accepts a JSON object of externally read
cpu_brand, cpu_count, memory_bytes, sw_vers, kernel_release and source when host
probes are restricted; those supplied fields are explicitly marked as external.

Sandbox timing is Rust Instant inside the example, from before VFS/store/policy/
limits/import setup to host entry from the guest's first Wasm instruction. Engine
and module preparation, worker thread creation, and teardown are outside it.
Cold performs no hidden warmup; warm reuses the engine and compiled marker module.
The marker adds dispatch/host-entry overhead. This exercises production runtime
functions, not the HTTP server request pipeline or the TypeScript compiler.
"""

import argparse
import datetime as dt
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import queue
import re
import statistics
import subprocess
import sys
import tempfile
import threading
import time


READY = {"event": "ready"}
REPORT = re.compile(r"wall: (\d+) ms \(compile (\d+) ms, run (\d+) ms\)")
BUILD_COMMAND = "cargo build --release --locked -p submilli --bin submilli --example bench_sandbox_start"
SOURCE_PATHS = ("crates/submilli/examples/bench_sandbox_start.rs",
                "scripts/bench-sandbox-start.py", "scripts/tests/test_bench_sandbox_start.py")


class BenchmarkFailure(RuntimeError):
    pass


def command_record(command):
    """Keep failures in the metadata rather than guessing unavailable host facts."""
    try:
        completed = subprocess.run(command, text=True, capture_output=True, check=False, timeout=30)
        return {"command": command, "returncode": completed.returncode,
                "stdout": completed.stdout.strip(), "stderr": completed.stderr.strip()}
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"command": command, "returncode": None, "stdout": "", "stderr": str(error)}


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def nearest_rank(values, percentile):
    """One-indexed ceil(p*n/100); no interpolation."""
    if not values or not 0 < percentile <= 100:
        raise ValueError("percentile needs samples and a percentile in (0, 100]")
    return sorted(values)[math.ceil(len(values) * percentile / 100) - 1]


def summary_ns(samples, *, allow_zero=False):
    minimum = 0 if allow_zero else 1
    if not samples or any(type(item) is not int or item < minimum for item in samples):
        raise BenchmarkFailure("invalid nanosecond samples (expected non-empty integer values)")
    return {"count": len(samples), "p50_ns": nearest_rank(samples, 50),
            "p95_ns": nearest_rank(samples, 95), "p99_ns": nearest_rank(samples, 99),
            "max_ns": max(samples)}


def median_summary(repetitions):
    return {key: statistics.median([item[key] for item in repetitions])
            for key in ("p50_ns", "p95_ns", "p99_ns", "max_ns")}


def process_lines(stream, output):
    for line in stream:
        output.put((line.rstrip("\n"), time.perf_counter_ns()))
    output.put((None, time.perf_counter_ns()))


def run_worker(binary, mode, vfs, warmups, samples, timeout, env):
    """Time parent launch through observed READY separately; bound child lifetime."""
    # A file avoids stderr pipe backpressure if a failing child emits diagnostics.
    with tempfile.TemporaryFile(mode="w+") as stderr:
        started = time.perf_counter_ns()
        process = subprocess.Popen(
            [str(binary), mode, vfs, str(warmups), str(samples)],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=stderr,
            text=True, bufsize=1, env=env,
        )
        lines = queue.Queue()
        reader = threading.Thread(target=process_lines, args=(process.stdout, lines), daemon=True)
        reader.start()
        deadline = time.monotonic() + timeout
        payload = None
        boot_ns = None
        try:
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise BenchmarkFailure("worker timed out")
                try:
                    value, observed = lines.get(timeout=remaining)
                except queue.Empty as error:
                    raise BenchmarkFailure("worker timed out") from error
                if value is None:
                    break
                try:
                    candidate = json.loads(value)
                except json.JSONDecodeError as error:
                    raise BenchmarkFailure("worker emitted non-JSON") from error
                if boot_ns is None:
                    if candidate != READY:
                        raise BenchmarkFailure("worker's first stdout line must be READY")
                    boot_ns = observed - started
                elif payload is not None:
                    raise BenchmarkFailure("worker emitted more than one result JSON object")
                elif not isinstance(candidate, dict):
                    raise BenchmarkFailure("worker result must be a JSON object")
                else:
                    payload = candidate
            process.wait(timeout=max(0.001, deadline - time.monotonic()))
        finally:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=5)
            reader.join(timeout=5)
            process.stdout.close()
        stderr.seek(0)
        error_text = stderr.read().strip()
    if process.returncode != 0:
        raise BenchmarkFailure("worker failed: " + error_text)
    if boot_ns is None or not isinstance(payload, dict):
        raise BenchmarkFailure("worker exited without READY and a result object: " + error_text)
    if "error" in payload or "errors" in payload:
        raise BenchmarkFailure("worker reported an error result")
    if payload.get("mode") != mode or payload.get("vfs") != vfs or payload.get("warmups") != warmups:
        raise BenchmarkFailure("worker result mode/vfs/warmups does not match invocation")
    if not isinstance(payload.get("measurement"), str) or not payload["measurement"]:
        raise BenchmarkFailure("worker is missing its measurement description")
    if payload.get("module_prepared_before_measurement") is not True or payload.get("program_marker_first_instruction") is not True:
        raise BenchmarkFailure("worker did not confirm the marker/module timing contract")
    values = payload.get("samples_ns")
    if not isinstance(values, list) or len(values) != samples:
        raise BenchmarkFailure(f"worker did not return exactly {samples} samples")
    summary_ns(values)
    return {"boot_ns": boot_ns, "result": payload, "stderr": error_text}


def run_cli(binary, script, timeout, env):
    started = time.perf_counter_ns()
    completed = subprocess.run([str(binary), "run", "--report", str(script)], env=env,
                               capture_output=True, text=True, timeout=timeout, check=False)
    elapsed = time.perf_counter_ns() - started
    if completed.returncode != 0 or completed.stdout.strip() != "0":
        raise BenchmarkFailure("CLI failed or returned an unexpected result: " + completed.stderr.strip())
    match = REPORT.search(completed.stderr)
    if match is None:
        raise BenchmarkFailure("CLI --report did not emit wall/compile/run timings")
    wall_ms, compile_ms, run_ms = (int(value) for value in match.groups())
    return {"process_total_ns": elapsed, "report_wall_ms": wall_ms,
            "report_compile_ms": compile_ms, "report_run_ms": run_ms,
            "stdout": completed.stdout.strip(), "stderr": completed.stderr.strip()}


def machine_state():
    return {"at": dt.datetime.now(dt.timezone.utc).isoformat(),
            "load_average_1_5_15": list(os.getloadavg()),
            "power": command_record(["pmset", "-g", "batt"]),
            "thermal": command_record(["pmset", "-g", "therm"])}


def host_metadata(args, binaries):
    commands = {}

    def record(name, command):
        commands[name] = command_record(command)
        return commands[name]["stdout"] if commands[name]["returncode"] == 0 else None

    external = json.loads(args.host_info.read_text()) if args.host_info else {}
    if not isinstance(external, dict):
        raise BenchmarkFailure("--host-info must contain a JSON object")
    probes = {"cpu_brand": ["sysctl", "-n", "machdep.cpu.brand_string"],
              "cpu_count": ["sysctl", "-n", "hw.ncpu"],
              "memory_bytes": ["sysctl", "-n", "hw.memsize"],
              "sw_vers": ["sw_vers"], "kernel_release": ["uname", "-r"]}
    hardware = {key: external[key] if key in external else record(key, command)
                for key, command in probes.items()}
    git_status = record("git_status", ["git", "--no-optional-locks", "status", "--porcelain"])
    git_sha = record("git_sha", ["git", "rev-parse", "HEAD"])
    return {
        "recorded_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "build_command": args.build_command,
        "invocation": [sys.executable, *sys.argv],
        "git_sha": git_sha, "harness_commit_sha": git_sha if git_status == "" else None,
        "git_base": record("git_base", ["git", "merge-base", "HEAD", "origin/main"]),
        "git_describe": record("git_describe", ["git", "describe", "--always", "--tags"]),
        "git_dirty": None if git_status is None else bool(git_status),
        "cargo_version": re.search(r'^version = "([^"]+)"', Path("Cargo.toml").read_text(), re.M).group(1),
        "cli_version": record("cli_version", [str(binaries[1]), "--version"]),
        "rustc": record("rustc", ["rustc", "-Vv"]),
        "python": sys.version, "machine_architecture": platform.machine(),
        "hardware": hardware, "externally_supplied_host_info": external,
        "host_command_records": commands,
        "temp_directory": tempfile.gettempdir(),
        "python_clock": vars(time.get_clock_info("perf_counter")),
        "source_sha256": {path: sha256(Path(path)) for path in SOURCE_PATHS},
        "binary_sha256": {str(path): sha256(path) for path in binaries},
    }


def markdown_table(title, rows):
    print(f"\n### {title}\n")
    print("| mode | repetition | count | p50 (ms) | p95 (ms) | p99 (ms) | max (ms) |")
    print("| --- | ---: | ---: | ---: | ---: | ---: | ---: |")
    for mode, repetition, values in rows:
        print("| {} | {} | {} | {:.3f} | {:.3f} | {:.3f} | {:.3f} |".format(
            mode, repetition, values.get("count", "—"), values["p50_ns"] / 1e6,
            values["p95_ns"] / 1e6, values["p99_ns"] / 1e6, values["max_ns"] / 1e6))


def print_tables(results):
    for key, title in (("sandbox", "Sandbox internal start"), ("process_boot", "Process launch to READY (separate)")):
        rows = []
        for name, data in results[key].items():
            for number, repetition in enumerate(data["repetitions"], 1):
                rows.append((name, number, repetition["summary"]))
            rows.append((name, "median", data["median_of_repetition_quantiles"]))
        markdown_table(title, rows)
    cli = results["cli_first_run"]
    markdown_table("CLI first-run (fresh processes, ephemeral VFS)", [
        (label, "fresh", cli[key]) for label, key in (
            ("process total", "process_total"), ("--report wall", "report_wall"),
            ("--report compile bucket", "report_compile"), ("--report run", "report_run"))])


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--worker-binary", type=Path, default=Path("target/release/examples/bench_sandbox_start"))
    parser.add_argument("--cli-binary", type=Path, default=Path("target/release/submilli"))
    parser.add_argument("--warmups", type=int, default=1000)
    parser.add_argument("--warm-samples", type=int, default=10000)
    parser.add_argument("--cold-processes", type=int, default=200)
    parser.add_argument("--cli-processes", type=int, default=50)
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=300.0, help="Seconds allowed per child process.")
    parser.add_argument("--build-command", default=BUILD_COMMAND)
    parser.add_argument("--host-info", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--smoke", action="store_true", help="Use short counts to verify the protocol.")
    args = parser.parse_args()
    if min(args.warmups, args.warm_samples, args.cold_processes, args.cli_processes, args.repetitions) < 1 or not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("all counts and timeout must be positive and finite")
    if args.smoke:
        args.warmups, args.warm_samples, args.cold_processes, args.cli_processes, args.repetitions = 2, 4, 2, 2, 1
    worker = args.worker_binary.resolve(strict=True)
    cli = args.cli_binary.resolve(strict=True)
    print("benchmark status: collecting metadata", file=sys.stderr, flush=True)
    metadata = host_metadata(args, [worker, cli])
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    path = args.output or Path("/tmp") / f"submilli-bench-sandbox-start-{metadata['git_sha'] or 'unknown'}-{stamp}.json"
    results = {"schema_version": 1, "metadata": metadata, "parameters": {
        key: getattr(args, key) for key in ("warmups", "warm_samples", "cold_processes", "cli_processes", "repetitions", "smoke")},
        "method": {
            "boot": "parent Popen begin to reader observation of first flushed READY line; includes spawn, IPC, parent scheduling; not an exact exec-to-main timestamp",
            "quantiles": "nearest-rank: ceil(percentile / 100 * count), one-indexed; medians taken across repetitions, not pooled",
            "cli_report": "integer milliseconds truncated by CLI; compile bucket starts at execute_on_this_thread, before source read and compilation, and ends after engine/VFS/store/module/linker setup; run covers imports, instantiation, main, result conversion and blocking-work drain; wall is their sum before truncation, excludes process boot and final store/VFS teardown",
            "program": "marker Wasm start function: call benchmark.mark; end; one 64-KiB linear memory; full runtime imports installed",
            "cli_program": "function main(): number { return 0; }",
        }, "sandbox": {}, "process_boot": {}, "cli_first_run": {}}
    metadata["measurement_start"] = machine_state()
    with tempfile.TemporaryDirectory(prefix="submilli-bench-home-") as home, tempfile.TemporaryDirectory(prefix="submilli-bench-script-") as scripts:
        env = os.environ.copy()
        env.update({"SUBMILLI_HOME": home, "SUBMILLI_TELEMETRY": "0", "NO_COLOR": "1"})
        trivial = Path(scripts) / "trivial.ts"
        trivial.write_text(results["method"]["cli_program"] + "\n", encoding="utf-8")
        for mode in ("warm", "cold"):
            for vfs in ("ephemeral", "none"):
                repetitions, boot_repetitions = [], []
                for repetition in range(args.repetitions):
                    print(f"benchmark status: {mode}/{vfs} repetition {repetition + 1}/{args.repetitions}", file=sys.stderr, flush=True)
                    samples, boots = [], []
                    processes = 1 if mode == "warm" else args.cold_processes
                    for index in range(processes):
                        run = run_worker(worker, mode, vfs, args.warmups if mode == "warm" else 0,
                                         args.warm_samples if mode == "warm" else 1, args.timeout, env)
                        samples.extend(run["result"]["samples_ns"])
                        boots.append(run["boot_ns"])
                        if mode == "cold" and (index + 1) % 50 == 0:
                            print(f"  {index + 1}/{processes} fresh processes", file=sys.stderr, flush=True)
                    repetitions.append({"samples_ns": samples, "summary": summary_ns(samples)})
                    boot_repetitions.append({"samples_ns": boots, "summary": summary_ns(boots)})
                results["sandbox"][f"{mode}/{vfs}"] = {
                    "worker_contract": {key: value for key, value in run["result"].items() if key != "samples_ns"},
                    "repetitions": repetitions,
                    "median_of_repetition_quantiles": median_summary([item["summary"] for item in repetitions])}
                results["process_boot"][f"{mode}/{vfs}"] = {
                    "repetitions": boot_repetitions,
                    "median_of_repetition_quantiles": median_summary([item["summary"] for item in boot_repetitions])}
                # Preserve completed groups if a later child fails.
                path.write_text(json.dumps(results, indent=2, sort_keys=True) + "\n")
        print("benchmark status: CLI first-run", file=sys.stderr, flush=True)
        runs = [run_cli(cli, trivial, args.timeout, env) for _ in range(args.cli_processes)]
        results["cli_first_run"] = {"runs": runs, "process_total": summary_ns([run["process_total_ns"] for run in runs])}
        for bucket in ("wall", "compile", "run"):
            results["cli_first_run"][f"report_{bucket}"] = summary_ns(
                [run[f"report_{bucket}_ms"] * 1_000_000 for run in runs], allow_zero=True)
    metadata["measurement_end"] = machine_state()
    results["complete"] = True
    path.write_text(json.dumps(results, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print_tables(results)
    print(f"\nRaw JSON: {path}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (BenchmarkFailure, OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"benchmark error: {error}", file=sys.stderr)
        raise SystemExit(1)
