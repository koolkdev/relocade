import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest

from cargo_cache import restore, snapshot


class SourceTimestamps(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.source = self.root / "source.rs"
        self.source.write_text("fn value() -> u32 { 1 }\n")
        self.old_time = 1_700_000_000_123_456_789
        self.new_time = self.old_time + 100_000_000_000
        os.utime(self.source, ns=(self.old_time, self.old_time))
        self.manifest = self.root / "cache/sources.json"
        self.paths = [Path("source.rs")]
        snapshot(self.root, self.paths, self.manifest)

    def fresh_checkout_time(self):
        os.utime(self.source, ns=(self.new_time, self.new_time))

    def test_unchanged_contents_restore_original_timestamp(self):
        self.fresh_checkout_time()
        self.assertEqual(restore(self.root, self.paths, self.manifest), 1)
        self.assertEqual(self.source.stat().st_mtime_ns, self.old_time)

    def test_same_size_edit_gets_a_new_timestamp(self):
        self.source.write_text("fn value() -> u32 { 2 }\n")
        self.fresh_checkout_time()
        self.assertEqual(restore(self.root, self.paths, self.manifest), 0)
        self.assertGreater(self.source.stat().st_mtime_ns, self.new_time)

    def test_new_and_deleted_files_are_not_restored(self):
        self.source.unlink()
        new = self.root / "new.rs"
        new.write_text("fn new() {}\n")
        os.utime(new, ns=(self.new_time, self.new_time))
        self.assertEqual(restore(self.root, self.paths + [Path("new.rs")], self.manifest), 0)
        self.assertGreater(new.stat().st_mtime_ns, self.new_time)

    def test_edit_is_newer_than_outputs_created_after_checkout(self):
        self.source.write_text("fn value() -> u32 { 2 }\n")
        self.fresh_checkout_time()
        output = self.root / "target/artifact"
        output.parent.mkdir()
        output.write_text("compiled before this cache was uploaded")
        output_time = time.time_ns() - 1_000_000
        os.utime(output, ns=(output_time, output_time))
        self.assertEqual(restore(self.root, self.paths, self.manifest), 0)
        self.assertGreater(self.source.stat().st_mtime_ns, output.stat().st_mtime_ns)

    def test_outputs_from_a_future_clock_are_discarded(self):
        self.fresh_checkout_time()
        output = self.root / "target/artifact"
        output.parent.mkdir()
        output.write_text("future output")
        timestamp = time.time_ns() + 10_000_000_000
        os.utime(output, ns=(timestamp, timestamp))
        self.assertEqual(restore(self.root, self.paths, self.manifest), 0)
        self.assertFalse(output.parent.exists())
        self.assertEqual(self.source.stat().st_mtime_ns, self.new_time)

    def test_symlinks_do_not_change_target_timestamps(self):
        target = self.root / "target.rs"
        self.source.rename(target)
        self.source.symlink_to(target)
        os.utime(target, ns=(self.new_time, self.new_time))
        self.assertEqual(restore(self.root, self.paths, self.manifest), 0)
        self.assertEqual(target.stat().st_mtime_ns, self.new_time)

    def test_symlink_inputs_discard_cached_outputs(self):
        self.source.unlink()
        self.source.symlink_to(self.root / "external.rs")
        outputs = self.root / "target"
        outputs.mkdir()
        (outputs / "artifact").write_text("old")
        restore(self.root, self.paths, self.manifest)
        self.assertFalse(outputs.exists())

    def test_missing_invalid_and_unknown_manifests_leave_sources_fresh(self):
        self.fresh_checkout_time()
        for content in [None, "invalid JSON", "[]", json.dumps({"version": 2, "files": {}})]:
            with self.subTest(content=content):
                outputs = self.root / "target"
                outputs.mkdir()
                (outputs / "artifact").write_text("unverified")
                if content is None:
                    self.manifest.unlink(missing_ok=True)
                else:
                    self.manifest.write_text(content)
                self.assertEqual(restore(self.root, self.paths, self.manifest), 0)
                self.assertEqual(self.source.stat().st_mtime_ns, self.new_time)
                self.assertFalse(outputs.exists())


@unittest.skipUnless(shutil.which("cargo"), "Cargo is required for the integration test")
class CargoFreshness(unittest.TestCase):
    def test_fresh_checkout_reuses_artifacts_but_a_real_edit_rebuilds(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            (root / "Cargo.toml").write_text(
                '[package]\nname="cache-probe"\nversion="0.0.0"\nedition="2021"\n'
            )
            (root / "build.rs").write_text(
                'fn main() { println!("cargo:rerun-if-changed=build.rs"); '
                'std::fs::write(std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())'
                '.join("generated.rs"), "const GENERATED: u32 = 7;").unwrap(); }\n'
            )
            source = root / "src/lib.rs"
            code = (
                'include!(concat!(env!("OUT_DIR"), "/generated.rs"));\n'
                'fn value() -> u32 { GENERATED + 1 }\n'
                '#[test] fn check() { assert_eq!(value(), '
                'std::env::var("EXPECTED_VALUE").unwrap().parse::<u32>().unwrap()); }\n'
            )
            source.write_text(code)
            paths = [Path("Cargo.toml"), Path("build.rs"), Path("src/lib.rs")]
            manifest = root / "cache/source-mtimes.json"
            env = os.environ.copy()
            env.update(CARGO_TARGET_DIR=str(root / "target"), CARGO_INCREMENTAL="0")

            def build(expected):
                env["EXPECTED_VALUE"] = str(expected)
                result = subprocess.run(
                    ["cargo", "test", "--offline", "--message-format=json"],
                    cwd=root, env=env, text=True, capture_output=True,
                )
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                artifacts = [json.loads(line) for line in result.stdout.splitlines()
                             if line.startswith("{")]
                fresh = [item["fresh"] for item in artifacts
                         if item.get("reason") == "compiler-artifact"]
                self.assertTrue(fresh)
                return all(fresh)

            snapshot(root, paths, manifest)
            self.assertFalse(build(8))
            for path in paths:
                os.utime(root / path, None)
            restore(root, paths, manifest)
            self.assertTrue(build(8))

            old_time = source.stat().st_mtime_ns
            source.write_text(code.replace("GENERATED + 1", "GENERATED + 2"))
            os.utime(source, ns=(old_time, old_time))
            restore(root, paths, manifest)
            snapshot(root, paths, manifest)
            self.assertFalse(build(9))
            for path in paths:
                os.utime(root / path, None)
            restore(root, paths, manifest)
            self.assertTrue(build(9))


if __name__ == "__main__":
    unittest.main()
