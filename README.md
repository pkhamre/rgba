# rgba

A Game Boy Advance emulator written from scratch in Rust for Linux. rgba has
its own ARM7TDMI interpreter, hardware models, and software renderer; it does
not use another emulator as a backend.

This is an experimental desktop preview. BIOS boot, Pokémon Emerald's first
battle, and in-game save/restart/Continue have been exercised. Full-game
compatibility and hardware accuracy remain unverified. Independent CPU
reference comparisons still have known failures; see
[accuracy and limitations](docs/ACCURACY.md) and [validation](docs/VALIDATION.md).

## Features

- Linux X11 desktop, including Wayland sessions with XWayland, with integer scaling.
- Keyboard controls and hot-plugged USB gamepads through Linux evdev.
- Audio through PipeWire, PulseAudio, or ALSA command-line players.
- Cartridge saves, a quick-save-state slot per game, and cartridge RTC support.
- Game library, configurable bindings, and per-game settings.
- Menu-selectable 1x/2x/4x/8x speed and temporary unlimited fast-forward.
- A separate headless runner for bounded execution, screenshots, and traces.

No BIOS or game images are included. Supply your own lawfully obtained images.

## Download and run

Tagged GitHub Releases provide an x86_64 Linux desktop archive named
`rgba-vVERSION-x86_64-unknown-linux-musl.tar.gz` and a SHA-256 checksum file.
Open this repository's **Releases** page and download both files for the version
you want. The binary is built with musl to avoid a glibc-version dependency.
ARM64, Windows, and macOS binaries are not provided.

From the download directory, replacing `vVERSION` with the downloaded version:

```sh
sha256sum -c rgba-vVERSION-x86_64-unknown-linux-musl.tar.gz.sha256
tar -xzf rgba-vVERSION-x86_64-unknown-linux-musl.tar.gz
cd rgba-vVERSION-x86_64-unknown-linux-musl
./rgba --bios /path/to/bios.bin --rom /path/to/game.gba
```

Run from an X11/XWayland graphical session. For audio, install at least one of
`pw-cat` (PipeWire), `pacat` (PulseAudio), or `aplay` (ALSA). By default rgba tries
them in that order. Missing audio tools/devices produce a warning; the game
still runs. Use `--audio pipewire|pulse|alsa` to select a player or `--audio off`
to disable audio. See [audio setup and limitations](docs/AUDIO.md).

The desktop binary requires `--bios PATH`. To use the game library, omit
`--rom` and put `.gba`, `.agb`, or `.bin` files directly in
`$XDG_DATA_HOME/rgba/games` (default `~/.local/share/rgba/games`):

```sh
./rgba --bios "$HOME/.local/share/rgba/bios.bin"
```

The only game starts automatically; multiple games open a chooser showing ROM
filenames. Use Up/Down to select, Enter to launch, and Escape to exit.
`./rgba --help` lists command-line options without needing a display or images.

## Controls

These are the default keyboard bindings; change them in the Escape options menu.

| Action | Key |
| --- | --- |
| D-pad | Arrow keys or WASD |
| A / B | Z / X |
| Start / Select | Enter / Backspace |
| L / R | Q / E |
| Pause / resume | Space |
| Temporary unlimited fast-forward | Hold Tab |
| Flush cartridge save to disk | F5 |
| Save / load quick state | F6 / F7 |
| Capture PPM screenshot | F12 |
| Open / close options | Escape |

Choose Speed in the options menu for 1x, 2x, 4x, or 8x pacing. The choice is saved
per game. Releasing Tab returns to that speed. Audio is muted while paused,
during fast-forward, and above 1x. Speed changes affect host pacing, not guest
clocks, and do not skip emulated frames. Higher multipliers depend on host performance.

The options menu also provides Back to Library and Exit, both of which flush
the cartridge save. State save/load results appear on screen. Screenshots are
written under `artifacts/` without overwriting existing files.

### Gamepads

Gamepads require read access to `/dev/input/event*`. The D-pad and left stick
control movement; the bottom face button maps to A, the right face button to B,
and the shoulder buttons to L/R. Start + Select opens the options menu.

In the library and menus, use the D-pad to navigate, A/Start to select, and B to
go back (or exit the library). Choose **PAD KEYBINDS** to assign a button or axis
to a GBA action. Escape cancels; Reset Defaults restores the layout. Keyboard
and gamepad bindings are separate.

## Saves and settings

| Data | Default location |
| --- | --- |
| Game library | `~/.local/share/rgba/games/` |
| Cartridge saves and quick states | `~/.local/share/rgba/` |
| Per-game settings | `~/.config/rgba/settings/` |

`XDG_DATA_HOME` and `XDG_CONFIG_HOME` override the respective base directories.
Per-game filenames use the ROM's SHA-256 identity. Existing saves, states, and
settings beside a ROM are copied on first launch and retained; existing central
files take precedence. To migrate a ROM outside the library, launch its original
path once or copy its sidecars alongside it. `--save PATH` overrides the cartridge
save location when using `--rom`.

Use the game's own save menu before relying on a cartridge save. F5 only flushes
that save to disk; it does not create an emulator state. F6/F7 use a separate
versioned quick-state file. Loading a state also synchronizes its cartridge
backup to the configured save file. See [save and RTC policy](docs/TEST_BUILD.md).

## Build from source

Install [Rust via rustup](https://rustup.rs/), a native Linux C linker/toolchain,
and Python 3.9+ for tooling tests. The repository pins Rust 1.90.0. No native
X11 or audio SDK is required to compile the desktop.

```sh
sh scripts/cargo.sh build --locked --release -p gba-desktop
sh scripts/play.sh --bios /path/to/bios.bin --rom /path/to/game.gba
```

The launcher builds and runs the desktop. Without `--bios`, it uses `RGBA_BIOS`,
then `$XDG_DATA_HOME/rgba/bios.bin` (default `~/.local/share/rgba/bios.bin`), with
an untracked `bins/gba_bios.bin` compatibility fallback. Without `--rom`, it
opens the library. A positional ROM path also works:
`sh scripts/play.sh path/to/game.gba --scale 2`.

`scripts/cargo.sh` uses Cargo from PATH on normal hosts. Its fallback for an
isolated `.tools/` toolchain is for minimal development environments; `.tools/`
and build products are not tracked.

### Checks

```sh
sh scripts/check.sh
sh scripts/cargo.sh test --locked --workspace --release
```

The standard checks run formatting, Clippy, Rust tests, and Python tooling tests
without BIOS/game images. They do not establish hardware accuracy. Optional
asset-backed and desktop checks, including known CPU-reference mismatches and
ignored tests, are documented in [testing](docs/TESTING.md). A 32-minute automated
desktop soak passed with a virtual display and null audio sink; varied human
gameplay and real-device audio validation remain open.

### Tagged releases

[The release workflow](.github/workflows/release.yml) runs the standard checks
and release-mode tests when a `v*` tag is pushed. It then builds the x86_64 musl
desktop binary, runs its asset-free `--help` check, and publishes a GitHub Release
with the archive and checksum. The archive contains `rgba`, this README, the
MIT license, documentation, and third-party notices. No game/BIOS images or saves
are packaged. Tags containing a hyphen, such as `v0.1.0-rc.1`, become prereleases.

After the project is on GitHub, push an annotated tag to its GitHub remote
(replace the example remote name `origin` if needed):

```sh
git tag -a v0.1.0 -m "rgba v0.1.0"
git push origin v0.1.0
```

GitHub Actions must be enabled, with permission for the workflow's token to
create releases (`contents: write` on the release job). No separate release
secret is required. Use a new tag for each release; the workflow does not
replace existing releases or their assets.

To build the same package locally, install the target and run:

```sh
rustup target add x86_64-unknown-linux-musl
sh scripts/package-linux.sh v0.1.0
```

The archive and checksum are written to `artifacts/release/`. GitHub-hosted
execution and release uploads must be verified after publishing the repository.

## Development and documentation

- `crates/gba-core`: deterministic CPU, bus, devices, renderer, and cartridge.
- `crates/gba-host`: asset loading, atomic saves, audio output, and screenshots.
- `crates/gba-desktop`: X11 presentation, input, menus, library, and pacing.
- `crates/gba-headless`: bounded CLI runner and end-to-end tests. Run
  `sh scripts/cargo.sh run --release -p gba-headless -- --help` for options.
- [Plan](PLAN.md) and [progress](docs/PROGRESS.md): milestones and outstanding work.
- [Accuracy](docs/ACCURACY.md), [testing](docs/TESTING.md), and
  [validation](docs/VALIDATION.md): implemented behavior, evidence, and known failures.
- [Test-build guide](docs/TEST_BUILD.md): launch options and persistence details.
- [CPU research](docs/CPU_RESEARCH.md), [upstream CPU tests](docs/UPSTREAM_CPU_TESTS.md),
  and [hardware test ROMs](docs/HARDWARE_TESTS.md): reference comparisons and reproduction.
- [Style guide](docs/STYLE.md): Google Rust style policy.

## License

Original project code is licensed under the [MIT license](LICENSE).
Third-party code, dependencies, algorithms, and test material retain their
respective licenses and notices under [LICENSES/](LICENSES/). BIOS/game images,
saves, downloaded test assets, and generated traces are excluded from version control.
