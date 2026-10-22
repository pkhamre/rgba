# Upstream CPU baselines

## Source selection and provenance

Selected source: [mGBA's GBA hardware test suite](https://github.com/mgba-emu/suite),
revision `e6942030d25ffe3ba76c72b73a86da073ec857cc`, MIT license, copyright
2015 Jeffrey Pfau. The [maintainer's suite announcement](https://forums.mgba.io/showthread.php?tid=18)
describes hardware tests and publishes emulator scores for the carry, shifter,
and multiply-long groups.

This is stronger independent coverage than regenerating expected values from
our emulator, but it is **not a fresh physical-hardware capture**. The repository
contains authored expected-result tables, not a measurement log with a specific
GBA unit/BIOS identity. No physical console has been tested in this workspace.

Other candidates reviewed:

- `jsmolka/gba-tests`: MIT, small ARM/Thumb/memory ROMs with source-level failure
  numbers. The README does not establish physical-capture provenance. Its UI
  currently needs unsupported display I/O, so it is not the first runner target.
- `DenSinH/FuzzARM`: GPL-3.0, generated expected values and a useful EWRAM failure
  dump. It is not a substitute for direct hardware measurements.
- Original Armwrestler: upstream licensing/provenance requires care; the mGBA
  maintainer reports that some old LDM writeback tests fail on real hardware.
  We will not use those results as unquestioned hardware truth.

## Pinned data preparation

```sh
python3 scripts/fetch_mgba_cpu_cases.py
```

This fetches only LICENSE and four small source files: carry.c, shifter.c,
shifter-impl.s, and multiply-long.c. Pinned Git blob IDs are checked even for
cached files. All downloads and generated data stay under ignored
`tests/assets/mgba-suite-cpu/`; the upstream license remains beside the data.
A manifest records revision, source SHA-256 hashes, adapter hash, coverage,
and the generated TSV hash. No downloaded host code or build scripts run.

The restricted adapter emits **235 isolated instruction cases**:

| Group | Source rows | Instruction cases | Compared values per case |
| --- | ---: | ---: | --- |
| Carry: ADCS/SBCS/RSCS | 31 | 93 | Result and full CPSR |
| Shifter: MOVS/ADDS | 70 | 70 | Result and full CPSR |
| Multiply long: SMULLS/UMULLS | 36 | 72 | High word, low word, full CPSR |

Coverage is fixed and checked. Unexpected table contents, field counts,
expressions, function names, or missing cases fail preparation rather than
quietly reducing the test set. Accumulating multiply and timing/peripheral
checks are not in this selected subset.

## Headless protocol

The TSV starts with `gba-cpu-cases-v1`, then a fixed column header. Each row
specifies group/index/name, ARM opcode, initial flags, r0/r1 inputs, expected
output register(s), and full expected CPSR. Expected integers are copied from
the upstream tables. Only literal +/- and `ADDR(current_function)` relocation
are allowed; expressions are parsed without Python eval/exec.

Shifter test instructions are lowered from the pinned assembly function names.
The test instruction is relocated to `0x08000100`, and upstream ADDR expressions
refer to that instruction's PC+8. There is no general-purpose C compiler or
assembler here, and the original binary/UI/wrappers are not being executed.

The planned isolated runner uses a three-instruction program:

1. At `0x080000fc`: `MSR CPSR_f, r12`, with r12 holding the requested flags.
2. At `0x08000100`: the selected instruction, with r0/r1 inputs loaded.
3. At `0x08000104`: `MRS r12, CPSR`, followed by checking results and completion.

Each case begins independently in ARM System mode and must finish exactly at
`0x08000108`. Execution is bounded to three steps; exceptions/device faults,
wrong results/flags, missing cases, and invalid fixtures must fail the run.
This is a test-only CPU-entry setup, not a BIOS skip in the emulator or a claim
that the complete upstream ROM boots. The normal machine still resets at BIOS
address zero.

## Status

Data acquisition/lowering and seven asset-free adapter tests are implemented.
The Rust execution runner is the next separate implementation step. Full-ROM,
peripheral, bus-contention, and physical-hardware validation remain open.
