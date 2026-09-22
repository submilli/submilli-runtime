#!/usr/bin/env python3
"""Flag trials whose assistant reached material outside its workspace.

A baseline that read the repository, the docs, an installed copy of the
skill, or the skill text embedded in the CLI executable is not a baseline.
Only the tool calls an assistant made are examined, so prose that merely
mentions a path or a command does not count. Expects Claude Code
`--output-format stream-json` transcripts in each trial's stdout.txt.
Prints one line per trial and exits 1 if any trial is contaminated.
"""
import json
import pathlib
import re
import sys

PATTERNS = {
    "repository": re.compile(r"submilli-runtime/(skills|docs|crates|examples|llm-prompt)"),
    "home": re.compile(r"(/Users/[^/\s]+|~|\$HOME)/(development|\.claude/skills|\.submilli)"),
    "skill install": re.compile(r"submilli(\.real)? skill (install|sync|update)"),
    # The CLI embeds the skill and llm-prompt.md as text, so dumping the
    # executable's strings recovers them.
    "binary inspection": re.compile(r"\b(strings|xxd|hexdump)\b.*submilli"),
    "eval tooling": re.compile(r"submilli-eval/(settings\.json|smoke|run\.py)"),
}


def tool_inputs(transcript):
    for line in transcript.read_text(errors="replace").splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        for block in (event.get("message") or {}).get("content") or []:
            if isinstance(block, dict) and block.get("type") == "tool_use":
                yield json.dumps(block.get("input", {}))


def main(run_dir):
    contaminated = False
    for trial in sorted(p for p in pathlib.Path(run_dir).iterdir() if (p / "stdout.txt").exists()):
        calls = "\n".join(tool_inputs(trial / "stdout.txt"))
        hits = [name for name, pattern in PATTERNS.items() if pattern.search(calls)]
        # The skill variant runs `skill sync` because the skill tells it to.
        if trial.name.endswith("-skill"):
            hits = [hit for hit in hits if hit != "skill install"]
        print(f"{trial.name:36} {'CONTAMINATED: ' + ', '.join(hits) if hits else 'clean'}")
        contaminated |= bool(hits)
    return 1 if contaminated else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
