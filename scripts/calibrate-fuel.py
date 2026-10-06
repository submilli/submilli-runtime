#!/usr/bin/env python3
"""Measure representative programs with a freshly built release CLI (SUB-1270).

Run from the repository root after `cargo build --release --locked -p submilli`.
Uses only Python's standard library. HTTP measurements use a loopback server.
JSON output and generated programs belong outside Git, e.g. under /tmp.
"""

import argparse
import contextlib
import http.server
import json
import os
from pathlib import Path
import re
import resource
import subprocess
import tempfile
import threading
import time


REPORT = re.compile(
    r"fuel: ([\d,]+) \(wasm ([\d,]+), host ([\d,]+)\).*"
    r"wall: (\d+) ms \(compile (\d+) ms, run (\d+) ms\)"
)
# Including the comma, each record occupies 250 ASCII bytes.
JSON_ROW = '{"id":123,"name":"' + "x" * 229 + '"},'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/submilli"))
    parser.add_argument("--only", help="Measure names containing this substring")
    parser.add_argument("--http", action="store_true", help="Include a local HTTP response")
    parser.add_argument("--trials", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=120)
    args = parser.parse_args()
    if args.trials < 1 or args.timeout <= 0:
        parser.error("trials and timeout must be positive")
    binary = args.binary.resolve(strict=True)
    succeeded = True
    matched = False
    with tempfile.TemporaryDirectory(prefix="submilli-fuel-") as directory:
        root = Path(directory)
        for name, body in workloads().items():
            if args.only is None or args.only in name:
                matched = True
                succeeded = measure(binary, root, name, "function main(): number {\n" + body + "\n}\n", args) and succeeded
        if args.http and (args.only is None or args.only in "http-1000000"):
            with response_server() as url:
                source = (
                    'import { get } from "submilli:http";\n'
                    'function main(): number {\n'
                    f'    const response = get({json.dumps(url)});\n'
                    '    if (response.status !== 200) { throw new Error("HTTP failure"); }\n'
                    '    return (response.json() as unknown[]).length;\n'
                    '}\n'
                )
                matched = True
                succeeded = measure(binary, root, "http-1000000", source, args) and succeeded
    if not matched:
        parser.error("no workload matched (HTTP requires --http)")
    return 0 if succeeded else 1


def workloads():
    programs = {
        "baseline": "return 0;",
        "loop": "let acc = 0; for (let i = 0; i < 1000000; i++) { acc = (acc + i * 7) % 1000003; } return acc;",
    }
    for size in (100000, 1000000):
        text = f'const text = "abcd efgh ".repeat({size // 10}); '
        programs[f"upper-{size}"] = text + "return text.toUpperCase().length;"
        programs[f"replace-{size}"] = text + 'return text.replaceAll("abcd", "ABCD").length;'
        programs[f"split-{size}"] = text + 'return text.split(" ").length;'
        records = (size - 6) // len(JSON_ROW)
        programs[f"json-{size}"] = (
            f'const text = ("[" + {json.dumps(JSON_ROW)}.repeat({records}) + "null]")'
            f'.padEnd({size}, " "); return JSON.stringify(JSON.parse(text)).length;'
        )
    programs["regex-100000"] = (
        'const text = "item=1234 padding....".repeat(5000); '
        'const pattern = /item=[0-9]+/g; let count = 0; '
        'while (pattern.test(text)) { count++; } return count;'
    )
    for size in (10000, 100000):
        programs[f"map-{size}"] = (
            'const values = new Map<string, number>(); '
            f'for (let i = 0; i < {size}; i++) {{ values.set(String(i), i); }} return values.size;'
        )
        array = (
            'const values: number[] = []; '
            f'for (let i = 0; i < {size}; i++) {{ values.push(({size} - i) % 997); }} '
        )
        programs[f"array-{size}"] = array + "return values.length;"
        programs[f"sort-{size}"] = array + "values.sort((a, b) => a - b); return values.length;"
    return programs


def measure(binary, root, name, source, args):
    program = root / f"{name}.ts"
    program.write_text(source)
    for trial in range(args.trials):
        before = resource.getrusage(resource.RUSAGE_CHILDREN)
        start = time.perf_counter()
        try:
            result = subprocess.run(
                [str(binary), "run", str(program), "--report"],
                env={**os.environ, "SUBMILLI_HOME": str(root / "home"), "SUBMILLI_TELEMETRY": "0"},
                capture_output=True, text=True, timeout=args.timeout,
            )
        except subprocess.TimeoutExpired:
            print(json.dumps({"name": name, "trial": trial + 1, "error": "timeout", "seconds": args.timeout}), flush=True)
            return False
        wall = time.perf_counter() - start
        after = resource.getrusage(resource.RUSAGE_CHILDREN)
        sample = {
            "name": name, "trial": trial + 1, "exit_code": result.returncode,
            "process_wall_ms": wall * 1000,
            "process_cpu_ms": (after.ru_utime + after.ru_stime - before.ru_utime - before.ru_stime) * 1000,
            "result": result.stdout.strip(), "report": result.stderr.strip(),
        }
        match = REPORT.search(result.stderr)
        if match:
            fields = ("fuel", "wasm", "host", "wall_ms", "compile_ms", "run_ms")
            sample.update(zip(fields, (int(value.replace(",", "")) for value in match.groups())))
        print(json.dumps(sample), flush=True)
        if result.returncode != 0 or match is None:
            return False
    return True


@contextlib.contextmanager
def response_server():
    body = ("[" + JSON_ROW * 3999 + "null]").ljust(1000000).encode("ascii")

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_args):
            pass

    server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}/data"
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == "__main__":
    raise SystemExit(main())
