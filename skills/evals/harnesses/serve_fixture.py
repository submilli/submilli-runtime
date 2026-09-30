#!/usr/bin/env python3
"""Serve the documented billing fixture in disposable storage for SDK checks.

No model or business-service credentials are needed. Build the local CLI and
server first. The server requires an API token: the fixture generates an admin
token, which the server and the CLI read from SUBMILLI_SERVER_TOKEN, for its
own setup. It also declares a second token in the `user` role and prints that
one for the adapter checks, so they prove the MCP surface needs no more than
`user`. Stop with Ctrl-C; the child server and temporary store are removed.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


REPO = Path(__file__).resolve().parents[3]
# The admin token comes from SUBMILLI_SERVER_TOKEN; a second role needs a config.
CONFIG = """\
api_tokens:
- name: app
  role: user
  token_file: {token_file}
"""


def first_block(path, language):
    return path.read_text().split(f"```{language}\n", 1)[1].split("\n```", 1)[0]


def call(url, token=None, payload=None):
    """Return (status, JSON body or None); an HTTP error status is an answer."""
    headers = {} if token is None else {"Authorization": f"Bearer {token}"}
    data = None
    if payload is not None:
        headers["content-type"] = "application/json"
        data = json.dumps(payload).encode()
    try:
        response = urlopen(Request(url, data=data, headers=headers), timeout=10)
    except HTTPError as error:
        response = error
    with response:
        body = response.read()
    return response.status, json.loads(body) if body else None


def verify_auth(url, user_token):
    payload = {"blueprint": "support-read", "variables": {"customerId": "cus_northwind"},
               "code": "function main(): number { return 1; }"}
    status, body = call(url + "/v1/execute", None, payload)
    assert status == 401 and body.get("error") == "unauthorized", (status, body)
    status, body = call(url + "/v1/execute", secrets.token_hex(32), payload)
    assert status == 401 and body.get("error") == "unauthorized", (status, body)
    # The user token runs code and cannot manage the server.
    status, body = call(url + "/v1/status", user_token)
    assert status == 403 and body.get("error") == "forbidden", (status, body)


def verify_policy(url, user_token):
    code = ('import { readBalance } from "@acme/billing"; '
            'function main(): number { return readBalance("cus_northwind"); }')
    for label, program, variables in [
        ("allowed", code, {"customerId": "cus_northwind"}),
        ("denied", code.replace("cus_northwind", "cus_initech"), {"customerId": "cus_northwind"}),
        ("missing", code, {}),
    ]:
        payload = {"blueprint": "support-read", "variables": variables, "code": program}
        _, body = call(url + "/v1/execute", user_token, payload)
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
        admin_token, user_token = secrets.token_hex(32), secrets.token_hex(32)
        # The server takes its admin token from this, and the CLI sends it.
        env.update(SUBMILLI_HOME=str(root / "store"), SUBMILLI_TELEMETRY="0",
                   SUBMILLI_SERVER_TOKEN=admin_token)

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
        token_file = root / "app.token"
        token_file.touch(mode=0o600)
        token_file.write_text(user_token)
        config = root / "server.yaml"
        config.write_text(CONFIG.format(token_file=json.dumps(str(token_file))))
        url = f"http://127.0.0.1:{args.port}"
        with (root / "server.log").open("w+") as log:
            process = subprocess.Popen(
                [str(server), "--config", str(config), "--bind", "127.0.0.1", "--port", str(args.port)],
                cwd=root, env=env, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 20
                while True:
                    if process.poll() is not None:
                        log.seek(0)
                        raise RuntimeError(log.read())
                    try:
                        # The health probe needs no token.
                        if call(url + "/healthz")[0] == 200:
                            break
                    except URLError:
                        pass
                    if time.monotonic() >= deadline:
                        raise TimeoutError("Fixture server did not become ready")
                    time.sleep(0.1)
                # Do not register a test blueprint on a pre-existing server: one
                # that refuses this run's admin token, or reports another pid.
                code, status = call(url + "/v1/status", admin_token)
                if code != 200 or status.get("pid") != process.pid:
                    raise RuntimeError("Port is owned by another server; choose --port")
                run("server", "blueprint", "apply", str(blueprint), "--server", url)
                verify_auth(url, user_token)
                verify_policy(url, user_token)
                print("Fixture ready: blueprint=support-read. For the adapter checks:", flush=True)
                # The user token: enough for every adapter check, and no more.
                print(f"export SUBMILLI_SERVER_URL={url} SUBMILLI_SERVER_TOKEN={user_token}", flush=True)
                print("readBalance(bound customer) = 6150; other customer denied; "
                      "no token, or an unknown one, refused; user token 403 on /v1/status. Ctrl-C to stop.", flush=True)
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
