#!/usr/bin/env python3
"""Prepare pinned test fixtures using Python 3.11+ and its standard library."""

import filecmp
import hashlib
from pathlib import Path
import shutil
import sys
import tempfile
import tomllib
import urllib.error
import urllib.request
import zipfile


ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / "target" / "test-assets"


def verify_archive(path, expected):
    with path.open("rb") as archive:
        actual = hashlib.file_digest(archive, "sha256").hexdigest()
    if actual != expected:
        raise ValueError(
            f"{path}: SHA-256 mismatch: expected {expected}, got {actual}"
        )


def fetch_archive(asset):
    CACHE.mkdir(parents=True, exist_ok=True)
    archive = CACHE / f"{asset['sha256']}.zip"
    if archive.exists():
        verify_archive(archive, asset["sha256"])
        return archive

    print(f"Downloading {asset['name']} from {asset['url']}", flush=True)
    with tempfile.TemporaryDirectory(dir=CACHE) as temporary:
        download = Path(temporary) / "download.zip"
        with urllib.request.urlopen(asset["url"], timeout=30) as response:
            with download.open("wb") as output:
                shutil.copyfileobj(response, output)
        verify_archive(download, asset["sha256"])
        download.replace(archive)
    return archive


def prepare_asset(asset):
    archive = fetch_archive(asset)
    directory = ROOT / asset["directory"]
    directory.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive) as package:
        with tempfile.TemporaryDirectory(dir=directory) as temporary:
            for name, member in asset["files"].items():
                extracted = Path(temporary) / name
                extracted.parent.mkdir(parents=True, exist_ok=True)
                with package.open(member) as source, extracted.open("wb") as output:
                    shutil.copyfileobj(source, output)
                destination = directory / name
                # Preserve timestamps so repeated preparation does not rebuild tests.
                if not destination.exists() or not filecmp.cmp(
                    extracted, destination, shallow=False
                ):
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    extracted.replace(destination)
    print(f"Prepared {asset['name']} ({asset['version']})", flush=True)


def main():
    with (ROOT / "test-assets.toml").open("rb") as manifest:
        assets = tomllib.load(manifest)["assets"]
    for asset in assets:
        prepare_asset(asset)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, urllib.error.URLError, zipfile.BadZipFile) as error:
        sys.exit(f"Test asset preparation failed: {error}")
