#!/usr/bin/env python3
"""Serve the documented billing fixture in disposable storage for SDK checks.

No model or business-service credentials are needed. Build the local CLI and
server first. Stop with Ctrl-C; the child server and temporary store are removed.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


REPO = Path(__file__).resolve().parents[3]


def first_block(path, language):
    return path.read_text().split(f"```{language}\n", 1)[1].split("\n```", 1)[0]


def verify_policy(url):
    code = ('import { readBalance } from "@acme/billing"; '
            'function main(): number { return readBalance("cus_northwind"); }')
    for label, program, variables in [
        ("allowed", code, {"customerId": "cus_northwind"}),
        ("denied", code.replace("cus_northwind", "cus_initech"), {"customerId": "cus_northwind"}),
        ("missing", code, {}),
    ]:
        payload = {"blueprint": "support-read", "variables": variables, "code": program}
        request = Request(url + "/v1/execute", data=json.dumps(payload).encode(),
                          headers={"content-type": "application/json"})
        try:
            response = urlopen(request, timeout=10)
        except HTTPError as error:
            response = error
        with response:
            body = json.load(response)
        if label == "allowed":
            assert body.get("result") == "6150", body
        elif label == "denied":
            assert "permission denied" in json.dumps(body), body
            assert "acme.com/balance.read" in json.dumps(body), body
        else:
            assert body.get("error", {}).get("kind") == "invalid_request", body


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=18128)
    parser.add_argument("--cli", type=Path, default=REPO / "target/debug/submilli")
    parser.add_argument("--server", type=Path, default=REPO / "target/debug/submilli-server")
    args = parser.parse_args()
    cli, server = args.cli.resolve(), args.server.resolve()
    references = REPO / "skills/submilli/references"
    with tempfile.TemporaryDirectory(prefix="submilli-harness-") as directory:
        root = Path(directory)
        # Isolate every Submilli location, including inherited server overrides.
        env = {k: v for k, v in os.environ.items()
               if not k.startswith("SUBMILLI_") and k not in {"HOST", "PORT"}}
        env.update(SUBMILLI_HOME=str(root / "store"), SUBMILLI_TELEMETRY="0")

        def run(*arguments):
            subprocess.run([str(cli), *arguments], cwd=root, env=env,
                           check=True, stdout=subprocess.DEVNULL)

        run("build", "init", "@acme/billing", "package")
        (root / "package/src/lib.ts").write_text(first_block(references / "packages.md", "typescript"))
        (root / "package/tests/lib.test.ts").write_text(
            'import { readBalance } from "@acme/billing";\n'
            'function main(): void { assert(readBalance("cus_northwind") === 6150); }\n')
        run("build", "check")
        run("build", "test")
        run("build", "publish-local")
        blueprint = root / "blueprint.yaml"
        blueprint.write_text(first_block(references / "blueprints.md", "yaml"))
        run("blueprint", "lint", str(blueprint))
        url = f"http://127.0.0.1:{args.port}"
        with (root / "server.log").open("w+") as log:
            process = subprocess.Popen(
                [str(server), "--bind", "127.0.0.1", "--port", str(args.port)],
                cwd=root, env=env, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 20
                while True:
                    if process.poll() is not None:
                        log.seek(0)
                        raise RuntimeError(log.read())
                    try:
                        with urlopen(url + "/v1/status", timeout=1) as response:
                            status = json.load(response)
                        # Do not register a test blueprint on a pre-existing server.
                        if status.get("pid") != process.pid:
                            raise RuntimeError("Port is owned by another server; choose --port")
                        break
                    except URLError:
                        if time.monotonic() >= deadline:
                            raise TimeoutError("Fixture server did not become ready")
                        time.sleep(0.1)
                run("server", "blueprint", "apply", str(blueprint), "--server", url)
                verify_policy(url)
                print(f"Fixture ready: SUBMILLI_SERVER_URL={url}; blueprint=support-read", flush=True)
                print("readBalance(bound customer) = 6150; other customer denied. Ctrl-C to stop.", flush=True)
                process.wait()
            except KeyboardInterrupt:
                pass
            finally:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()


if __name__ == "__main__":
    main()
