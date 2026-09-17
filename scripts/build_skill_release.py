#!/usr/bin/env python3
"""Write the `submilli-skill.json` asset that `submilli skill sync` downloads.

The file set matches what the CLI bundles: everything under skills/submilli
except dot-prefixed entries. The version comes from skills/submilli/VERSION.
"""
import argparse
import json
from pathlib import Path

SKILL = Path(__file__).resolve().parents[1] / "skills/submilli"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--expect-version", type=int,
                        help="fail unless VERSION equals this (the tag being released)")
    parser.add_argument("--min-cli", help="oldest CLI version that may install this release")
    args = parser.parse_args()

    version = int((SKILL / "VERSION").read_text().strip())
    if args.expect_version is not None and args.expect_version != version:
        raise SystemExit(f"skills/submilli/VERSION is {version}, expected {args.expect_version}")
    files = {
        path.relative_to(SKILL).as_posix(): path.read_text(encoding="utf-8")
        for path in sorted(SKILL.rglob("*"))
        if path.is_file() and not any(part.startswith(".") for part in path.relative_to(SKILL).parts)
    }
    release = {"schema": 1, "version": version, "files": files}
    if args.min_cli:
        release["min_cli"] = args.min_cli
    args.output.write_text(json.dumps(release, indent=1) + "\n", encoding="utf-8")
    print(f"skill v{version}: {len(files)} files -> {args.output}")


if __name__ == "__main__":
    main()
