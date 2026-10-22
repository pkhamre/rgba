# Rust GBA emulator

A from-scratch Rust ARM7TDMI/GBA emulator targeting Linux desktop and hardware
accuracy. **The desktop preview includes video, controls, pause indication,
and audio.** A user-provided BIOS boots normally. Game-menu save/restart/Continue
is verified, and the first battle succeeded in user testing. Full-game and
hardware accuracy remain unverified.

## Try the preview

From a Linux X11 desktop, or Wayland desktop with XWayland:

```sh
sh scripts/play.sh
```

No BIOS or game images are bundled or tracked. The launcher looks for a BIOS
at `$XDG_DATA_HOME/rgba/bios.bin` (default `~/.local/share/rgba/bios.bin`);
`RGBA_BIOS` or `--bios PATH` can override it. For compatibility it also accepts
a local, untracked `bins/gba_bios.bin` when the XDG default is absent. With no
`--rom` argument, the desktop scans `$XDG_DATA_HOME/rgba/games` (default
`~/.local/share/rgba/games`), starts its only game automatically, or shows a
game chooser when there are multiple games. Put `.gba`, `.agb`, or `.bin` ROMs
directly in that folder. The chooser shows each ROM's filename without the
extension and wraps longer names. To launch a ROM directly instead:
`sh scripts/play.sh path/to/game.gba --scale 2`.

Controls: arrows/WASD = D-pad; Z/X = A/B; Enter = Start; Backspace = Select;
Q/E = L/R. Space pauses; Tab fast-forwards. F5 flushes the cartridge save,
F6/F7 save/load a machine state, and F12 captures a PPM screenshot. Escape opens
the options menu; choose Back to Library to switch games after flushing the
cartridge save, or Exit to quit. The menu configures bindings. State saves and
loads show a brief on-screen result notice.
Hold Tab through boot/intro if desired; use Enter at the title and Z to confirm.

USB gamepads are detected through Linux evdev when the emulator starts, and
newly connected pads are detected while it is running. The D-pad and left stick
control movement; the bottom face button maps to GBA A, the right face button
to B, and the left/right shoulder buttons to L/R. Start + Select opens the
options menu; use the D-pad to navigate, A/Start to select, and B to go back.
The current implementation requires read access to the controller's
`/dev/input/event*` device.

Saves and states are stored under `$XDG_DATA_HOME/rgba` (default
`~/.local/share/rgba`); settings are under `$XDG_CONFIG_HOME/rgba` (default
`~/.config/rgba`). Per-game filenames use the ROM's SHA-256 identity. ROM-side saves, states, and settings are copied on first launch and retained;
existing central files win conflicts. To migrate files beside a ROM outside the
games folder, launch that original path once before adding its ROM to the folder,
or copy the sidecars alongside it. Use the game's save menu first: F5 is not an
emulator save state.
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
  --bios "${XDG_DATA_HOME:-$HOME/.local/share}/rgba/bios.bin" \
  --rom "path/to/game.gba" \
  --steps 8000000 --stop-at 08000000
```

This reaches cartridge entry through the user-provided BIOS, without replacing
BIOS calls. `--stop-at` returns failure if the CPU-activity budget is exhausted first.
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
