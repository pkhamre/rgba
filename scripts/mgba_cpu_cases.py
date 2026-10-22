"""Lower pinned mGBA suite CPU tables into isolated, relocatable ARM test cases.

This is not a C compiler or a general assembler. Expected values come from the
upstream tables, never from executing or evaluating the emulator under test.
"""

import ast
from dataclasses import dataclass
import re
from typing import Optional

INSTRUCTION_ADDRESS = 0x08000100
SCHEMA = "gba-cpu-cases-v1"
COLUMNS = ("suite", "index", "name", "opcode", "in_cpsr", "in_r0", "in_r1",
           "out_reg0", "out0", "out_reg1", "out1", "out_cpsr")


@dataclass(frozen=True)
class CpuCase:
    suite: str
    index: int
    name: str
    opcode: int
    in_cpsr: int
    in_r0: int
    in_r1: int
    out_reg0: int
    out0: int
    out_reg1: Optional[int]
    out1: Optional[int]
    out_cpsr: int

    def tsv(self):
        if any(character in self.name for character in "\t\r\n"):
            raise ValueError("Case name is not safe for TSV")
        numbers = [self.opcode, self.in_cpsr, self.in_r0, self.in_r1]
        fields = [self.suite, str(self.index), self.name]
        fields += [f"{value:08x}" for value in numbers]
        fields += [str(self.out_reg0), f"{self.out0:08x}"]
        fields += (["-", "-"] if self.out_reg1 is None
                   else [str(self.out_reg1), f"{self.out1:08x}"])
        fields.append(f"{self.out_cpsr:08x}")
        return "\t".join(fields)


def value(expression, function=None):
    """Evaluate only C-table integer literals, +/- and the selected ADDR symbol."""
    def visit(node):
        if isinstance(node, ast.Constant) and type(node.value) is int:
            return node.value
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, (ast.UAdd, ast.USub)):
            operand = visit(node.operand)
            return -operand if isinstance(node.op, ast.USub) else operand
        if isinstance(node, ast.BinOp) and isinstance(node.op, (ast.Add, ast.Sub)):
            left, right = visit(node.left), visit(node.right)
            return left + right if isinstance(node.op, ast.Add) else left - right
        if (isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
                and node.func.id == "ADDR" and len(node.args) == 1 and not node.keywords
                and isinstance(node.args[0], ast.Name) and function is not None
                and node.args[0].id == function):
            return INSTRUCTION_ADDRESS + 8
        raise ValueError(f"Unsupported expected-value expression: {expression}")
    try:
        return visit(ast.parse(expression.strip(), mode="eval").body) & 0xffffffff
    except SyntaxError as error:
        raise ValueError(f"Invalid integer expression: {expression}") from error


def table(source, name):
    match = re.search(r"\b" + re.escape(name) + r"\[\]\s*=\s*\{(.*?)\n\s*\};", source, re.S)
    if match is None:
        raise ValueError(f"Missing pinned table {name}")
    return match.group(1)


def numeric_rows(source, name, fields):
    body = table(source, name)
    pattern = r'\{\s*"([^"\\]*)"\s*,\s*\{([^{}]*)\}\s*\}\s*,?'
    rows = list(re.finditer(pattern, body, re.S))
    remainder = re.sub(pattern, "", body, flags=re.S)
    if remainder.strip():
        raise ValueError(f"Unparsed content in {name}")
    result = []
    for row in rows:
        raw = row.group(2).strip().rstrip(",")
        numbers = [value(item) for item in raw.split(",")]
        if len(numbers) not in fields:
            raise ValueError(f"Unexpected field count in {name}: {len(numbers)}")
        result.append((row.group(1), numbers))
    return result


def carry_cases(source):
    rows = numeric_rows(source, "carryTests", {9})
    for index, (name, row) in enumerate(rows):
        for variant, (mnemonic, opcode) in enumerate([
                ("adcs", 0xe0b02001), ("sbcs", 0xe0d02001), ("rscs", 0xe0f02001)]):
            yield CpuCase("carry", index * 3 + variant, f"{name}/{mnemonic}",
                          opcode, row[2], row[0], row[1], 2, row[3 + 2 * variant],
                          None, None, row[4 + 2 * variant])


def multiply_cases(source):
    rows = numeric_rows(source, "multiplyLongTests", {8})
    for index, (name, row) in enumerate(rows):
        for variant, (mnemonic, opcode) in enumerate([
                ("smulls", 0xe0d32190), ("umulls", 0xe0932190)]):
            # Upstream stores the high word before the low word. Registers are
            # renamed to r3:r2, with independent, non-overlapping r0/r1 inputs.
            yield CpuCase("multiply-long", index * 2 + variant, f"{name}/{mnemonic}",
                          opcode, 0, row[0], row[1], 3, row[2 + 3 * variant],
                          2, row[3 + 3 * variant], row[4 + 3 * variant])


def shifter_opcode(function):
    immediates = {"movImm0": (0, 0), "movImmFF": (0xff, 0),
                  "movImmFF000000": (0xff, 8), "movImmF000000F": (0xff, 4)}
    if function in immediates:
        immediate, rotation = immediates[function]
        return 0xe3b00000 | ((rotation // 2) << 8) | immediate
    prefixes = {"movR1": 0xe1b00001, "movPC": 0xe1b0000f, "addR0PCR1": 0xe09f0001}
    for prefix, opcode in prefixes.items():
        if not function.startswith(prefix):
            continue
        suffix = function[len(prefix):]
        if not suffix:
            return opcode
        if suffix == "Rrx":
            return opcode | (3 << 5)
        match = re.fullmatch(r"(Lsl|Lsr|Asr|Ror)(0|1|31|32|R0)", suffix)
        if match is None:
            raise ValueError(f"Unknown shifter function {function}")
        kind = {"Lsl": 0, "Lsr": 1, "Asr": 2, "Ror": 3}[match.group(1)]
        amount = match.group(2)
        if amount == "R0":
            return opcode | (kind << 5) | 0x10
        amount = int(amount)
        if amount == 32:
            if kind not in {1, 2}:
                raise ValueError(f"Invalid immediate shift in {function}")
            amount = 0
        return opcode | (kind << 5) | (amount << 7)
    raise ValueError(f"Unknown shifter function {function}")


def shifter_cases(source):
    body = table(source, "shifterTests")
    pattern = (r'\{\s*"([^"\\]*)"\s*,\s*(\w+)\s*,\s*([^,]+),\s*([^,]+),'
               r'\s*([^,]+),\s*\{([^,]+),\s*([^}]+)\}\s*\}\s*,?')
    rows = list(re.finditer(pattern, body))
    if re.sub(pattern, "", body).strip():
        raise ValueError("Unparsed content in shifterTests")
    for index, row in enumerate(rows):
        name, function, cpsr, r0, r1, out, out_cpsr = row.groups()
        yield CpuCase("shifter", index, name, shifter_opcode(function),
                      value(cpsr), value(r0), value(r1), 0, value(out, function),
                      None, None, value(out_cpsr))


def build_cases(sources):
    cases = list(carry_cases(sources["src/carry.c"]))
    cases += list(shifter_cases(sources["src/shifter.c"]))
    cases += list(multiply_cases(sources["src/multiply-long.c"]))
    counts = {group: sum(case.suite == group for case in cases)
              for group in ("carry", "shifter", "multiply-long")}
    if counts != {"carry": 93, "shifter": 70, "multiply-long": 72}:
        raise ValueError(f"Pinned coverage changed: {counts}")
    return SCHEMA + "\n" + "\t".join(COLUMNS) + "\n" + "\n".join(case.tsv() for case in cases) + "\n"
