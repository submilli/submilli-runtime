#!/bin/sh
# Usage: sh install.sh [--version v0.1.0] [--install-dir "$HOME/.local/bin"]
set -eu

version=latest
install_dir=${HOME:?HOME must be set}/.local/bin
release_url=https://github.com/submilli/submilli-runtime/releases
fail() { printf 'Error: %s\n' "$*" >&2; exit 1; }
while [ "$#" -gt 0 ]; do
    case "$1" in
        --version|--install-dir)
            [ "$#" -ge 2 ] || fail "$1 requires a value"
            case "$1" in
                --version) version=$2 ;;
                --install-dir) install_dir=$2 ;;
            esac
            shift 2 ;;
        -h|--help)
            printf '%s\n' 'Usage: sh install.sh [--version TAG] [--install-dir DIR]' \
                'Defaults: latest release; $HOME/.local/bin. Requires curl and a SHA-256 utility.'
            exit 0 ;;
        *) fail "Unknown option: $1" ;;
    esac
done
[ -n "$install_dir" ] || fail 'Install directory must not be empty'
case "$(uname -s):$(uname -m)" in
    Linux:x86_64|Linux:amd64) target=x86_64-unknown-linux-musl ;;
    Darwin:x86_64) target=x86_64-apple-darwin ;;
    Darwin:arm64|Darwin:aarch64) target=aarch64-apple-darwin ;;
    *) fail 'Supported platforms: Linux x86_64, macOS Intel and Apple Silicon. Use install.ps1 on Windows.' ;;
esac
command -v curl >/dev/null 2>&1 || fail 'curl is required'
if command -v sha256sum >/dev/null 2>&1; then
    hash_tool=sha256sum
elif command -v shasum >/dev/null 2>&1; then
    hash_tool=shasum
else
    fail 'sha256sum or shasum is required'
fi
# Resolve latest once so the binary and checksum always use the same release.
if [ "$version" = latest ]; then
    resolved=$(curl -fsSL --proto '=https' --proto-redir '=https' -o /dev/null -w '%{url_effective}' "$release_url/latest")
    case "$resolved" in
        "$release_url/tag/"*) version=${resolved##*/} ;;
        *) fail 'Could not resolve the latest release tag' ;;
    esac
fi
case "$version" in
    ''|*[!a-zA-Z0-9._-]*|-*) fail 'Invalid release tag' ;;
esac
asset=submilli-$target
scratch=$(mktemp -d)
staged=
cleanup() {
    rm -rf "$scratch"
    if [ -n "$staged" ]; then rm -f "$staged"; fi
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
base=$release_url/download/$version
curl -fsSL --proto '=https' --proto-redir '=https' "$base/$asset" -o "$scratch/$asset"
curl -fsSL --proto '=https' --proto-redir '=https' "$base/SHA256SUMS" -o "$scratch/SHA256SUMS"
expected=$(awk -v name="$asset" '$2 == name { print $1 }' "$scratch/SHA256SUMS")
[ "${#expected}" -eq 64 ] || fail "Missing or ambiguous checksum for $asset"
case "$expected" in *[!0-9a-fA-F]*) fail 'Invalid checksum' ;; esac
if [ "$hash_tool" = sha256sum ]; then
    actual=$(sha256sum "$scratch/$asset" | awk '{print $1}')
else
    actual=$(shasum -a 256 "$scratch/$asset" | awk '{print $1}')
fi
[ "$actual" = "$(printf '%s' "$expected" | tr A-F a-f)" ] || fail 'Checksum mismatch; existing installation was not changed'
chmod 755 "$scratch/$asset"
"$scratch/$asset" --version
mkdir -p "$install_dir"
[ ! -d "$install_dir/submilli" ] || fail 'Installation target is a directory'
# Stage on the destination filesystem so replacement is an atomic rename.
staged=$(mktemp "$install_dir/.submilli.XXXXXX")
cp "$scratch/$asset" "$staged"
chmod 755 "$staged"
mv -f "$staged" "$install_dir/submilli"
staged=
printf 'Installed %s to %s/submilli\n' "$version" "$install_dir"
case ":${PATH:-}:" in
    *":$install_dir:"*) ;;
    *) printf 'Add this directory to your PATH: %s\n' "$install_dir" ;;
esac
