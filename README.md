# Rust GBA emulator

A from-scratch Rust ARM7TDMI/GBA emulator targeting Linux desktop and hardware
accuracy. **This is an early CPU/bus implementation, not a playable emulator.**
The supplied BIOS currently stops at unimplemented POSTFLG I/O before booting
Emerald. There is no desktop frontend, graphics, audio, save hardware, or RTC yet.

## Build and check

Install Rust using rustup, a native Linux linker/toolchain, and Python 3.9+ for
asset-free tooling tests. The repository
pins stable Rust 1.90.0 and uses no third-party Rust dependencies at this stage.

```sh
sh scripts/check.sh
```

This workspace also has an isolated toolchain under ignored `.tools/`, using
Rust's self-contained musl target because the execution environment has no C
compiler. `scripts/cargo.sh` selects it when `cargo` is absent from PATH.
Normal development hosts use their own Cargo and native target.

## Headless development runner

```sh
mkdir -p artifacts
sh scripts/cargo.sh run -p gba-headless -- \
  --bios bins/gba_bios.bin \
  --rom "bins/Pokemon - Emerald Version (USA, Europe).gba" \
  --steps 100000 --trace artifacts/boot.trace
```

The runner validates asset sizes, bounds instruction execution, and reports the
first unsupported operation with the executing address. **The above command
currently exits unsuccessfully on POSTFLG I/O; that is expected, not successful
BIOS boot.** Trace files are created exclusively and never overwrite existing
files. Use a new trace filename on subsequent runs.

`--stop-at 08000000` stops before executing a target address; exhausting the
instruction budget without reaching it returns failure. `--help` needs no assets.

## Layout

- `crates/gba-core`: CPU, status/banked registers, bus, machine, and trace types.
- `crates/gba-headless`: bounded CLI runner and end-to-end tests.
- `scripts/`: checks and optional pinned reference-vector download.
- [PLAN.md](PLAN.md): agreed architecture and release checkpoint.
- [docs/PROGRESS.md](docs/PROGRESS.md): milestone status and next work.
- [docs/ACCURACY.md](docs/ACCURACY.md): implemented behavior and known failures.
- [docs/TESTING.md](docs/TESTING.md): reproducible checks and reference comparisons.
- [docs/STYLE.md](docs/STYLE.md): Google Rust style policy.
- [docs/CPU_RESEARCH.md](docs/CPU_RESEARCH.md): CPU fixes, evidence, and reference-model disagreements.
- [docs/UPSTREAM_CPU_TESTS.md](docs/UPSTREAM_CPU_TESTS.md): isolated mGBA CPU-table replay (235 passing cases).

The multiply carry calculation is an attributed adaptation of a published
hardware-research model, not an emulator backend. Its zlib notice and the MIT
notice for minimized reference-test constants are retained in `LICENSES/`.

BIOS/game images, saves, local test assets, build products, and traces are ignored
by version control. Supply your own lawfully obtained images; none are embedded
in the source or tests.
