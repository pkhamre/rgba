# Rust GBA emulator

A from-scratch Rust ARM7TDMI/GBA emulator targeting Linux desktop and hardware
accuracy. **The desktop preview includes video, controls, pause indication,
and audio.** The supplied BIOS boots normally. Game-menu save/restart/Continue
is verified, and the first battle succeeded in user testing. Full-game and
hardware accuracy remain unverified.

## Try the preview

From a Linux X11 desktop, or Wayland desktop with XWayland:

```sh
sh scripts/play.sh
```

This uses your existing `bins/gba_bios.bin` and
`bins/Pokemon - Emerald Version (USA, Europe).gba`. No images are bundled.
You can select another ROM: `sh scripts/play.sh path/to/game.gba --scale 2`.

Controls: arrows/WASD = D-pad; Z/X = A/B; Enter = Start; Backspace = Select;
Q/E = L/R. Space pauses; Tab fast-forwards. F5 flushes the cartridge save,
F6/F7 save/load a machine state, and F12 captures a PPM screenshot. Escape opens
the options menu; choose Exit or close the window to quit and flush saves.
The menu configures bindings. State saves and loads show a brief on-screen result notice.
Hold Tab through boot/intro if desired; use Enter at the title and Z to confirm.

Save data defaults to the ROM's `.sav` file; previous data is kept in
`.sav.bak`. Use the game's save menu first: F5 is not an emulator save state.
See [docs/TEST_BUILD.md](docs/TEST_BUILD.md) for launch options, RTC/save policy,
verified behavior, and limitations.

No graphical display? Run the bounded supplied-asset smoke test:

```sh
sh scripts/smoke-emerald.sh
```

It runs 4,000 frames with scripted input and writes a screenshot under
`artifacts/`. It does not verify a battle, in-game save/continue, or audio.

Audio uses an installed `pw-cat`, `pacat`, or `aplay` client. Select
`--audio pipewire|pulse|alsa`, or use `--audio off`. Missing audio devices/tools
produce a terminal warning; gameplay still runs. Pause/fast-forward mute new
audio output. See [docs/AUDIO.md](docs/AUDIO.md) for setup and accuracy limits.

A 32-minute automated real-time desktop soak passed. This used a virtual
display/null audio sink, not a human listening session. Save-cycle and desktop
test reproduction, tested-build distinctions, and remaining human validation
are documented in [docs/VALIDATION.md](docs/VALIDATION.md).

## Build and check

Install Rust/rustup, a native Linux linker/toolchain, and Python 3.9+ for
asset-free tooling tests. Rust 1.90.0 is pinned. The desktop uses x11rb for
X11 protocol transport, not for emulation; no emulator backend is used.

```sh
sh scripts/check.sh
sh scripts/cargo.sh test --workspace --release
```

The ignored `.tools/` setup in this workspace supplies musl Rust and an optional
Zig C-linker wrapper for dependency build scripts. `scripts/cargo.sh` selects
that isolated setup when Cargo is absent from PATH. Normal hosts use their
own Cargo and native linker. Toolchains/build products are not committed.

## Headless development runner

```sh
mkdir -p artifacts
sh scripts/cargo.sh run --release -p gba-headless -- \
  --bios bins/gba_bios.bin \
  --rom "bins/Pokemon - Emerald Version (USA, Europe).gba" \
  --steps 8000000 --stop-at 08000000
```

This reaches cartridge entry through the supplied BIOS, without replacing BIOS
calls. `--stop-at` returns failure if the CPU-activity budget is exhausted first.
Use `--frames N --screenshot new-file.ppm` for framebuffer capture,
`--input FRAME:buttons` for held keys, and `--trace new-file.trace` for
versioned activity traces (instructions, HALT, IRQ entry). Screenshot/trace files
never overwrite existing files. `--help` needs no assets.

## Layout and evidence

- `crates/gba-core`: deterministic CPU, bus, devices, renderer, and cartridge.
- `crates/gba-host`: shared asset loading, atomic saves, and screenshots.
- `crates/gba-desktop`: X11 window, options/keybinding menus, integer scaling, and pacing.
- `crates/gba-headless`: bounded CLI runner and end-to-end tests.
- [PLAN.md](PLAN.md): agreed milestones and outstanding release checkpoint.
- [docs/PROGRESS.md](docs/PROGRESS.md): status and next work.
- [docs/ACCURACY.md](docs/ACCURACY.md): implemented behavior and known failures.
- [docs/TESTING.md](docs/TESTING.md): reproducible checks and reference results.
- [docs/STYLE.md](docs/STYLE.md): Google Rust style policy.
- [docs/CPU_RESEARCH.md](docs/CPU_RESEARCH.md): CPU corrections and disagreements.
- [docs/UPSTREAM_CPU_TESTS.md](docs/UPSTREAM_CPU_TESTS.md): 235 isolated mGBA cases.

Strict CPU comparison still fails; it is not suppressed by the passing standard
checks. The multiply carry model and test constants retain their notices under
`LICENSES/`, as do host dependency license notices.

BIOS/game images, saves, downloaded test assets, build products, and traces are
ignored by version control. Supply your own lawfully obtained images.
