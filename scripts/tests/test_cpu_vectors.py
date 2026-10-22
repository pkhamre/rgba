"""Asset-free tests for vector inspection and failure classification."""

from pathlib import Path
import struct
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import inspect_cpu_vector as vectors
import triage_cpu_vectors as triage


class CpuVectorToolsTest(unittest.TestCase):
    def test_decodes_a_synthetic_record(self):
        initial = list(range(40))
        final = list(range(100, 140))
        initial_block = struct.pack("<II40I", 168, 1, *initial)
        final_block = struct.pack("<II40I", 168, 2, *final)
        transactions = struct.pack("<III6I", 36, 3, 1, 0, 4, 8, 0x1234, 1, 3)
        opcode = struct.pack("<IIII", 16, 4, 0xe1a00000, 0)
        payload = initial_block + final_block + transactions + opcode
        data = struct.pack("<III", 0xd33dbae0, 1, len(payload) + 4) + payload
        case = vectors.decode(data, 0)
        self.assertEqual(case["opcode"], 0xe1a00000)
        self.assertEqual(case["initial"]["R"], tuple(range(16)))
        self.assertEqual(case["final"]["CPSR"], 131)
        self.assertEqual(case["transactions"][0]["access"], 3)
        with self.assertRaises(ValueError):
            vectors.decode(data, 1)
        with self.assertRaises(ValueError):
            vectors.decode(data, -1)

    def test_rejects_invalid_header(self):
        with self.assertRaises(ValueError):
            vectors.decode(b"", 0)
        with self.assertRaises(ValueError):
            vectors.decode(struct.pack("<II", 0, 1), 0)

    def test_msr_thumb_difference_does_not_hide_other_flag_changes(self):
        case = {"opcode": 0x0121f000}
        self.assertEqual(triage.classify("arm_msr_reg", case,
                         "CPSR expected 50000033, got 50000013"),
                         "msr_thumb_bit_write_difference")
        self.assertEqual(triage.classify("arm_msr_reg", case,
                         "CPSR expected 60000033, got 50000013"), "unclassified")

    def test_pc_snapshot_category_requires_other_registers_to_match(self):
        expected = [0] * 15 + [0x1237]
        actual = [0] * 15 + [0x1234]
        case = {"opcode": 0xe12fff10, "final": {"CPSR": 0x1f}}
        def detail():
            return "registers expected [" + ", ".join(f"{x:08x}" for x in expected) + "], got [" + ", ".join(f"{x:08x}" for x in actual) + "]"
        self.assertEqual(triage.classify("arm_bx", case, detail()),
                         "pc_snapshot_alignment_difference")
        actual[0] = 1
        self.assertEqual(triage.classify("arm_bx", case, detail()), "unclassified")

    def test_exception_return_is_not_a_user_bank_writeback(self):
        case = {"opcode": 0xe8f08001}
        self.assertEqual(triage.classify("arm_ldm_stm", case, "some mismatch"),
                         "unclassified")

    def test_reserved_condition_category_checks_reference_branch_target(self):
        registers = [0] * 16
        registers[15] = 0x1004
        final = registers.copy()
        final[15] = 0x1006  # DEFF branches by -2, then exposes PC+4.
        case = {"opcode": 0xdeff,
                "initial": {"R": registers, "CPSR": 0x3f},
                "final": {"R": final, "CPSR": 0x3f}}
        self.assertEqual(triage.classify("thumb_undefined_bcc", case, "transaction mismatch"),
                         "reserved_thumb_condition_reference_branches")
        final[15] = 0x1008
        self.assertEqual(triage.classify("thumb_undefined_bcc", case, "transaction mismatch"),
                         "unclassified")


if __name__ == "__main__":
    unittest.main()
