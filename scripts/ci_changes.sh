#!/usr/bin/env bash
# Keep required jobs present while avoiding their toolchain setup for unrelated edits.
set -euo pipefail

target=${1:?expected rust or packages}
paths=(
  crates skills examples scripts .cargo .gitattributes
  Cargo.toml Cargo.lock rust-toolchain rust-toolchain.toml
  rustfmt.toml .rustfmt.toml clippy.toml .clippy.toml
  docs/part-1-start-here/03-quickstart.md
  .github/workflows/ci.yml
  ':(glob)**/*.rs' ':(glob)**/Cargo.toml' ':(glob)**/Cargo.lock'
)
case "$target" in
  rust) ;;
  packages) paths+=(packages submilli.toml submilli.lock .github/workflows/packages.yml) ;;
  *) echo "Unknown CI target: $target" >&2; exit 1 ;;
esac

# PRs and merge groups compare their base tree with the checked-out merge tree.
# Pushes compare the previous tip with the new tip. Missing/unavailable bases
# (including a branch's first push) conservatively run the checks.
run=true
base=${BASE_SHA:-}
if [[ -n "$base" && ! "$base" =~ ^0+$ ]]; then
  if { git cat-file -e "${base}^{commit}" 2>/dev/null || git fetch --no-tags --depth=1 origin "$base"; } \
    && git diff --quiet --no-renames "$base" HEAD -- "${paths[@]}"; then
    run=false
    echo "::notice::No $target inputs changed; skipping expensive checks."
  fi
fi
printf 'run=%s\n' "$run" >> "${GITHUB_OUTPUT:?expected GITHUB_OUTPUT}"
