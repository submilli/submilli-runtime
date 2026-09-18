#!/usr/bin/env python3
"""Prepare paired workspaces, run a stdin-driven assistant, and score evidence.

No model SDK dependency: the command after -- reads the task from stdin and
writes its transcript to stdout. Configure its model, permissions and clean
profile explicitly. This runner does not grant additional execution authority.
"""
import argparse
import hashlib
import json
import pathlib
import shutil
import subprocess
import sys
import time

HERE = pathlib.Path(__file__).resolve().parent
SKILL = HERE.parent / "submilli"
DIRECTORIES = {"claude": ".claude", "codex": ".agents", "cursor": ".cursor"}


def cases():
    return json.loads((HERE / "cases.json").read_text())


def fingerprint(root):
    return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(root.rglob("*")) if p.is_file() and not p.is_symlink()}


def prepare(args):
    selected = [c for c in cases() if not args.case or c["id"] in args.case]
    if args.case and set(args.case) - {c["id"] for c in selected}:
        raise ValueError("unknown case id")
    if args.repeats < 1:
        raise ValueError("repeats must be positive")
    args.output.mkdir(parents=True, exist_ok=False)
    trials = []
    for case in selected:
        for repeat in range(args.repeats):
            for variant in ["skill", "baseline"]:
                trial_id = f'{case["id"]}-{repeat + 1}-{variant}'
                folder = args.output / trial_id
                workspace = folder / "workspace"
                workspace.mkdir(parents=True)
                if "fixture" in case:
                    shutil.copytree(HERE / "fixtures" / case["fixture"], workspace,
                                    dirs_exist_ok=True)
                if variant == "skill":
                    shutil.copytree(SKILL, workspace / DIRECTORIES[args.agent] / "skills/submilli")
                # Rubrics stay outside the task workspace; never send them in
                # the actor prompt. Natural prompts also test skill triggering.
                prompt = case["prompt"] + "\n"
                if getattr(args, "notice", None):
                    prompt += "\n" + args.notice.rstrip() + "\n"
                (folder / "prompt.txt").write_text(prompt)
                trials.append({"id": trial_id, "case": case["id"], "variant": variant,
                               "repeat": repeat + 1, "before": fingerprint(workspace)})
    (args.output / "suite.json").write_text(json.dumps({
        "agent": args.agent, "skill_sha256": fingerprint(SKILL),
        "cases": selected, "trials": trials}, indent=2) + "\n")
    print(f"Prepared {len(trials)} trials in {args.output}")


def run(args):
    command = args.command
    if command and command[0] == "--":
        command = command[1:]
    if not command:
        raise ValueError("provide an assistant command after --")
    if args.timeout <= 0:
        raise ValueError("timeout must be positive")
    suite = json.loads((args.output / "suite.json").read_text())
    for trial in suite["trials"]:
        if args.trial and trial["id"] not in args.trial:
            continue
        folder = args.output / trial["id"]
        if (folder / "result.json").exists():
            raise ValueError(f'{trial["id"]} already ran; prepare a new run to repeat')
        print(f'Running {trial["id"]}', flush=True)
        started = time.monotonic()
        try:
            # Own session and process group: servers the assistant leaves
            # running in the background are stopped with it.
            result = subprocess.run(command, cwd=folder / "workspace",
                                    input=(folder / "prompt.txt").read_text(),
                                    text=True, capture_output=True, timeout=args.timeout,
                                    check=False, start_new_session=True)
            stdout, stderr, code = result.stdout, result.stderr, result.returncode
        except subprocess.TimeoutExpired as error:
            stdout = error.stdout or b""
            stderr = error.stderr or b""
            stdout = stdout.decode(errors="replace") if isinstance(stdout, bytes) else stdout
            stderr = stderr.decode(errors="replace") if isinstance(stderr, bytes) else stderr
            code = 124
        except OSError as error:
            stdout, stderr, code = "", str(error), 127
        stop_leftovers(folder / "workspace")
        (folder / "stdout.txt").write_text(stdout)
        (folder / "stderr.txt").write_text(stderr)
        (folder / "result.json").write_text(json.dumps({
            "command": command, "exit_code": code,
            "duration_seconds": time.monotonic() - started,
            "after": fingerprint(folder / "workspace")}, indent=2) + "\n")


def stop_leftovers(workspace):
    """Terminate processes still running from inside the trial workspace."""
    listing = subprocess.run(["ps", "-axo", "pid=,command="], text=True,
                             capture_output=True, check=False).stdout
    for line in listing.splitlines():
        pid, _, command = line.strip().partition(" ")
        if str(workspace) in command or "submilli-server" in command:
            cwd = subprocess.run(["lsof", "-a", "-p", pid, "-d", "cwd", "-Fn"], text=True,
                                 capture_output=True, check=False).stdout
            if str(workspace) in cwd or str(workspace) in command:
                subprocess.run(["kill", pid], check=False)


def score(args):
    suite = json.loads((args.output / "suite.json").read_text())
    grades = json.loads(args.grades.read_text())
    by_case = {case["id"]: case for case in suite["cases"]}
    expected = {trial["id"] for trial in suite["trials"]}
    if set(grades) != expected:
        raise ValueError("grades must include exactly every prepared trial")
    totals = {variant: {"passed": 0, "total": 0, "critical_failures": 0,
                        "execution_failures": 0} for variant in ["skill", "baseline"]}
    for trial in suite["trials"]:
        case = by_case[trial["case"]]
        checks = case["criteria"] + case["critical"]
        grade = grades[trial["id"]]
        if set(grade) != set(checks):
            raise ValueError(f'{trial["id"]}: grade each criterion exactly once')
        for verdict in grade.values():
            if (set(verdict) != {"pass", "evidence"} or type(verdict["pass"]) is not bool
                    or not isinstance(verdict["evidence"], str) or not verdict["evidence"].strip()):
                raise ValueError("each verdict needs a boolean pass and nonempty evidence")
        result = json.loads((args.output / trial["id"] / "result.json").read_text())
        critical_failed = any(not grade[c]["pass"] for c in case["critical"])
        total = totals[trial["variant"]]
        total["total"] += 1
        total["critical_failures"] += int(critical_failed)
        total["execution_failures"] += int(result["exit_code"] != 0)
        total["passed"] += int(result["exit_code"] == 0 and all(v["pass"] for v in grade.values()))
    print(json.dumps(totals, indent=2))
    return 0 if totals["skill"]["passed"] == totals["skill"]["total"] else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subs = parser.add_subparsers(dest="action", required=True)
    prep = subs.add_parser("prepare")
    prep.add_argument("output", type=pathlib.Path)
    prep.add_argument("--agent", choices=DIRECTORIES, required=True)
    prep.add_argument("--case", action="append")
    prep.add_argument("--repeats", type=int, default=3)
    prep.add_argument("--notice", help="text appended to every prompt, identically in "
                      "both variants (for example a tool-call budget and a request "
                      "for a final status file)")
    execute = subs.add_parser("run")
    execute.add_argument("output", type=pathlib.Path)
    execute.add_argument("--trial", action="append")
    execute.add_argument("--timeout", type=int, default=300)
    execute.add_argument("command", nargs=argparse.REMAINDER)
    grade = subs.add_parser("score")
    grade.add_argument("output", type=pathlib.Path)
    grade.add_argument("grades", type=pathlib.Path)
    args = parser.parse_args()
    args.output = args.output.resolve()
    try:
        return {"prepare": prepare, "run": run, "score": score}[args.action](args) or 0
    except (ValueError, OSError, KeyError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
