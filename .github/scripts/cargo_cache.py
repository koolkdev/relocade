"""Preserve Cargo source timestamps only when cached file contents match."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time


def tracked_files(root):
    paths = subprocess.check_output(["git", "ls-files", "-z"], cwd=root)
    return [Path(os.fsdecode(path)) for path in paths.split(b"\0") if path]


def fingerprint(path):
    if path.is_symlink() or not path.is_file():
        return None
    before = path.stat()
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    after = path.stat()
    if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
        return None
    return {"sha256": digest, "mtime_ns": after.st_mtime_ns}


def snapshot(root, paths, manifest):
    files = {}
    for relative in paths:
        entry = fingerprint(root / relative)
        if entry is not None:
            files[relative.as_posix()] = entry
    manifest.parent.mkdir(parents=True, exist_ok=True)
    temporary = manifest.with_suffix(".tmp")
    temporary.write_text(json.dumps({"version": 1, "files": files}) + "\n")
    temporary.replace(manifest)
    return len(files)


def restore(root, paths, manifest):
    target = root / "target"
    try:
        saved = json.loads(manifest.read_text())
        if saved["version"] != 1 or not isinstance(saved["files"], dict):
            raise ValueError("unsupported source metadata")
    except (OSError, ValueError, KeyError, TypeError):
        # Outputs without matching input metadata cannot establish freshness.
        if target.exists():
            shutil.rmtree(target)
        return 0
    # Checkout can precede a concurrently produced cache. Changed inputs must be
    # newer than every restored output, not merely newer than this checkout.
    newest_output = 0
    if target.exists():
        for output in target.rglob("*"):
            if output.is_file() and not output.is_symlink():
                newest_output = max(newest_output, output.stat().st_mtime_ns)
    changed_time = time.time_ns()
    if newest_output >= changed_time:
        shutil.rmtree(target)
        return 0
    restored = 0
    for relative in paths:
        # Only Git supplies paths. The cache manifest supplies metadata, never paths.
        path = root / relative
        cached = saved["files"].get(relative.as_posix())
        current = fingerprint(path)
        if current is None:
            # Symlink targets can lie outside the tracked input set.
            if path.is_symlink() and target.exists():
                shutil.rmtree(target)
            continue
        timestamp = cached.get("mtime_ns") if isinstance(cached, dict) else None
        if (type(timestamp) is int and 0 <= timestamp <= changed_time
                and current["sha256"] == cached.get("sha256")):
            os.utime(path, ns=(path.stat().st_atime_ns, timestamp))
            restored += 1
        else:
            os.utime(path, ns=(path.stat().st_atime_ns, changed_time))
    return restored


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["restore", "snapshot"])
    parser.add_argument("manifest", type=Path)
    args = parser.parse_args()
    root = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"]).decode().strip())
    operation = restore if args.operation == "restore" else snapshot
    count = operation(root, tracked_files(root), args.manifest)
    print(f"{args.operation}: {count} source timestamps")


if __name__ == "__main__":
    main()
