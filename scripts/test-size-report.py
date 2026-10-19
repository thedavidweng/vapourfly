#!/usr/bin/env python3
"""Small real-file fixtures; these are not native performance benchmarks."""
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest

spec = importlib.util.spec_from_file_location("size_report", Path(__file__).with_name("size-report.py"))
size_report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(size_report)


class WorkflowReportInvocationTests(unittest.TestCase):
    def report_block(self):
        workflow = (Path(__file__).resolve().parents[1] /
                    ".github/workflows/size-experiment.yml").read_text()
        # Execute the workflow's real argument construction without a native build.
        return textwrap.dedent(workflow.split("          PY\n", 1)[1].split(
            "            cmp Cargo.lock", 1)[0])

    def run_report(self, profile, report_function):
        bash = shutil.which("bash")
        self.assertIsNotNone(bash, "The workflow regression requires Bash")
        script = ('profile=$1; stage="size-experiment/$profile"; '
                  'target=fixture-target; GITHUB_SHA=fixture-revision\n' +
                  report_function + "\n" + self.report_block() + "\n")
        return subprocess.run([bash, "--noprofile", "--norc", "-euo", "pipefail",
                               "-c", script, "report-regression", profile],
                              capture_output=True)

    def test_baseline_and_candidate_preserve_argument_boundaries_with_nounset(self):
        for profile in ("release", "release-size"):
            with self.subTest(profile=profile):
                result = self.run_report(profile, 'python3() { printf "%s\\0" "$@"; }')
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                stage = f"size-experiment/{profile}"
                expected = ["scripts/size-report.py", "--stage", stage, "--asset", stage + ".zip",
                            "--target", "fixture-target", "--revision", "fixture-revision",
                            "--profile", profile, "--toolchain", "1.99"]
                if profile == "release-size":
                    expected += ["--baseline", "size-experiment/report-release.json"]
                expected += ["--out", f"size-experiment/report-{profile}"]
                self.assertEqual(result.stdout.split(b"\0")[:-1],
                                 [argument.encode() for argument in expected])

    def test_report_failure_stops_both_profiles(self):
        for profile in ("release", "release-size"):
            with self.subTest(profile=profile):
                result = self.run_report(profile, "python3() { return 23; }")
                self.assertEqual(result.returncode, 23, result.stderr.decode())


class SizeReportTests(unittest.TestCase):
    def test_bundle_and_assets_remain_separate_and_read_only(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stage = root / "package"
            paths = {"vapourfly": b"cli", "Vapourfly.app/Contents/MacOS/vapourfly-gui": b"gui!!",
                     "Vapourfly.app/Contents/Frameworks/example.dylib": b"lib!",
                     "Vapourfly.app/Contents/Resources/icon.icns": b"icon!!", "LICENSE": b"oss"}
            for name, content in paths.items():
                path = stage / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(content)
            asset = root / "package.tar.gz"
            asset.write_bytes(b"compressed")
            result = size_report.report(stage, [asset], {})
            self.assertEqual(result["categories"], {"cli": 3, "gui": 5, "libraries": 4,
                                                     "resources": 6, "other": 3})
            self.assertEqual(result["stage_bytes"], 21)
            self.assertEqual(result["bundles"], {"Vapourfly.app": 15})
            self.assertEqual(result["assets"][0]["bytes"], 10)
            self.assertEqual(len(result["files"]), 5)
            for name, content in paths.items():
                self.assertEqual((stage / name).read_bytes(), content)

    def test_windows_executables_and_versioned_linux_libraries(self):
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory)
            for name in ["vapourfly.exe", "vapourfly-gui.exe", "libexample.so.1", "example.dll"]:
                (stage / name).write_bytes(b"x")
            result = size_report.report(stage, [], {})
            self.assertEqual(result["categories"], {"cli": 1, "gui": 1, "libraries": 2,
                                                     "resources": 0, "other": 0})

    def test_symlink_is_counted_without_reading_its_external_target(self):
        with tempfile.TemporaryDirectory() as directory:
            stage = Path(directory) / "stage"
            stage.mkdir()
            outside = Path(directory) / "outside"
            outside.mkdir()
            (outside / "secret").write_bytes(b"not part of the bundle")
            try:
                (stage / "link").symlink_to(outside, target_is_directory=True)
            except OSError as error:
                self.skipTest(f"OS does not permit fixture symlinks: {error}")
            records = size_report.inventory(stage)
            self.assertEqual(len(records), 1)
            self.assertEqual(records[0]["kind"], "symlink")
            self.assertNotIn("sha256", records[0])

    def test_baseline_delta_requires_identical_measurement_inputs(self):
        metadata = {"schema": 1, "target": "test", "rustc": "version", "lock_sha256": "lock",
                    "profile": "release", "profile_config": {"opt-level": 3},
                    "categories": {"cli": 10}, "stage_bytes": 10}
        current = {**metadata, "categories": {"cli": 8}, "stage_bytes": 8}
        comparable, rows = size_report.comparison(current, metadata)
        self.assertTrue(comparable)
        self.assertEqual(rows[0], ("cli", 10, 8, -2))
        for field in ["schema", "target", "rustc", "lock_sha256"]:
            comparable, rows = size_report.comparison({**current, field: "different"}, metadata)
            self.assertFalse(comparable)
            self.assertIsNone(rows[0][3])
        comparable, _ = size_report.comparison(
            {**current, "profile": "size", "profile_config": {"opt-level": "s"}}, metadata)
        self.assertTrue(comparable)
        comparable, rows = size_report.comparison(current, None)
        self.assertFalse(comparable)
        self.assertIsNone(rows[0][1])

    def test_missing_stage_or_asset_is_not_reported_as_zero(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(ValueError):
                size_report.inventory(root / "missing")
            with self.assertRaises(FileNotFoundError):
                size_report.report(root, [root / "missing.tar.gz"], {})

    def test_command_writes_json_and_markdown_with_actual_toolchain(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stage = root / "stage"
            stage.mkdir()
            (stage / "vapourfly").write_bytes(b"fixture, not a release executable")
            repository = Path(__file__).resolve().parents[1]
            result = subprocess.run([
                sys.executable, str(Path(__file__).with_name("size-report.py")),
                "--stage", str(stage), "--target", "fixture", "--revision", "fixture",
                "--toolchain", "1.99", "--manifest", str(repository / "Cargo.toml"),
                "--lockfile", str(repository / "Cargo.lock"), "--out", str(root / "report")],
                capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads((root / "report.json").read_text())
            self.assertEqual(report["categories"]["cli"], (stage / "vapourfly").stat().st_size)
            self.assertIn("rustc 1.99", report["rustc"])
            self.assertEqual((root / "report.md").read_text(), result.stdout)
            self.assertIn("unavailable", result.stdout)
            self.assertNotIn(str(root), (root / "report.json").read_text())


if __name__ == "__main__":
    unittest.main()
