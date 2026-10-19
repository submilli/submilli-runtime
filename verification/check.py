#!/usr/bin/env python3
"""Run bounded TLC checks; retain configurations, traces and summaries outside Git."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request


ROOT = Path(__file__).resolve().parent
COMPLETE = "Model checking completed. No error has been found."
INVARIANT_EXIT = 12  # TLC 1.7.4: EC.ExitStatus.VIOLATION_SAFETY.


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", default="durability")
    parser.add_argument("--java", default=os.environ.get("JAVA", "java"))
    parser.add_argument("--jar", type=Path, help="Must match the pinned SHA-256.")
    parser.add_argument("--case", action="append", dest="cases")
    parser.add_argument("--suite", choices=("ci", "extended"), default="ci")
    parser.add_argument("--output", type=Path, help="A new or empty output directory.")
    parser.add_argument("--list", action="store_true", help="List selected checks without Java.")
    args = parser.parse_args()
    try:
        model_dir = ROOT / args.model
        manifest = json.loads((model_dir / "checks.json").read_text())
        cases = select_cases(manifest, args.cases, args.suite)
        if args.list:
            for case in cases:
                print(case["name"], case["kind"])
            return 0
        output = output_directory(args.output)
        print("Evidence directory: {}".format(output), flush=True)
        jar = tool_jar(args.jar)
        java = subprocess.run(
            [args.java, "-version"], capture_output=True, text=True, timeout=15
        )
        if java.returncode:
            raise ValueError("Java is unavailable:\n" + java.stderr)
        (output / "java-version.txt").write_text(java.stdout + java.stderr)
        summary = []
        for case in cases:
            result = run_case(args.java, jar, model_dir, manifest, case, output)
            summary.append(result)
            (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
            print("{} {}: {}".format(
                "PASS" if result["passed"] else "FAIL", case["name"], result["detail"]
            ), flush=True)
            if not result["passed"]:
                print("See {}".format(output / case["name"] / "tlc.log"), file=sys.stderr)
                return 1
        return 0
    except (OSError, ValueError, KeyError, subprocess.TimeoutExpired) as error:
        print("Verification failed: {}".format(error), file=sys.stderr)
        return 1


def select_cases(manifest, requested, suite):
    cases = manifest["cases"]
    names = [case["name"] for case in cases]
    if len(names) != len(set(names)):
        raise ValueError("Duplicate check names")
    if any(not re.fullmatch(r"[a-z0-9-]+", name) for name in names):
        raise ValueError("Check names must be lowercase letters, digits or hyphens")
    if requested:
        missing = set(requested) - set(names)
        if missing:
            raise ValueError("Unknown checks: " + ", ".join(sorted(missing)))
        return [case for case in cases if case["name"] in requested]
    selected = [case for case in cases if case.get("suite", "ci") == suite]
    if not selected:
        raise ValueError("No checks selected")
    return selected


def output_directory(requested):
    if requested is None:
        return Path(tempfile.mkdtemp(prefix="submilli-tlc-"))
    requested = requested.resolve()
    requested.mkdir(parents=True, exist_ok=True)
    if any(requested.iterdir()):
        raise ValueError("Evidence output directory must be empty: " + str(requested))
    return requested


def tool_jar(override):
    pin = json.loads((ROOT / "toolchain.json").read_text())
    if override is not None:
        check_digest(override, pin["sha256"])
        return override.resolve()
    cache = Path(tempfile.gettempdir()) / "submilli-tla-tools"
    cache.mkdir(parents=True, exist_ok=True)
    jar = cache / ("tla2tools-" + pin["release"] + ".jar")
    if jar.exists():
        check_digest(jar, pin["sha256"])
        return jar
    # Verify before publishing; a partial or concurrently downloaded jar is never run.
    with tempfile.TemporaryDirectory(prefix="download-", dir=cache) as temporary:
        downloaded = Path(temporary) / "tla2tools.jar"
        with urllib.request.urlopen(pin["url"], timeout=60) as source:
            with downloaded.open("wb") as target:
                shutil.copyfileobj(source, target)
        check_digest(downloaded, pin["sha256"])
        os.replace(downloaded, jar)
    return jar


def check_digest(path, expected):
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    if actual != expected:
        raise ValueError("TLC jar checksum mismatch: " + str(path))


def render_config(manifest, case):
    constants = dict(manifest["constants"])
    overrides = case.get("constants", {})
    unknown = set(overrides) - set(constants)
    if unknown:
        raise ValueError("Unknown constants: " + ", ".join(sorted(unknown)))
    constants.update(overrides)
    lines = ["SPECIFICATION " + case.get("specification", "Spec"), "CONSTANTS"]
    for name, value in constants.items():
        lines.append("    {} = {}".format(name, tla_value(value)))
    lines.append("CHECK_DEADLOCK FALSE")
    kind = case["kind"]
    if kind in ("negative", "witness"):
        invariants = [case["invariant"]]
        properties = []
    elif kind in ("safety", "liveness"):
        invariants = manifest["invariants"]
        properties = case.get("properties", manifest.get("properties", []))
    else:
        raise ValueError("Unknown check kind: " + kind)
    lines.extend("INVARIANT " + name for name in invariants)
    lines.extend("PROPERTY " + name for name in properties)
    return "\n".join(lines) + "\n"


def tla_value(value):
    if isinstance(value, bool):
        return "TRUE" if value else "FALSE"
    if isinstance(value, (int, str)):
        return json.dumps(value)
    if isinstance(value, dict) and set(value) == {"model_value"}:
        name = value["model_value"]
        if isinstance(name, str) and re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", name):
            return name
    raise ValueError("Unsupported TLA constant: " + repr(value))


def run_case(java, jar, model_dir, manifest, case, output):
    directory = output / case["name"]
    directory.mkdir()
    module = manifest["module"]
    for source in model_dir.glob("*.tla"):
        shutil.copyfile(source, directory / source.name)
    config = directory / (module + ".cfg")
    config.write_text(render_config(manifest, case))
    command = [
        java, "-XX:+UseParallelGC", "-Xmx1g", "-cp", str(jar), "tlc2.TLC",
        "-workers", "1", "-fp", "0", "-seed", "1",
        "-metadir", str(directory / "states"),
        "-config", config.name, module + ".tla",
    ]
    (directory / "command.json").write_text(json.dumps(command, indent=2) + "\n")
    started = time.monotonic()
    with (directory / "tlc.log").open("w") as log:
        try:
            completed = subprocess.run(
                command, cwd=directory, stdout=log, stderr=subprocess.STDOUT,
                timeout=case.get("timeout_seconds", 90),
            )
            code = completed.returncode
            timed_out = False
        except subprocess.TimeoutExpired:
            code = None
            timed_out = True
    text = (directory / "tlc.log").read_text()
    passed, detail = assess_result(case, code, text, timed_out)
    counts = state_counts(text)
    return {
        "name": case["name"], "kind": case["kind"], "passed": passed,
        "detail": detail, "exit_code": code, "timed_out": timed_out,
        "seconds": round(time.monotonic() - started, 3),
        "counts": counts,
        "constants": dict(manifest["constants"], **case.get("constants", {})),
        "module_sha256": hashlib.sha256((directory / (module + ".tla")).read_bytes()).hexdigest(),
    }


def state_counts(text):
    matches = re.findall(
        r"([\d,]+) states generated, ([\d,]+) distinct states found, "
        r"([\d,]+) states left on queue\.", text
    )
    return [int(count.replace(",", "")) for count in matches[-1]] if matches else None


def assess_result(case, code, text, timed_out=False):
    if timed_out:
        return False, "timeout; exploration incomplete"
    if case["kind"] in ("negative", "witness"):
        expected = "Invariant {} is violated.".format(case["invariant"])
        if code == INVARIANT_EXIT and expected in text and "State 1:" in text:
            return True, "expected counterexample: " + case["invariant"]
        return False, "expected {} counterexample, got exit {}".format(
            case["invariant"], code
        )
    counts = state_counts(text)
    if code != 0 or COMPLETE not in text or counts is None or counts[2] != 0:
        return False, "exhaustive completion not established (exit {})".format(code)
    if case["kind"] == "liveness" and "Finished checking temporal properties" not in text:
        return False, "temporal-property completion not established"
    return True, "complete bounded " + case["kind"] + " check"


if __name__ == "__main__":
    sys.exit(main())
