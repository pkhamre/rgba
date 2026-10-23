"""Asset-free tests for the Linux release package allowlist and checksums."""

import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "package-linux.sh"
TARGET = "x86_64-unknown-linux-musl"


class PackageLinuxTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "scripts").mkdir()
        shutil.copyfile(SCRIPT, self.root / "scripts/package-linux.sh")
        (self.root / "scripts/cargo.sh").write_text(
            '#!/bin/sh\n[ "$*" = "build --locked --release --target '
            f'{TARGET} -p gba-desktop" ] || exit 99\n'
            'exit "${FAKE_BUILD_EXIT:-0}"\n'
        )
        self.target_dir = self.root / "build"
        binary = self.target_dir / TARGET / "release/gba-desktop"
        binary.parent.mkdir(parents=True)
        binary.write_text('#!/bin/sh\n[ "$1" = "--help" ]\n')
        binary.chmod(0o755)
        for directory in ("docs", "LICENSES", "bins"):
            (self.root / directory).mkdir()
        for name in ("README.md", "LICENSE", "docs/TESTING.md", "LICENSES/notice.txt",
                     "bins/private.gba", "bins/bios.bin", "private.sav"):
            (self.root / name).write_text(name)
        self.environment = {**os.environ, "CARGO_TARGET_DIR": str(self.target_dir)}

    def run_package(self, *arguments):
        return subprocess.run(
            ["sh", str(self.root / "scripts/package-linux.sh"), *arguments],
            env=self.environment, capture_output=True, text=True, check=False,
        )

    def test_archive_has_only_binary_docs_and_notices_with_valid_checksum(self):
        result = self.run_package("v0.1.0-rc.1")
        self.assertEqual(result.returncode, 0, result.stderr)
        name = f"rgba-v0.1.0-rc.1-{TARGET}"
        archive = self.root / "artifacts/release" / f"{name}.tar.gz"
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        self.assertEqual(
            archive.with_suffix(".gz.sha256").read_text(), f"{digest}  {archive.name}\n"
        )
        with tarfile.open(archive) as package:
            self.assertEqual(
                {member.name for member in package if member.isfile()},
                {f"{name}/{path}" for path in (
                    "rgba", "README.md", "LICENSE", "docs/TESTING.md", "LICENSES/notice.txt"
                )},
            )
            self.assertEqual(package.getmember(f"{name}/rgba").mode & 0o777, 0o755)

    def test_invalid_tags_and_wrong_argument_count_fail_before_build(self):
        for arguments in ((), ("v1", "v2"), ("1.0.0",), ("v1/../../bad",),
                          ("v1;echo bad",), ("v1\nbad",)):
            with self.subTest(arguments=arguments):
                self.assertEqual(self.run_package(*arguments).returncode, 2)
        self.assertFalse((self.root / "artifacts").exists())

    def test_build_failure_does_not_produce_a_package(self):
        self.environment["FAKE_BUILD_EXIT"] = "42"
        self.assertEqual(self.run_package("v0.1.0").returncode, 42)
        self.assertFalse((self.root / "artifacts").exists())


if __name__ == "__main__":
    unittest.main()
