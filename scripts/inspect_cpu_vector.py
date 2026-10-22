#!/usr/bin/env python3
"""Print one pinned CPU vector as JSON without executing upstream code."""

import argparse
import json
from pathlib import Path
import struct


def word(data, offset):
    return struct.unpack_from("<I", data, offset)[0]


def state(data, offset):
    values = struct.unpack_from("<40I", data, offset + 8)
    return {
        "R": values[:16],
        "R_fiq": values[16:23],
        "R_svc": values[23:25],
        "R_abt": values[25:27],
        "R_irq": values[27:29],
        "R_und": values[29:31],
        "CPSR": values[31],
        "SPSR": values[32:37],
        "pipeline": values[37:39],
        "access": values[39],
    }


def decode(data, index):
    if len(data) < 8 or word(data, 0) != 0xd33dbae0:
        raise ValueError("Invalid vector file header")
    count = word(data, 4)
    if not 0 <= index < count:
        raise ValueError(f"Vector index must be in 0..{count - 1}")
    offset = 8
    for _ in range(index):
        offset += word(data, offset)
    return decode_at(data, offset, index)


def decode_at(data, offset, index):
    """Decode a case at a previously indexed record offset."""
    initial = offset + 4
    final = initial + word(data, initial)
    transactions = final + word(data, final)
    opcode = transactions + word(data, transactions)
    result = {
        "index": index,
        "opcode": word(data, opcode + 8),
        "base_addr": word(data, opcode + 12),
        "initial": state(data, initial),
        "final": state(data, final),
        "transactions": [],
    }
    for i in range(word(data, transactions + 8)):
        values = struct.unpack_from("<6I", data, transactions + 12 + i * 24)
        result["transactions"].append(dict(zip(
            ["kind", "size", "address", "data", "cycle", "access"], values
        )))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("group", help="Vector group name, e.g. arm_mul_mla")
    parser.add_argument("index", type=int, help="Zero-based case index")
    parser.add_argument("--hex", action="store_true", help="Display integers as hexadecimal strings")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1] / "tests/assets/arm7tdmi/v1"
    groups = {path.name.removesuffix(".json.bin"): path for path in root.glob("*.json.bin")}
    if args.group not in groups:
        parser.error("Unknown group; fetch vectors first and use a group name without its suffix")
    try:
        result = decode(groups[args.group].read_bytes(), args.index)
    except (ValueError, struct.error) as error:
        parser.error(str(error))
    if args.hex:
        def hexify(value):
            if isinstance(value, int):
                return f"0x{value:08x}"
            if isinstance(value, dict):
                return {key: hexify(item) for key, item in value.items()}
            if isinstance(value, (list, tuple)):
                return [hexify(item) for item in value]
            return value
        result = hexify(result)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
