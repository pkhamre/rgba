# Headless benchmarks and profiling

The headless benchmark measures unpaced core execution, not desktop smoothness.
It includes CPU, hardware, PPU rendering, audio generation, PCM draining and a
small PCM fingerprint/timing overhead. It excludes window presentation, audio
players, wall-clock pacing and save-file writes. No BIOS or games are supplied.

## Repeatable benchmark

Build and run with your own assets (absolute paths simplify reproduction):

```sh
sh scripts/cargo.sh run --release -p gba-headless -- \
  --bios /path/to/gba_bios.bin --rom /path/to/game.gba \
  --benchmark --frames 1800 --warmup-frames 120 --repeats 5 \
  --input 0:start --input 10:none
```

For a representative gameplay scene, add `--state /path/to/game.state` using a
compatible rgba state for the same ROM. Unlike desktop startup, this command
never automatically loads an adjacent state or cartridge save. Without `--state`
it starts from BIOS reset and the cartridge's default backup contents. RTC is
not initialized from host time. It never creates or updates cartridge saves,
settings or state files. Optional `--screenshot PATH` writes a new final PPM.

Warm-up executes once, draining PCM each frame, before capturing the baseline.
Every measured repeat clones that baseline outside the timer. `--input` event
numbers start at zero **after warm-up**, relative to each repeat, and use the
same held-key semantics as ordinary headless runs. Choose a bounded input
sequence that reaches the scene you want; input during warm-up is not supported.
The defaults are 60 warm-up frames and 3 repeats; zero warm-up is allowed.
`--benchmark` requires positive `--frames` and cannot use instruction tracing,
step budgets, stop-address or hardware-test result detection.

Output on stderr includes:

- Per-repeat wall seconds, frames/sec and `realtime_x`. A value above 1 means
  faster than real time, using the actual emulated-clock delta at 16,777,216 Hz.
- Average, p95 (sorted index `floor((N-1)*0.95)`) and maximum frame wall time.
  Per-frame timing includes PCM draining/fingerprinting; overall timing also
  includes input dispatch and timing-vector bookkeeping.
- Clock and PCM counts, plus stable FNV-1a fingerprints of all measured PCM and
  the serialized final machine state. These fingerprints are not cryptographic.
- Median repeat wall time (upper median for an even number of repeats).

The runner checks exact final serialized state, PCM fingerprint, PCM count and
clock delta against repeat 1; it exits unsuccessfully on any difference or core
fault. Serialization and comparisons happen outside the timer. This checks
repeatability, **not hardware accuracy**. FNV collisions are possible; final
state comparison is exact, but the full historical PCM stream is not retained.
State formats and fingerprints may change between revisions. Compare matching
counts/fingerprints when testing optimizations, and run the correctness suite.

Record the commit, command, game/state identity, toolchain, OS, CPU, power
profile and whether AC is connected. Compare the same workload and build profile
under the same conditions, with other heavy work stopped. Use multiple runs;
thermal throttling, scheduling and frequency changes affect measurements.
Do not compare debug builds to optimized builds or profiled to unprofiled times.
Keep BIOS/game/state files and reports private and out of git.

## Linux function-level profiler

Install your distribution's Linux `perf` and Python 3. The wrapper builds the
`profiling` Cargo profile: release optimization, thin LTO and overflow checks
unchanged, with debug symbols retained for attribution.

From the repository root, choose a **new** directory under ignored `artifacts/`:

```sh
mkdir -p artifacts
sh scripts/profile-headless.sh artifacts/profile-balanced \
  --bios /path/to/gba_bios.bin --rom /path/to/game.gba \
  --state /path/to/game.state --frames 1800 --warmup-frames 120 --repeats 3
```

Outputs are `benchmark.log`, `perf.data`, `report.txt` and Cargo's `build.json`.
The wrapper uses Cargo's reported executable path, including custom targets and
`CARGO_TARGET_DIR`. It preserves workload/build failures and does not use sudo,
change power profiles, or weaken system security settings. Permission errors or
unsupported CPU events are terminal; consult your distribution's perf policy.

Recording uses user-space CPU cycles at a requested 199 Hz and DWARF call graphs
([Linux perf documentation](https://github.com/torvalds/linux/blob/master/tools/perf/Documentation/perf-record.txt)).
It samples the **whole process**, including startup, warm-up, baseline cloning,
fingerprints and final-state serialization, not just the timed frame loop.
Use a long measured workload so those costs do not dominate. In `report.txt`,
look under `Machine::run_frame` for CPU, bus, PPU and APU hot paths; distinguish
inclusive child percentages from self percentages. Inlining/LTO may merge or
remove symbols; samples are statistical, not exact per-subsystem timers.
This profiler does not measure device underruns or off-CPU scheduling delays.
You can inspect the recording interactively while the same binary still exists:

```sh
perf report -i artifacts/profile-balanced/perf.data
```

## Timer prescaler optimization

The supplied laptop Emerald recording (`1800` frames, `120` warm-up frames,
three repeats) reported a median 19.869469 seconds, about 90.59 frames/sec and
1.52x real time under perf. `Hardware::advance_timers` accounted for 17.54% of
self samples. Some recorded call chains contain implausible addresses/parents;
use the self samples as hotspot evidence, not precise subsystem accounting.

Timer advancement now uses shifts and masks for the power-of-two prescalers
1, 64, 256 and 1024 instead of runtime division/remainder. Overflow/reload
arithmetic, cascades and event scheduling are unchanged. A division-reference
regression covers every timer and prescaler, all snapshot-valid phases (including
phases retained when changing prescalers), boundary cycle counts, reloads,
multiple overflows and IRQ flags. Existing cascade and FIFO/DMA regressions
remain applicable. Validation: `sh scripts/check.sh`, targeted release hardware
tests and release workspace tests pass. Existing host audio tests still emit
`sleep: not found` warnings. The optional strict CPU-reference suite was not
rerun and retains its documented mismatches.

No laptop speedup is established by this change. Rerun the same workload before
and after without perf, compare clocks/PCM/final state and rendered output, and
then record a new profile. The supplied profiled timing is not an unprofiled
baseline; real-asset equivalence and generated-code inspection remain pending.

## Scanline configuration decoding

The renderer now prepares background controls, text row/scroll coordinates,
mosaic dimensions, affine steps, map/tile bases, bitmap page selection, window
bounds/masks and blend coefficients once per scanline. The decoded structures
are local to `render_line`: no persistent cache, save-state change or register
write invalidation is involved. Pixel composition, tile fetching, affine
reference updates and the existing scanline timing model remain unchanged.

Before refactoring, synthetic framebuffer/affine-reference fingerprints were
captured from the original renderer for 3,072 scanlines across modes 0–7, all
window enable combinations, varied background controls, mosaic and effects.
The new renderer matches these fingerprints. A separate comparison retains the
original window decoder and checks masks at normal, wrapped and equal bounds,
including WIN0/WIN1 precedence and OBJ/outside masks. Existing semi-transparent
OBJ and forced-blank tests pass. Fingerprints are regression evidence, not an
exhaustive equivalence proof or hardware-accuracy validation.

Validation: `sh scripts/check.sh`, targeted release PPU tests and release workspace
tests pass. Existing host audio `sleep: not found` warnings remain. The optional
strict CPU-reference suite was not rerun and retains its documented mismatches.
No real-asset framebuffer/state comparison or laptop speedup is established;
repeat the same unprofiled Emerald benchmark before/after to measure the effect.

## Transaction-width memory reads

Ordinary EWRAM, IWRAM, palette, VRAM, OAM and in-range ROM reads now dispatch
once per transaction and assemble a safe little-endian byte/halfword/word load
from a contiguous slice. Aligned transactions cannot cross the RAM mirror or
VRAM split boundaries. GPIO-containing ROM words (including the mixed word at
`0xc8`), partial/out-of-range ROM reads, protected BIOS and unmapped reads retain
the original byte-wise path. I/O and cartridge backup handling, read alignment,
waitstate charging, hardware advancement and latch updates remain unchanged.

A regression retains the original transaction reader and compares returned
values/errors, clocks, open-bus/BIOS latches and complete final bus snapshots.
It covers all widths and access kinds, unaligned inputs, RAM/VRAM mirror edges,
all ROM windows, six ROM lengths (including partial final halfwords/words),
absent/disabled/enabled RTC GPIO, mixed GPIO/ROM words, BIOS execution/protection,
I/O and unmapped/backup addresses. Snapshots include I/O registers, RTC, PCM and
framebuffer state. This establishes sampled synthetic equivalence, not hardware
accuracy or exhaustive address coverage.

Validation: `sh scripts/check.sh`, targeted release bus tests and release workspace
tests pass. Existing host audio `sleep: not found` warnings remain. The optional
strict CPU-reference suite was not rerun and retains its documented mismatches.
Real-asset equivalence and laptop performance measurements remain pending.

## Text tile-row reuse

Text backgrounds now keep one decoded eight-pixel tile row per background,
local to the scanline render call. A source tile-column change fetches the map
entry once, reads four (4bpp) or eight (8bpp) tile bytes, applies flips and palette
selection, and records transparent/color values. Subsequent samples in that
source tile reuse the row, including repeated mosaic samples. Window-hidden
spans may skip columns; sampling the next visible column refreshes the row as
needed. No heap allocation, persistent cache or save-state field was added.
Affine/bitmap sampling, compositor ordering and scanline timing are unchanged.

A differential test retains the previous per-pixel text sampler and compares
varied tile/map bases, sizes, color depths, scrolling, mosaic, flips, palette
banks and window-like gaps. A separate regression changes VRAM and palette data
between lines to check that decoded rows do not persist. The original renderer's
3,072-scanline framebuffer/affine-reference fingerprints still match, and existing
window/transparency regressions pass. These are synthetic unchanged-behavior
checks, not new hardware-accuracy evidence.

Validation: `sh scripts/check.sh`, targeted release PPU tests and release workspace
tests pass. Existing host audio `sleep: not found` warnings remain. The optional
strict CPU-reference suite was not rerun and retains its documented mismatches.
Real-asset equivalence and laptop performance measurements remain pending;
decoding unused pixels at narrow window edges can offset some saved work.

## APU channel-period caching

After the timer, renderer and bus-read steps, the user reported unprofiled
five-repeat Emerald medians of 20.094260 seconds at baseline `f520e2d` and
18.188221 seconds after tile-row reuse: 9.5% less execution time, or 10.5% more
throughput. Clocks, PCM count/hash and final-state hash matched. This is one
workload and does not isolate individual changes or prove identical historical
framebuffers. The subsequent optimized profile attributed 18.47% of self samples
to `Apu::advance`; implausible call chains remain an attribution limitation.

APU advancement now reads four cached channel periods instead of decoding
frequency/noise registers on every bus slice. Relevant accepted register writes,
channel-1 sweep updates and master reset maintain the cache. It is excluded from
serialization, preserving the original version-1 APU payload, and bus restoration
rebuilds it after restoring the separate I/O registers. Event scheduling, phase
retiming rules, noise feedback, mixing and sample generation are unchanged.

Tests cover all 2,048 square/wave frequencies, all 256 noise control bytes,
ignored powered-off writes, reset, positive/negative sweep, and exact PCM/state
comparison against the original uncached advancement with scripted writes,
triggers, FIFO activity and varying bus-sized clock slices. A legacy-layout
serialization comparison and active-audio bus snapshot continuation exercise
state compatibility and cache reconstruction. Required checks, targeted release
APU/restoration tests and release workspace tests pass. Existing host audio
`sleep: not found` warnings remain. The optional strict CPU-reference suite was
not rerun and retains its documented mismatches.

The reported 10.5% throughput improvement predates this APU change. No additional
laptop speedup or real-asset equivalence is claimed until the unprofiled benchmark
and profile are rerun; these synthetic checks do not establish hardware fidelity.

## Verified behavior and remaining gaps

Synthetic BIOS/ROM tests exercise repeated state/PCM consistency, scripted input,
state loading/rejection, fault propagation, and unchanged or absent cartridge
saves. Mocked profiler tests exercise paths with spaces, exclusive output
directories and terminal build/record failures. These do not validate real perf
sampling, debug-symbol attribution or a laptop/game performance improvement.
Real-asset benchmark results and native Linux perf recordings remain manual
validation work. Known optional CPU-reference mismatches remain documented in
[TESTING.md](TESTING.md); this tooling does not resolve them.
