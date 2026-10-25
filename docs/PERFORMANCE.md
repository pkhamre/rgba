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

## Verified behavior and remaining gaps

Synthetic BIOS/ROM tests exercise repeated state/PCM consistency, scripted input,
state loading/rejection, fault propagation, and unchanged or absent cartridge
saves. Mocked profiler tests exercise paths with spaces, exclusive output
directories and terminal build/record failures. These do not validate real perf
sampling, debug-symbol attribution or a laptop/game performance improvement.
Real-asset benchmark results and native Linux perf recordings remain manual
validation work. Known optional CPU-reference mismatches remain documented in
[TESTING.md](TESTING.md); this tooling does not resolve them.
