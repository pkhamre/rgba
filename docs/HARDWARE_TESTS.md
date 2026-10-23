# Headless CPU and memory test ROMs

The optional runner executes the ARM, Thumb, and memory ROMs from
[jsmolka/gba-tests](https://github.com/jsmolka/gba-tests), pinned at revision
`a7113b67e63f83a9b321696ddd7042ccfad6c881` (MIT; copyright 2019 Julian
Smolka). The required notice is retained in `LICENSES/jsmolka-gba-tests.txt`.
The binaries are downloaded into ignored `tests/assets/jsmolka-gba-tests/` and
are never committed. SHA-1 Git blob IDs and SHA-256 hashes are checked on every
fetch/use; the pinned upstream MIT `LICENSE` is retained beside the binaries.

These ROMs display “All tests passed” or a failing test number. The runner
recognizes their completion path from source: ARM and memory place the result
in r12; Thumb uses r7. It detects a stable, unconditional self-branch repeated
three times, then interprets zero as pass and nonzero as failure. If no
completion is reached within 10,000,000 CPU activities, the run fails. Each ROM
runs in its own temporary XDG data/config directories so it cannot touch the
user's cartridge saves or settings.

```sh
python3 scripts/run_hardware_tests.py --bios /path/to/bios.bin
# Or set RGBA_BIOS; optional suite selectors: arm, thumb, memory.
python3 scripts/run_hardware_tests.py --suite memory
```

A BIOS is required. The run follows the normal BIOS boot path; it does not
inject a machine state, skip BIOS execution, inspect guessed pixel output, or
write guest memory to report results. The pinned downloaded ROM bytes are test
inputs, not code built or executed on the host.

Local result in this workspace: **ARM PASS, Thumb PASS, memory PASS**. These
ROMs contain authored expected-result checks and their README does not document
fresh physical-console capture provenance. Passing is independent regression
evidence, not proof of physical-hardware accuracy. It does not close the CPU/bus
milestone, cover all device behavior, or replace the strict SingleStepTests
comparison; that comparison still has documented mismatches. See
[ACCURACY.md](ACCURACY.md) and [CPU_RESEARCH.md](CPU_RESEARCH.md).
