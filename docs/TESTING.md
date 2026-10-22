# Testing

## Required local checks

Development prerequisites: stable Rust/rustup, a native linker (or the workspace's
isolated musl setup), and Python 3.9+ for asset-free tooling tests.

```sh
sh scripts/check.sh
sh scripts/cargo.sh test --workspace --release
```

Current standard result: **84 Rust unit/integration tests, one doctest, and six
Python tool tests pass**. One ignored optional test requires reference assets.
The standard tests use synthetic data only; CI needs no BIOS/game/test ROM.

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

**The full strict command currently fails:** 2,121,019 matches, 128,981 failures.
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
`artifacts/cpu-validation-fixed-access.*` and `artifacts/cpu-triage-fixed-access.json`.

## BIOS diagnostic

Use the README's bounded runner command. Current supplied-asset result remains:

```text
completed=4 clocks=10 next=00000074 cpsr=600000d3
error: unimplemented I/O read at 04000300
```

This verifies loading and initial CPU execution only, not BIOS authenticity,
successful boot, or Emerald compatibility. Clock counts are model outputs, not
hardware measurements. Proprietary assets remain unchanged and excluded from CI.
