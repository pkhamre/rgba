#!/usr/bin/env python3
"""Categorize every strict reference mismatch; never change or skip test outcomes."""

import argparse
from collections import Counter, defaultdict
import csv
import json
from pathlib import Path
import re

import inspect_cpu_vector as vectors


def classify(group, case, detail):
    opcode = case["opcode"]
    if group.startswith("arm_data_proc") and (opcode >> 12) & 15 == 15:
        operation = (opcode >> 21) & 15
        if 8 <= operation <= 11 and case["initial"]["CPSR"] & 31 in {0x10, 0x1f}:
            return "legacy_psr_test_without_spsr"
    if group == "arm_ldm_stm":
        user_transfer = opcode & (1 << 22) and not (opcode & (1 << 20) and opcode & 0x8000)
        if user_transfer and opcode & (1 << 21):
            return "unsupported_user_bank_writeback"
    if group == "arm_msr_reg":
        match = re.search(r"CPSR expected ([0-9a-f]+), got ([0-9a-f]+)", detail)
        if match:
            expected, actual = (int(value, 16) for value in match.groups())
            if expected ^ actual == 0x20:
                return "msr_thumb_bit_write_difference"
    if group == "thumb_undefined_bcc" and opcode & 0xff00 == 0xde00:
        offset = opcode & 0xff
        if offset & 0x80:
            offset -= 256
        expected_pc = (case["initial"]["R"][15] + offset * 2 + 4) & 0xffffffff
        if case["final"]["R"][15] == expected_pc and case["final"]["CPSR"] == case["initial"]["CPSR"]:
            return "reserved_thumb_condition_reference_branches"
    if detail.startswith("reserved processor mode"):
        return "reserved_mode_diagnostic"
    if group == "thumb_bx" and opcode & 0x80:
        return "reserved_thumb_bx_encoding"
    if group in {"arm_mul_mla", "arm_mull_mlal", "arm_swp", "arm_mrs"}:
        registers = []
        if group == "arm_mul_mla":
            registers = [opcode & 15, (opcode >> 8) & 15, (opcode >> 16) & 15]
            if opcode & (1 << 21):
                registers.append((opcode >> 12) & 15)
        elif group == "arm_mull_mlal":
            registers = [opcode & 15, (opcode >> 8) & 15,
                         (opcode >> 12) & 15, (opcode >> 16) & 15]
        elif group == "arm_swp":
            registers = [opcode & 15, (opcode >> 12) & 15, (opcode >> 16) & 15]
        elif group == "arm_mrs":
            registers = [(opcode >> 12) & 15]
        if 15 in registers:
            return "forbidden_r15_operand"
        if group == "arm_mul_mla" and (opcode & 15) == ((opcode >> 16) & 15):
            return "forbidden_multiply_overlap"
        if group == "arm_mull_mlal":
            distinct = [opcode & 15, (opcode >> 12) & 15, (opcode >> 16) & 15]
            if len(set(distinct)) != 3:
                return "forbidden_multiply_overlap"
    transfer_groups = {
        "arm_ldr_str_immediate_offset", "arm_ldr_str_register_offset",
        "arm_ldrh_strh", "arm_ldrsb_ldrsh", "arm_ldm_stm",
    }
    if group in transfer_groups and (opcode >> 16) & 15 == 15:
        writeback = opcode & (1 << 21) or (group != "arm_ldm_stm" and not opcode & (1 << 24))
        if writeback:
            return "forbidden_pc_base_writeback"
    match = re.search(r"registers expected \[([^]]+)\], got \[([^]]+)\]", detail)
    if match:
        expected, actual = ([int(value, 16) for value in part.split(", ")]
                            for part in match.groups())
        mask = ~1 if case["final"]["CPSR"] & 0x20 else ~3
        if expected[:15] == actual[:15] and expected[15] & mask == actual[15]:
            return "pc_snapshot_alignment_difference"
    return "unclassified"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path, help="TSV written by GBA_VECTOR_REPORT")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1] / "tests/assets/arm7tdmi/v1"
    with args.report.open(newline="") as report:
        grouped = defaultdict(list)
        for row in csv.DictReader(report, delimiter="\t"):
            grouped[row["group"]].append((int(row["index"]), row["detail"]))
    categories = Counter()
    groups = {}
    examples = {}
    for filename, failures in grouped.items():
        data = (root / filename).read_bytes()
        offset = 8
        offsets = []
        for _ in range(vectors.word(data, 4)):
            offsets.append(offset)
            offset += vectors.word(data, offset)
        counts = Counter()
        group = filename.removesuffix(".json.bin")
        for index, detail in failures:
            case = vectors.decode_at(data, offsets[index], index)
            category = classify(group, case, detail)
            counts[category] += 1
            examples.setdefault(category, {"group": group, "index": index, "detail": detail})
        groups[group] = dict(counts)
        categories.update(counts)
    print(json.dumps({"total_failures": sum(categories.values()), "categories": dict(categories),
                      "groups": groups, "first_examples": examples}, indent=2))


if __name__ == "__main__":
    main()
