"""Publish the chart without replacing an existing version; verify anonymous access."""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import urllib.error
import urllib.parse
import urllib.request


CHART_REPOSITORY = "submilli/charts/submilli"
IMAGE_REPOSITORY = "submilli/submilli-runtime"
CHART_URL = "oci://ghcr.io/" + CHART_REPOSITORY
VERSION = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--chart", type=Path, default=Path("charts/submilli"))
    parser.add_argument("--check-image", action="store_true")
    parser.add_argument("--runtime-tag", default="")
    args = parser.parse_args()
    metadata = chart_metadata(args.chart)
    if args.runtime_tag and args.runtime_tag != "v" + metadata["appVersion"]:
        raise ValueError("Chart appVersion must match the runtime release tag")
    # The cluster install also checks the chart's actual default image. This
    # early check gives a useful error before waiting for ImagePullBackOff.
    if not Registry().manifest_exists(IMAGE_REPOSITORY, metadata["appVersion"]):
        raise RuntimeError("The chart's runtime image has not been published")
    if args.check_image:
        print("The runtime image is anonymously readable")
        return
    publish(args.chart, metadata["version"])


def chart_metadata(chart):
    output = subprocess.check_output(["helm", "show", "chart", str(chart)], text=True)
    fields = {}
    for field in ("name", "version", "appVersion"):
        match = re.search(r"^" + field + r":\s*([^\n]+)$", output, re.MULTILINE)
        if match is None:
            raise ValueError(f"Missing chart field: {field}")
        fields[field] = match.group(1).strip().strip("\"'")
    if fields["name"] != "submilli":
        raise ValueError("This publisher only accepts the submilli chart")
    for field in ("version", "appVersion"):
        if re.fullmatch(VERSION, fields[field]) is None:
            raise ValueError(f"Invalid {field}: {fields[field]}")
    return fields


def publish(chart, version):
    actor = os.environ["GITHUB_ACTOR"]
    credential = os.environ["GH_TOKEN"]
    with tempfile.TemporaryDirectory(prefix="submilli-chart-") as directory:
        root = Path(directory)
        env = registry_environment(root / "authenticated")
        subprocess.run(
            ["helm", "registry", "login", "ghcr.io", "--username", actor, "--password-stdin"],
            input=credential, text=True, env=env, check=True,
        )
        subprocess.run(["helm", "package", str(chart), "--destination", str(root)], check=True)
        archive = root / f"submilli-{version}.tgz"
        registry = Registry(actor, credential)
        if registry.manifest_exists(CHART_REPOSITORY, version):
            compare_remote(archive, version, root / "existing", env)
            print(f"Chart {version} already exists with identical content; leaving it unchanged")
        else:
            subprocess.run(["helm", "push", str(archive), "oci://ghcr.io/submilli/charts"],
                           env=env, check=True)
            compare_remote(archive, version, root / "uploaded", env)
        # Helm and Docker get empty credential stores. A private first upload
        # intentionally fails here; make that package public and rerun the job.
        try:
            compare_remote(archive, version, root / "public",
                           registry_environment(root / "anonymous"))
        except subprocess.CalledProcessError as error:
            raise RuntimeError(
                "Chart uploaded but anonymous pull failed. Make the charts/submilli "
                "container package public in GitHub package settings, then rerun the release workflow."
            ) from error
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as file:
            file.write(f"version={version}\n")


def registry_environment(directory):
    directory.mkdir(parents=True)
    return dict(os.environ, HELM_REGISTRY_CONFIG=str(directory / "helm.json"),
                DOCKER_CONFIG=str(directory))


def compare_remote(archive, version, destination, env):
    destination.mkdir()
    subprocess.run(["helm", "pull", CHART_URL, "--version", version,
                    "--destination", str(destination)], env=env, check=True)
    downloaded = destination / archive.name
    # gzip timestamps and tar entry order are not chart content. Compare paths,
    # permissions and bytes without extracting an untrusted archive.
    if archive_contents(archive) != archive_contents(downloaded):
        raise RuntimeError(f"Chart {version} has different content in GHCR; bump the chart version")


def archive_contents(path):
    contents = {}
    with tarfile.open(path, "r:gz") as archive:
        for entry in archive:
            if entry.isdir():
                continue
            if not entry.isfile() or entry.name in contents:
                raise ValueError(f"Unsupported or duplicate chart archive entry: {entry.name}")
            with archive.extractfile(entry) as file:
                contents[entry.name] = (entry.mode, hashlib.sha256(file.read()).hexdigest())
    return contents


class Registry:
    def __init__(self, actor=None, credential=None):
        self.actor = actor
        self.credential = credential

    def manifest_exists(self, repository, version):
        scope = "pull,push" if self.credential else "pull"
        query = urllib.parse.urlencode({"service": "ghcr.io", "scope": f"repository:{repository}:{scope}"})
        headers = {}
        if self.credential:
            basic = base64.b64encode(f"{self.actor}:{self.credential}".encode()).decode()
            headers["Authorization"] = "Basic " + basic
        token = request_json("https://ghcr.io/token?" + query, headers)["token"]
        if not isinstance(token, str) or not token:
            raise ValueError("GHCR returned an empty token")
        headers = {"Authorization": "Bearer " + token,
                   "Accept": "application/vnd.oci.image.manifest.v1+json, "
                             "application/vnd.oci.image.index.v1+json, "
                             "application/vnd.docker.distribution.manifest.list.v2+json"}
        tag = urllib.parse.quote(version.replace("+", "_"), safe="")
        url = f"https://ghcr.io/v2/{repository}/manifests/{tag}"
        return request_json(url, headers, missing_allowed=True) is not None


def request_json(url, headers, missing_allowed=False):
    request = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        if missing_allowed and error.code == 404:
            body = json.load(error)
            codes = {item.get("code") for item in body.get("errors", [])}
            if codes and codes <= {"MANIFEST_UNKNOWN", "NAME_UNKNOWN"}:
                return None
        # Authentication and service failures must never be interpreted as an
        # absent version, which would let a retry overwrite a published chart.
        raise RuntimeError(f"GHCR request failed (HTTP {error.code}); publication stopped") from error


if __name__ == "__main__":
    main()
