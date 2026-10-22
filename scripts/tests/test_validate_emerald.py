"""Synthetic structural Flash checks; no copyrighted assets required."""
import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import validate_emerald


def fixture(generations):
    data = bytearray([255] * 131072)
    for slot, counter in enumerate(generations):
        for section in range(14):
            struct.pack_into("<HHII", data, (slot * 14 + section) * 4096 + 0xFF4,
                             section, 0, 0x08012025, counter)
    return data


class FlashGenerations(unittest.TestCase):
    def test_complete_generations(self):
        self.assertEqual(validate_emerald.flash_generations(fixture([1, 2])), [1, 2])

    def test_invalid_size_or_no_headers(self):
        for data in [b"", bytes(131072), bytes(128)]:
            with self.assertRaises(ValueError):
                validate_emerald.flash_generations(data)

    def test_duplicate_or_missing_section_is_not_complete(self):
        data = fixture([1])
        struct.pack_into("<H", data, 0xFF4, 1)
        with self.assertRaises(ValueError):
            validate_emerald.flash_generations(data)

    def test_partial_new_generation_does_not_hide_complete_old(self):
        data = fixture([1, 2])
        struct.pack_into("<I", data, 14 * 4096 + 0xFF8, 0)
        self.assertEqual(validate_emerald.flash_generations(data), [1])

    def test_continuation_is_bounded_and_uses_only_keys(self):
        commands = validate_emerald.continuation()
        self.assertEqual(sum(int(line.split()[1]) for line in commands), 3200)
        self.assertTrue(all(line.startswith("run ") for line in commands))


if __name__ == "__main__":
    unittest.main()
