#!/usr/bin/env python3
"""Reproduce real Emerald saves/Continue using ordinary input and isolated files.

This is supplied-ROM validation, not a generic game compatibility test. Captured
images still need visual inspection; save structure checks do not check every
section checksum. No emulator state or guest memory is injected.
"""

import argparse
import array
import hashlib
import json
import re
import struct
import subprocess
import sys
import time
import wave
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIOS_SHA256 = "fd2547724b505f487e6dcb29ec2ecff3af35a841a77ab2e85fd87350abd36570"
ROM_SHA256 = "a9dec84dfe7f62ab2220bafaef7479da0929d066ece16a6885f6226db19085af"


def flash_generations(data):
    """Return complete 14-section generations (structural check, not checksum)."""
    if len(data) != 131072:
        raise ValueError("Emerald Flash must contain exactly 131072 bytes")
    generations = {}
    for sector in range(32):
        section, _, magic, counter = struct.unpack_from("<HHII", data, sector * 4096 + 0xFF4)
        if magic == 0x08012025 and section < 14:
            generations.setdefault(counter, []).append(section)
    complete = sorted(counter for counter, ids in generations.items() if sorted(ids) == list(range(14)))
    if not complete:
        raise ValueError("no complete 14-section Emerald save generation")
    return complete


def wav_summary(path):
    """Check transport metadata and nonconstant PCM, not perceptual quality."""
    with wave.open(str(path)) as source:
        if (source.getnchannels(), source.getsampwidth(), source.getframerate()) != (2, 2, 32768):
            raise ValueError("unexpected PCM capture format")
        frames = source.getnframes()
        data = source.readframes(frames)
    values = array.array("h", data)
    if sys.byteorder != "little":
        values.byteswap()
    if len(set(values)) < 2:
        raise ValueError("PCM capture is constant, not sound")
    return dict(frames=frames, sample_rate=32768,
                peak=max(abs(value) for value in values),
                distinct_values=len(set(values)), sha256=hashlib.sha256(data).hexdigest())


def continuation():
    commands = ["run 2000 0"]
    for _ in range(3):
        commands += ["run 10 8", "run 390 0"]
    return commands


def run_session(bios, rom, save, commands, log):
    command = ["sh", str(ROOT / "scripts/cargo.sh"), "run", "--release", "-p", "gba-host",
               "--example", "scripted_session", "--", str(bios), str(rom), str(save)]
    start = time.monotonic()
    with log.open("x") as output:
        subprocess.run(command, cwd=ROOT, input="\n".join(commands + ["quit"]) + "\n",
                       text=True, stdout=output, stderr=subprocess.STDOUT, check=True, timeout=1800)
    text = log.read_text()
    activity = re.findall(r"OK frames=(\d+) clocks=(\d+) pc=([0-9a-f]+) dirty=(\w+) revision=(\d+)", text)
    pcm = re.findall(r"PCM frames=(\d+) dropped=(\d+) nonzero=(\d+) peak=(\d+)", text)
    if not activity or not pcm:
        raise ValueError("session did not report completed CPU/audio activity")
    frames, clocks, pc, dirty, revision = activity[-1]
    generated, dropped, nonzero, peak = map(int, pcm[-1])
    if dropped or generated != int(clocks) // 512:
        raise ValueError("audio clock/count/drop invariant failed")
    return dict(frames=int(frames), clocks=int(clocks), pc=pc, dirty=dirty == "true",
                save_revision=int(revision), pcm_frames=generated, pcm_dropped=dropped,
                nonzero_samples=nonzero, peak=peak, wall_seconds=round(time.monotonic() - start, 3))


def validate(bios, rom, output, soak_frames=0):
    bios, rom, output = bios.resolve(), rom.resolve(), output.resolve()
    for path, expected in [(bios, BIOS_SHA256), (rom, ROM_SHA256)]:
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError(f"scenario requires the documented supplied image: {path}")
    output.mkdir(parents=True, exist_ok=False)
    save = output / "validation.sav"
    report = {"bios_sha256": BIOS_SHA256, "rom_sha256": ROM_SHA256,
              "visual_assertions": "captured for manual inspection; not automated OCR"}
    try:
        commands = (ROOT / "tests/scenarios/emerald-new-save.txt").read_text().splitlines()
        commands = [f"wav {output / 'startup.wav'}"] + commands[:]
        # Capture only the first 67 seconds, not the entire new-game setup.
        elapsed = 0
        for index, line in enumerate(commands):
            if line.startswith("run "):
                elapsed += int(line.split()[1])
            if elapsed >= 4000:
                commands.insert(index + 1, "stop-wav")
                break
        commands += [f"shot {output / 'first-save.ppm'}", "run 5 2", "run 120 0",
                     f"shot {output / 'before-restart.ppm'}"]
        report["new_game"] = run_session(bios, rom, save, commands, output / "new-game.log")
        first = save.read_bytes()
        first_generations = flash_generations(first)
        (output / "first-save.sav").write_bytes(first)
        commands = [f"wav {output / 'continue.wav'}"] + continuation()
        commands += [f"shot {output / 'continue-menu.ppm'}", "run 10 1", "run 600 0",
                     f"shot {output / 'continued-world.ppm'}", "stop-wav", "run 5 8", "run 80 0",
                     "run 5 128", "run 30 0", "run 5 128", "run 30 0", "run 5 1", "run 240 0",
                     "run 5 1", "run 500 0", "run 5 1", "run 500 0", "flush",
                     f"shot {output / 'second-save.ppm'}"]
        report["continue_and_resave"] = run_session(bios, rom, save, commands, output / "continue.log")
        second = save.read_bytes()
        second_generations = flash_generations(second)
        if max(second_generations) <= max(first_generations):
            raise ValueError("game did not commit a newer save generation after Continue")
        if Path(str(save) + ".bak").read_bytes() != first:
            raise ValueError("host backup does not match the original save")
        commands = continuation() + [f"shot {output / 'second-continue-menu.ppm'}",
                                     "run 10 1", "run 600 0", f"shot {output / 'second-continued-world.ppm'}"]
        # Optional emulated-time soak, separate from real-time desktop testing.
        remaining = soak_frames
        while remaining:
            count = min(120, remaining)
            commands.append(f"run {count} {16 if (remaining // 120) % 2 else 32}")
            remaining -= count
        report["second_continue"] = run_session(bios, rom, save, commands, output / "second-continue.log")
        if save.read_bytes() != second:
            raise ValueError("reloading unexpectedly changed the disk save")
        report["startup_audio"] = wav_summary(output / "startup.wav")
        report["continue_audio"] = wav_summary(output / "continue.wav")
        report.update(status="passed", first_generation=max(first_generations),
                      second_generation=max(second_generations), backup_matches=True,
                      first_sha256=hashlib.sha256(first).hexdigest(),
                      second_sha256=hashlib.sha256(second).hexdigest())
    except Exception as error:
        report.update(status="failed", error=str(error))
        raise
    finally:
        (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bios", type=Path, default=ROOT / "bins/gba_bios.bin")
    parser.add_argument("--rom", type=Path, default=ROOT / "bins/Pokemon - Emerald Version (USA, Europe).gba")
    parser.add_argument("--output", type=Path, required=True, help="new artifact directory; must not exist")
    parser.add_argument("--soak-frames", type=int, default=0)
    args = parser.parse_args()
    if not 0 <= args.soak_frames <= 200000:
        parser.error("soak frames must be 0..200000")
    print(json.dumps(validate(args.bios, args.rom, args.output, args.soak_frames), indent=2))


if __name__ == "__main__":
    main()
