# Testing

## Required local checks

Development prerequisites: stable Rust/rustup, a native linker (or the workspace's
isolated musl setup), and Python 3.9+ for asset-free tooling tests.

```sh
sh scripts/check.sh
sh scripts/cargo.sh test --workspace --release
```

Current standard result: **176 Rust unit/integration tests, one doctest, and 23
Python tool tests pass**. Two ignored optional tests require reference assets.
An additional ignored presentation test requires a dedicated Xvfb display.
The standard tests use synthetic data only; CI needs no BIOS/game/test ROM.

Core save-state unit tests exercise complete deterministic round trips, ROM
mismatch, corrupt checksum and payload rejection, and unchanged state on failure.
Host integration tests also cover atomic quick-slot round trips, backup retention
when state loading replaces cartridge data, alias protection, and corrupt-state
failure without changing the running machine or save file. Desktop menu, game
library, keyboard/gamepad controls, and state-result notices are implemented and
have separate coverage documented in the desktop validation section below.

## Optional headless CPU and memory test ROMs

The ARM, Thumb, and memory suites from pinned jsmolka/gba-tests run via the
normal BIOS boot path with a bounded execution budget and explicit result
register/self-branch completion detection. Downloaded ROMs stay ignored. The
local result is ARM/Thumb/memory PASS; the source does not establish physical
capture provenance. See [HARDWARE_TESTS.md](HARDWARE_TESTS.md) for pinning,
license, protocol, and limitations.

## Optional strict CPU reference comparison

Source: https://github.com/SingleStepTests/ARM7TDMI
Revision: `e3097d88d428752736b949d94c094d5da50f0db6`, MIT, copyright 2024
SingleStepTests. The downloader retains upstream README/LICENSE and verifies
pinned Git blobs. A SHA-256 manifest records local bytes; assets are ignored.

```sh
python3 scripts/fetch_cpu_vectors.py
sh scripts/cargo.sh test -p gba-core external_cpu_vectors -- --ignored --nocapture
```

Download size is approximately 950 MB; network access is required only for
fetching. No downloaded code is executed. All 45 groups, including absent
coprocessors and reserved Thumb conditions, are now exercised: 2,250,000 cases.

Assertions cover User/banked registers, CPSR/SPSRs, pipeline, memory transaction
order/address/width/data, sequential/nonsequential flags, and final fetch state.
The flat reference bus is not a GBA memory map; memory behavior is tested
separately. LOCK, other access flags, and transaction timestamps are not checked.

These experimental vectors come from NanoBoyAdvance, not direct hardware
capture. Upstream warns about illegal operands and cycle recording. They do
not establish GBA cycle accuracy.

**The full strict command currently fails:** 2,124,072 matches, 125,928 failures.
27 groups match completely. No test outcomes are waived or normalized.
See [CPU_RESEARCH.md](CPU_RESEARCH.md) for first-diagnostic categories and
documented disagreements, and [ACCURACY.md](ACCURACY.md) for remaining gaps.

## Replay one case

A filter is a filename substring. A selected index is zero-based and requires
exactly one matching group; out-of-range indices and empty selections fail.

```sh
GBA_VECTOR_FILTER=thumb_data_proc GBA_VECTOR_INDEX=165 \
  sh scripts/cargo.sh test -p gba-core external_cpu_vectors -- --ignored --nocapture

python3 scripts/inspect_cpu_vector.py thumb_data_proc 165 --hex
```

Case 165 now passes. The inspector prints the original complete state and bus
transactions without running foreign code. Omit GBA_VECTOR_INDEX to check every
case in the selected group.

## Failure reports and triage

```sh
mkdir -p artifacts
GBA_VECTOR_REPORT=artifacts/cpu-new.tsv \
  sh scripts/cargo.sh test -p gba-core external_cpu_vectors -- --ignored --nocapture

python3 scripts/triage_cpu_vectors.py artifacts/cpu-new.tsv
```

The strict Rust command still returns failure. The Python command only groups
diagnostics; it cannot turn a failing test into a pass.

Reports contain every failing case's group, index, and first diagnostic.
GBA_VECTOR_REPORT paths are relative to the workspace root unless absolute;
the parent directory must exist. Existing files are never overwritten.
Choose a new report filename on each run.

Classification identifies first-diagnostic patterns, not every possible issue
in a case. Current full logs/TSV/triage are under ignored
`artifacts/cpu-preview-report.txt`, `artifacts/cpu-preview-new.tsv`, and
`artifacts/cpu-preview-triage.json`.

## Independent mGBA CPU-table baselines

```sh
python3 scripts/fetch_mgba_cpu_cases.py
sh scripts/cargo.sh test -p gba-core mgba_cpu_baselines -- --ignored --nocapture
```

All 235 isolated carry/shifter/multiply-long cases pass. This is a table-driven
CPU replay, not execution of the complete suite ROM or new physical-GBA capture.
[UPSTREAM_CPU_TESTS.md](UPSTREAM_CPU_TESTS.md) documents pinned sources, license,
lowering, bounded completion, coverage, and group/case selection. These results
do not replace or suppress strict reference-vector failures.

## Supplied-asset BIOS and game smoke

The README's bounded `--stop-at 08000000 --steps 8000000` command now succeeds:
7,610,482 CPU activities, 75,998,009 clocks, cartridge entry at `08000000`.
POSTFLG and the BIOS's zero-SPSR initialization no longer block normal boot.

```sh
sh scripts/smoke-emerald.sh
```

This executes 4,000 display frames with scripted Start/A presses and captures
Professor Birch's new-game introduction. Inspect the PPM output; the script
does not automatically assert visual correctness. Core/host checks use
synthetic Flash, RTC, save, and framebuffer fixtures. The separate
`python3 scripts/validate_emerald.py --output artifacts/new-save-check` scenario
now verifies actual game-menu save/restart/Continue/re-save/Continue with
isolated saves and captured images; see VALIDATION.md.

## Optional X11 scaling regression

This asset-free test checks every destination pixel at scales 1..6. Only run
against a dedicated display; its largest window is 1440x960:

```sh
xvfb-run -a -s '-screen 0 1600x1200x24' sh scripts/cargo.sh test -p gba-desktop x11_scaled_pixels_match_cpu_reference -- --ignored
xvfb-run -a -s '-screen 0 1600x1200x24 -extension RENDER' sh scripts/cargo.sh test -p gba-desktop x11_scaled_pixels_match_cpu_reference -- --ignored
```

Both enabled-RENDER and disabled-RENDER fixtures passed locally. This validates
host scaling/transport, not the hardware accuracy of guest pixels.
`validate_desktop.py --scale 3` additionally exercises default-scale presentation;
its historical default remains 1 for comparison with older soak runs.

## Optional desktop smoke

On a Linux desktop: `sh scripts/play.sh`. With Xvfb installed:

```sh
xvfb-run -a sh scripts/play.sh path/to/game.gba --scale 1 --frames 120
```

A 6,000-frame `scripts/validate_desktop.py` run passed under Xvfb at scale 3
(100.222 emulated seconds, 109.065 wall seconds). It exercised options state
save/load/resume, duplicate binding rejection, keybinding persistence across
launches, F6/F7 success/failure notices, and explicit Exit. The disposable
cartridge save was unchanged on both exits. The validator now also cycles
1x/2x/4x/8x/16x and checks persistence plus 1x reload; that new path has not been
run here because Xvfb is unavailable. Audio was off; this result does not
validate a playback device. A separate virtual-X-server run compared captured
paused-window RGB bytes against the core screenshot. A separate Xvfb run with
two distinct ROM hashes verified the startup chooser, Back to Library action,
and selection/resumption of a game. The validator isolated XDG data/config
paths under its output directory; the disposable cartridge save remained
unchanged. These tests do not establish physical-display latency. Local test
artifacts are ignored (`artifacts/desktop-smoke.json` and
`artifacts/desktop-xvfb.log`).

PSG/Direct Sound and Linux audio transport are implemented. Game save/Continue
is locally verified; the first battle succeeded in user testing. A 32-minute
controlled real-time desktop soak passed on the earlier buffer configuration;
a 6,000-frame latest-build repeat and fixed-Pulse smoke also passed. See
VALIDATION.md for exact scope/builds, and AUDIO.md for sound limitations.
Human listening, varied interactive gameplay, and physical/reference comparisons
remain outstanding.
Clock counts are model outputs, not hardware measurements. Proprietary assets
remain unchanged and excluded from CI.
