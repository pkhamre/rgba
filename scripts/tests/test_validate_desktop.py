"""Asset-free Cargo artifact discovery checks for optional Xvfb automation."""
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import validate_desktop


class ScaledCapture(unittest.TestCase):
    def test_snapshot_reads_scaled_coordinates_and_native_byte_order(self):
        C = validate_desktop.C
        scale = 3
        stride = 240*scale*4+16
        for order in (0,1):
            data = C.create_string_buffer(stride*160*scale)
            offset = 68*scale*stride+92*scale*4
            encoded = (0x112233).to_bytes(4,"little" if order==0 else "big")
            C.memmove(C.addressof(data)+offset,encoded,4)
            image = validate_desktop.Image()
            image.data = C.addressof(data)
            image.stride = stride
            image.bpp = 32
            image.byte_order = order
            image.red_mask,image.green_mask,image.blue_mask = 0xff0000,0xff00,0xff

            class FakeX:
                geometry = None
                freed = False
                def XGetImage(self,*arguments):
                    self.geometry = arguments[4:6]
                    return C.addressof(image)
                def XDestroyImage(self,pointer):
                    self.freed = pointer==C.addressof(image)

            capture = object.__new__(validate_desktop.X11)
            capture.x = FakeX()
            capture.display = None
            capture.window = 123
            pixels = capture.snapshot(scale)
            self.assertEqual(capture.x.geometry,(720,480))
            self.assertTrue(capture.x.freed)
            self.assertEqual(len(pixels),240*160*3)
            start = (68*240+92)*3
            self.assertEqual(pixels[start:start+6],bytes([0x11,0x22,0x33,0,0,0]))


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
