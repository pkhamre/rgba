"""Asset-free Cargo artifact discovery checks for optional Xvfb automation."""
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import validate_desktop


class ArtifactDiscovery(unittest.TestCase):
    def test_reads_actual_executable_not_guessed_target_directory(self):
        artifact = dict(reason="compiler-artifact",
                        target=dict(name="gba-desktop", kind=["bin"]),
                        executable="/custom/target/release/gba-desktop")
        text = "build noise\n{invalid JSON\n" + json.dumps(artifact)
        self.assertEqual(validate_desktop.artifact_executable(text),
                         Path("/custom/target/release/gba-desktop"))

    def test_rejects_missing_or_ambiguous_binaries(self):
        for text in ["", json.dumps(dict(reason="build-finished", success=True))]:
            with self.assertRaises(ValueError):
                validate_desktop.artifact_executable(text)


if __name__ == "__main__":
    unittest.main()
