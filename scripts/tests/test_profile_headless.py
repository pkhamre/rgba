"""Asset-free profiler orchestration checks; not real perf/hardware validation."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class ProfileHeadlessTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="rgba-profile-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "scripts").mkdir()
        shutil.copy(ROOT / "scripts/profile-headless.sh", self.root / "scripts")
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.output = self.root / "profile with spaces"
        self.binary = self.root / "headless with spaces"
        self.binary.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\"\n", encoding="utf-8")
        self.binary.chmod(0o755)
        message = json.dumps({
            "reason": "compiler-artifact", "target": {"name": "gba-headless"},
            "executable": str(self.binary),
        })
        (self.root / "scripts/cargo.sh").write_text(
            "#!/bin/sh\n[ \"${FAIL_BUILD:-0}\" = 0 ] || exit 7\n"
            f"printf '%s\\n' '{message}'\n", encoding="utf-8",
        )
        perf = self.bin / "perf"
        perf.write_text(
            "#!/bin/sh\n"
            "[ \"${FAIL_PERF:-0}\" = 0 ] || exit 9\n"
            "case \"$1\" in\n"
            "record) while [ \"$1\" != -- ]; do shift; done; shift; exec \"$@\" ;;\n"
            "report) echo 'synthetic call graph' ;;\n"
            "*) exit 3 ;;\nesac\n", encoding="utf-8",
        )
        perf.chmod(0o755)

    def run_profile(self, **extra_env):
        env = {**os.environ, "PATH": f"{self.bin}:{os.environ['PATH']}", **extra_env}
        return subprocess.run(
            ["sh", str(self.root / "scripts/profile-headless.sh"), str(self.output),
             "--bios", "bios with spaces", "--rom", "game.gba", "--frames", "10"],
            env=env, cwd=self.root, capture_output=True, text=True, check=False,
        )

    def test_records_workload_and_generates_report(self):
        result = self.run_profile()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            (self.output / "benchmark.log").read_text().splitlines(),
            ["--benchmark", "--bios", "bios with spaces", "--rom", "game.gba", "--frames", "10"],
        )
        self.assertIn("synthetic call graph", (self.output / "report.txt").read_text())
        self.assertEqual(self.output.stat().st_mode & 0o777, 0o700)
        self.assertNotEqual(self.run_profile().returncode, 0, "must not overwrite output")

    def test_build_failure_is_terminal(self):
        result = self.run_profile(FAIL_BUILD="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.output / "benchmark.log").exists())
        self.assertFalse((self.output / "report.txt").exists())

    def test_perf_failure_is_terminal(self):
        result = self.run_profile(FAIL_PERF="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Profiling failed", result.stderr)
        self.assertFalse((self.output / "report.txt").exists())


if __name__ == "__main__":
    unittest.main()
