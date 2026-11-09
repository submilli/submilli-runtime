"""Check whether the Compose guide's pinned runtime image can be exercised."""

import argparse
import json
import os
from pathlib import Path
import re
import sys

from publish_chart import IMAGE_REPOSITORY, VERSION, Registry


ROOT = Path(__file__).resolve().parents[1]
MCP_SERVER_NAME = "io.github.submilli/submilli-runtime"


def main(arguments=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, default=ROOT,
                        help="Read the guide and workspace version from this checkout")
    parser.add_argument("--print-target", action="store_true")
    parser.add_argument("--require-published", action="store_true")
    parser.add_argument("--skip-registry", action="store_true",
                        help="Check local release metadata without querying GHCR")
    parser.add_argument("--runtime-tag", default="")
    args = parser.parse_args(arguments)
    document = args.source_root / "docs/part-4-server/06-deploy-with-compose.md"
    tag, image = compose_target(document.read_text())
    if args.runtime_tag and tag != args.runtime_tag:
        raise ValueError("Compose guide release pin must match the runtime release tag")
    if args.print_target:
        print(tag, image)
        return

    manifest = args.source_root / "Cargo.toml"
    version = workspace_version(manifest.read_text())
    metadata_path = args.source_root / "server.json"
    if args.source_root.resolve() == ROOT.resolve() or metadata_path.exists():
        release_metadata(args.source_root, version)
        metadata_checked = True
    else:
        # The verifier runs from the current workflow revision against old
        # release checkouts, which predate MCP registry metadata.
        metadata_checked = False
    if args.skip_registry:
        if metadata_checked:
            print(f"Compose release metadata is consistent: {image}")
        else:
            print(f"Compose image is from a legacy source without MCP registry metadata: {image}")
        return

    ready = image_ready(tag[1:], version, args.require_published)
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as file:
            file.write(f"available={str(ready).lower()}\n")
    if ready:
        print(f"Compose image is anonymously available: {image}")
        return

    message = (
        f"Compose journey deferred: {image} has not been published yet. "
        "The Release workflow requires this journey after container publication."
    )
    print(f"::notice::{message}")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as file:
            file.write(message + "\n")


def compose_target(document):
    section = re.search(r"^## Start it\s*\n(.*?)(?=^## |\Z)", document, re.MULTILINE | re.DOTALL)
    commands = re.search(r"```sh\n(.*?)```", section.group(1), re.DOTALL) if section else None
    if commands is None:
        raise ValueError("Compose guide must contain the Start it shell commands")
    release = re.search(
        r"curl -fsSLO https://raw\.githubusercontent\.com/submilli/submilli-runtime/"
        r"(v[^/\s]+)/compose\.yaml", commands.group(1),
    )
    image = re.search(
        r"printf 'SUBMILLI_IMAGE=(ghcr\.io/submilli/submilli-runtime:([^\\\s']+))\\n",
        commands.group(1),
    )
    if release is None or image is None:
        raise ValueError("Compose Start it commands must pin the release URL and image")
    if re.fullmatch(VERSION, image.group(2)) is None:
        raise ValueError("Compose image must have a valid runtime version")
    if release.group(1) != "v" + image.group(2):
        raise ValueError("Compose file and image release pins differ")
    return release.group(1), image.group(1)


def workspace_version(manifest):
    section = re.search(r"^\[workspace\.package\]\s*\n(.*?)(?=^\[|\Z)",
                        manifest, re.MULTILINE | re.DOTALL)
    if section is None:
        raise ValueError("Cargo.toml must declare workspace.package.version")
    version = re.search(r'^version\s*=\s*"([^"]+)"', section.group(1), re.MULTILINE)
    if version is None or re.fullmatch(VERSION, version.group(1)) is None:
        raise ValueError("Cargo.toml must declare workspace.package.version")
    return version.group(1)


def release_metadata(source_root, version):
    metadata_path = source_root / "server.json"
    try:
        metadata = json.loads(metadata_path.read_text())
    except FileNotFoundError as error:
        raise ValueError("server.json must exist when metadata validation is required") from error
    except json.JSONDecodeError as error:
        raise ValueError(f"server.json must contain valid JSON: {error.msg}") from error
    if not isinstance(metadata, dict):
        raise ValueError("server.json must contain an object")
    if metadata.get("name") != MCP_SERVER_NAME:
        raise ValueError(f"server.json name must be {MCP_SERVER_NAME}")
    if metadata.get("version") != version:
        raise ValueError("server.json version must match workspace.package.version")

    image = f"ghcr.io/{IMAGE_REPOSITORY}:{version}"
    packages = metadata.get("packages")
    if not isinstance(packages, list) or not any(
        package.get("registryType") == "oci"
        and package.get("identifier") == image
        and package.get("version") == version
        for package in packages if isinstance(package, dict)
    ):
        raise ValueError("server.json must declare the versioned runtime OCI package")

    dockerfile = (source_root / "Dockerfile").read_text()
    runtime = re.search(r"^FROM .* AS runtime\s*$\n(.*?)(?=^FROM |\Z)", dockerfile,
                        re.MULTILINE | re.DOTALL)
    if runtime is None:
        raise ValueError("Dockerfile must define a runtime stage")
    labels = runtime.group(1)
    if 'org.opencontainers.image.version="${VERSION}"' not in labels:
        raise ValueError("Dockerfile must label the image with VERSION")
    label = f'io.modelcontextprotocol.server.name="{MCP_SERVER_NAME}"'
    if label not in labels:
        raise ValueError("Dockerfile MCP server name label must match server.json")


def image_ready(version, current_version, require_published=False):
    if Registry().manifest_exists(IMAGE_REPOSITORY, version):
        return True
    # Release preparation advances the guide before its image exists. Only
    # that version may defer; the release job itself must exercise the image.
    if require_published or version != current_version:
        raise RuntimeError(f"Compose runtime image {version} has not been published")
    return False


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError) as error:
        print(f"Compose release check failed: {error}", file=sys.stderr)
        sys.exit(1)
