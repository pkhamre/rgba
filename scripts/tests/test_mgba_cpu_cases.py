"""Asset-free checks for restricted mGBA table lowering and instruction encoding."""

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mgba_cpu_cases as cases


class MgbaCpuCasesTest(unittest.TestCase):
    def test_expected_values_are_relocated_not_executed(self):
        self.assertEqual(cases.value("ADDR(movPC) + 4", "movPC"), 0x0800010c)
        self.assertEqual(cases.value("-1"), 0xffffffff)
        self.assertEqual(cases.value("0xffffffff + 2"), 1)
        for expression in ["ADDR(other)", "__import__('os')", "1 * 2", "1 << 2", "True", "x"]:
            with self.assertRaises(ValueError, msg=expression):
                cases.value(expression, "movPC")

    def test_shifter_encodings_match_armv4t_fields(self):
        for name, opcode in {
            "movImm0": 0xe3b00000,
            "movImmFF": 0xe3b000ff,
            "movImmFF000000": 0xe3b004ff,
            "movImmF000000F": 0xe3b002ff,
            "movR1Lsl0": 0xe1b00001,
            "movR1LslR0": 0xe1b00011,
            "movPCLsr32": 0xe1b0002f,
            "movR1Rrx": 0xe1b00061,
            "addR0PCR1LslR0": 0xe09f0011,
        }.items():
            self.assertEqual(cases.shifter_opcode(name), opcode, name)
        for name in ["unknown", "movR1Bad", "movR1Lsl32"]:
            with self.assertRaises(ValueError):
                cases.shifter_opcode(name)

    def test_carry_expected_results_are_preserved(self):
        source = '''static const struct CarryTest carryTests[] = {
            { "example", { 1, 2, 0, 10, 11, 20, 21, 30, 31 } },
        };'''
        rows = list(cases.carry_cases(source))
        self.assertEqual(len(rows), 3)
        self.assertEqual([(row.out0, row.out_cpsr) for row in rows],
                         [(10, 11), (20, 21), (30, 31)])
        self.assertEqual([row.opcode for row in rows], [0xe0b02001, 0xe0d02001, 0xe0f02001])
        self.assertEqual((rows[0].in_r0, rows[0].in_r1), (1, 2))

    def test_multiply_high_and_low_words_are_not_swapped(self):
        source = '''static const struct MultiplyLongTest multiplyLongTests[] = {
            { "example", { 1, 2, 10, 11, 12, 20, 21, 22, }},
        };'''
        rows = list(cases.multiply_cases(source))
        self.assertEqual([(row.out_reg0, row.out0, row.out_reg1, row.out1, row.out_cpsr)
                          for row in rows], [(3, 10, 2, 11, 12), (3, 20, 2, 21, 22)])

    def test_shifter_pc_and_negative_shift_inputs(self):
        source = '''static const struct ShifterTest shifterTests[] = {
            { "example", movPCLslR0, 0, -1, 2, { ADDR(movPCLslR0) + 4, 0x1f } },
        };'''
        row, = cases.shifter_cases(source)
        self.assertEqual(row.in_r0, 0xffffffff)
        self.assertEqual(row.out0, 0x0800010c)
        self.assertEqual(row.out_cpsr, 0x1f)

    def test_bad_tables_fail_closed(self):
        for source in ["", "carryTests[] = {\nunsupported\n};",
                       'carryTests[] = {\n{ "example", { 1, 2, 3 } },\n};']:
            with self.assertRaises(ValueError):
                list(cases.carry_cases(source))
        sources = {"src/carry.c": "carryTests[] = {\n};",
                   "src/shifter.c": "shifterTests[] = {\n};",
                   "src/multiply-long.c": "multiplyLongTests[] = {\n};"}
        with self.assertRaises(ValueError):
            cases.build_cases(sources)

    def test_protocol_has_fixed_columns_and_safe_labels(self):
        case = cases.CpuCase("carry", 0, "example", 0xe0b02001, 0, 1, 2, 2, 3, None, None, 0x1f)
        self.assertEqual(len(case.tsv().split("\t")), len(cases.COLUMNS))
        self.assertEqual(case.tsv().split("\t")[-3:], ["-", "-", "0000001f"])
        case = cases.CpuCase("carry", 0, "bad\tlabel", 0, 0, 0, 0, 0, 0, None, None, 0)
        with self.assertRaises(ValueError):
            case.tsv()


if __name__ == "__main__":
    unittest.main()
