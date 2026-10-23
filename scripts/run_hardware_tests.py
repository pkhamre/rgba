#!/usr/bin/env python3
"""Run pinned GBA CPU and memory test ROMs through the normal BIOS boot path."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

import fetch_jsmolka_gba_tests

ROOT = Path(__file__).resolve().parents[1]
ROM_DIRECTORY = fetch_jsmolka_gba_tests.DESTINATION
SUITES = (("ARM", "arm/arm.gba", 12), ("Thumb", "thumb/thumb.gba", 7),
          ("memory", "memory/memory.gba", 12))
MAX_ACTIVITIES = 10_000_000


def default_bios():
    data_home = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share"))
    candidates = (Path(os.environ["RGBA_BIOS"]) if "RGBA_BIOS" in os.environ else None,
                  data_home / "rgba/bios.bin", ROOT / "bins/gba_bios.bin")
    return next((path for path in candidates if path is not None and path.is_file()), None)


def run(bios, selected):
    subprocess.run([sys.executable, str(ROOT / "scripts/fetch_jsmolka_gba_tests.py")],
                   cwd=ROOT, check=True)
    suites = [suite for suite in SUITES if selected == "all" or suite[0].lower() == selected]
    if not bios.is_file():
        raise FileNotFoundError(f"BIOS not found: {bios}; use --bios PATH or RGBA_BIOS")
    failed = False
    for name, relative_rom, result_register in suites:
        rom = ROM_DIRECTORY / relative_rom
        with tempfile.TemporaryDirectory(prefix="rgba-hwtest-") as temporary:
            temporary_path = Path(temporary)
            isolated_rom = temporary_path / rom.name
            shutil.copyfile(rom, isolated_rom)
            environment = os.environ.copy()
            environment["XDG_DATA_HOME"] = str(temporary_path / "data")
            environment["XDG_CONFIG_HOME"] = str(temporary_path / "config")
            command = [
                "sh", "scripts/cargo.sh", "run", "--release", "-p", "gba-headless", "--",
                "--bios", str(bios), "--rom", str(isolated_rom), "--steps", str(MAX_ACTIVITIES),
                "--test-result-register", str(result_register),
            ]
            print(f"Running {name} test ROM through BIOS (limit {MAX_ACTIVITIES:,} activities)",
                  flush=True)
            result = subprocess.run(command, cwd=ROOT, env=environment, timeout=240)
            if result.returncode == 0:
                print(f"{name}: PASS")
            else:
                print(f"{name}: FAIL (headless runner exit {result.returncode})")
                failed = True
    return 1 if failed else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bios", type=Path, default=default_bios(),
                        help="user-provided 16 KiB GBA BIOS (or RGBA_BIOS)")
    parser.add_argument("--suite", choices=("all", "arm", "thumb", "memory"), default="all")
    args = parser.parse_args()
    if args.bios is None:
        parser.error("BIOS not found; specify --bios PATH or set RGBA_BIOS")
    return run(args.bios.expanduser().resolve(), args.suite)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, subprocess.SubprocessError, ValueError) as error:
        print(f"hardware test runner: {error}", file=sys.stderr)
        raise SystemExit(2) from error
