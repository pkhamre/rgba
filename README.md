# rgba

rgba is a Game Boy Advance emulator written from scratch in Rust for Linux,
with its own ARM7TDMI interpreter, hardware models, and software renderer.
It does not use another emulator as a backend.

This is an experimental desktop preview. Testing has covered BIOS boot,
Pokémon Emerald's first battle, and saving in-game, restarting, and choosing
Continue. Full-game compatibility and hardware accuracy are still unverified.
Some independent CPU reference comparisons fail; see
[accuracy and limitations](docs/ACCURACY.md) and [validation](docs/VALIDATION.md).

## Features

- Linux X11 desktop, including Wayland sessions with XWayland, with resizable nearest-neighbor scaling when XRender is available.
- Keyboard controls and hot-plugged USB gamepads through Linux evdev.
- Audio through PipeWire, PulseAudio, or ALSA command-line players.
- Cartridge saves, a quick-save-state slot per game, and cartridge RTC support.
- Game library, configurable bindings, and per-game settings.
- Menu-selectable 1x/2x/4x/8x speed and temporary unlimited fast-forward.
- A separate headless runner for bounded execution, screenshots, and traces.

You'll need your own lawfully obtained BIOS and game images; none are included.

## Download and run

Each tagged GitHub Release includes an x86_64 Linux desktop archive named
`rgba-vVERSION-x86_64-unknown-linux-musl.tar.gz` and a SHA-256 checksum file.
Download both files from this repository's Releases page. The binary uses musl,
so it doesn't depend on a particular glibc version. There are no ARM64, Windows,
or macOS binaries.

From the download directory, replacing `vVERSION` with the downloaded version:

```sh
sha256sum -c rgba-vVERSION-x86_64-unknown-linux-musl.tar.gz.sha256
tar -xzf rgba-vVERSION-x86_64-unknown-linux-musl.tar.gz
cd rgba-vVERSION-x86_64-unknown-linux-musl
./rgba --bios /path/to/bios.bin --rom /path/to/game.gba
```

To install the binary for your user, copy it to `$HOME/bin`:

```sh
mkdir -p "$HOME/bin"
install -m 755 rgba "$HOME/bin/rgba"
```

Add `$HOME/bin` to `PATH` so you can run `rgba` from any directory. For Bash,
add this line to `~/.bashrc`:

```sh
export PATH="$HOME/bin:$PATH"
```

For fish, run this once:

```fish
fish_add_path $HOME/bin
```

Open a new shell (or reload `~/.bashrc` in Bash with `source ~/.bashrc`). Then
run rgba by name, for example `rgba --bios /path/to/bios.bin --rom /path/to/game.gba`.

Run rgba in an X11/XWayland graphical session. For audio, install at least one of
`pw-cat` (PipeWire), `pacat` (PulseAudio), or `aplay` (ALSA). By default rgba tries
them in that order. If no player or audio device is available, rgba warns you
and keeps running. Use `--audio pipewire|pulse|alsa` to select a player or
`--audio off` to disable audio. See [audio setup and limitations](docs/AUDIO.md).

The desktop binary requires `--bios PATH`. To use the game library, omit
`--rom` and put `.gba`, `.agb`, or `.bin` files directly in
`$XDG_DATA_HOME/rgba/games` (default `~/.local/share/rgba/games`):

```sh
./rgba --bios "$HOME/.local/share/rgba/bios.bin"
```

If the library has one game, it starts automatically. With multiple games,
you'll see a chooser showing the ROM filenames. Use Up/Down to select, Enter to
launch, and Escape to exit.
`./rgba --help` lists command-line options without needing a display or images.

## Controls

You can change these default keyboard bindings in the Escape options menu.

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

Choose Speed in the options menu to set 1x, 2x, 4x, or 8x pacing. rgba saves
that choice per game. Releasing Tab returns to the saved speed. Audio is muted
while paused, during fast-forward, and above 1x. Speed changes adjust host pacing without changing
guest clocks or skipping emulated frames. Your computer may not reach the higher
multipliers.

Back to Library and Exit in the options menu both flush the cartridge save.
Saving or loading a state shows the result on screen. Screenshots go under
`artifacts/` without overwriting existing files.

### Gamepads

Gamepads require read access to `/dev/input/event*`. The D-pad and left stick
control movement; the bottom face button maps to A, the right face button to B,
and RB/LB to the GBA R/L buttons. RT saves the quick state; LT loads it.
State shortcuts fire once per press, not repeatedly while a trigger is held.
Start + Select opens the options menu.

In the library and menus, use the D-pad to navigate, A/Start to select, and B to
go back (or exit the library). Choose **PAD KEYBINDS** to assign a button or axis
to a GBA action, **SAVE STATE**, or **LOAD STATE**. Select the action, then
press the desired button or trigger. Escape cancels; Reset Defaults restores
the layout. Keyboard and gamepad bindings are separate and saved per game.
Inputs already assigned to another action are rejected; reassign that action
first if you want to reuse its button.

RT/LT support standard Linux trigger buttons or Z/RZ analog axes. Extra buttons
such as R4/L4 can be captured if the controller exposes distinct Linux inputs;
buttons that mirror another button cannot be assigned separately. Existing
controller settings retain their GBA bindings; new state shortcuts use only
trigger inputs not already assigned in those settings.

## Saves and settings

| Data | Default location |
| --- | --- |
| Game library | `~/.local/share/rgba/games/` |
| Cartridge saves and quick states | `~/.local/share/rgba/` |
| Per-game settings | `~/.config/rgba/settings/` |

`XDG_DATA_HOME` and `XDG_CONFIG_HOME` override the data and configuration base
directories. Per-game filenames use the ROM's SHA-256 identity. On first launch,
rgba copies any saves, states, and settings from beside the ROM to central
storage, leaving the originals in place. It keeps any files already in central
storage. To migrate a ROM outside the library, launch its original path once
or copy its sidecars alongside it. `--save PATH` overrides the cartridge
save location when using `--rom`.

Use the game's own save menu before relying on a cartridge save. F5 only flushes
that save to disk; it does not create an emulator state. F6/F7 use a separate
versioned quick-state file. Loading a state also writes its cartridge
backup to the configured save file. See [save and RTC policy](docs/TEST_BUILD.md).

## Performance diagnostics

Use the [headless benchmark and Linux profiler](docs/PERFORMANCE.md) to measure
repeatable core workloads and identify hot functions. Benchmarks run without
window/audio output and never write cartridge saves.

## Debug logging

Add `--debug` to capture the desktop app's stdout and stderr, including startup
errors, in a new file under `$XDG_STATE_HOME/rgba/logs/` (default
`~/.local/state/rgba/logs/`). Empty or relative `XDG_STATE_HOME` values fall back
to the default. Logs include the application version, arguments, working
directory, and exit status. Without `--debug`, output behaves as before.

For a desktop launcher, append `--debug` to its `Exec=` line. Use absolute paths
there: desktop launchers do not expand `$HOME` or `~`. For example:

```ini
Exec=/home/you/apps/rgba.AppImage --bios /home/you/.local/share/rgba/gba_bios.bin --scale 4 --debug
```

Each run prints the log path to stderr and redirects subsequent output to the
file rather than the terminal. Build messages and AppImage loader errors that
occur before the application starts are not captured. Log files are created owner-readable/writable
only (`0600`); review them before sharing because they can contain local paths.
Old logs are not deleted automatically. If the log cannot be created, startup
fails with an error rather than silently running without logging.

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

`scripts/cargo.sh` uses Cargo from PATH when available. In minimal development
environments, it can fall back to an isolated `.tools/` toolchain. `.tools/`
and build products stay out of version control.

### Checks

```sh
sh scripts/check.sh
sh scripts/cargo.sh test --locked --workspace --release
```

The standard checks run formatting, Clippy, Rust tests, and Python tooling tests
without BIOS/game images. Passing them does not establish hardware accuracy.
See [testing](docs/TESTING.md) for optional tests that need images or a desktop,
known CPU-reference mismatches, and ignored tests. A 32-minute automated desktop
soak passed using a virtual display and null audio sink. Varied human gameplay
and audio on real devices still need testing.

### Tagged releases

[The release workflow](.github/workflows/release.yml) runs the standard checks
and release-mode tests when a `v*` tag is pushed. It then builds the x86_64 musl
desktop binary, runs its asset-free `--help` check, and publishes a GitHub Release
with the archive, AppImage, and their checksums. The archive contains `rgba`,
this README, the MIT license, documentation, and third-party notices. The
AppImage contains the desktop binary and launch metadata. No game/BIOS images or
saves are packaged. Tags containing a hyphen, such as `v0.1.0-rc.1`, become prereleases.

After the project is on GitHub, push an annotated tag to its GitHub remote
(replace the example remote name `origin` if needed):

```sh
git tag -a v0.1.0 -m "rgba v0.1.0"
git push origin v0.1.0
```

Enable GitHub Actions and allow the workflow's token to create releases
(`contents: write` on the release job). You don't need a separate release secret.
Use a new tag for each release; the workflow won't replace existing releases
or their assets.

To build locally, install the target and run:

```sh
rustup target add x86_64-unknown-linux-musl
sh scripts/package-linux.sh v0.1.0
APPIMAGETOOL=/path/to/appimagetool sh scripts/package-appimage.sh v0.1.0
```

`appimagetool` must be installed locally or specified with `APPIMAGETOOL`. The
tarball, AppImage, and SHA-256 checksums go in `artifacts/release/`. Audio
playback still uses host-installed `pw-cat`, `pacat`, or `aplay`; use `--audio
off` when none is available. The workflow still needs
an end-to-end run on GitHub to verify the hosted build and release upload.

## Development and documentation

See [CONTRIBUTION.md](CONTRIBUTION.md) for contributing through merge requests
(GitHub pull requests), and [AGENTS.md](AGENTS.md) for coding-agent instructions.

- `crates/gba-core`: deterministic CPU, bus, devices, renderer, and cartridge.
- `crates/gba-host`: asset loading, atomic saves, audio output, and screenshots.
- `crates/gba-desktop`: X11 presentation, input, menus, library, and pacing.
- `crates/gba-headless`: bounded CLI runner and end-to-end tests. Run
  `sh scripts/cargo.sh run --release -p gba-headless -- --help` for options.
- [Progress](docs/PROGRESS.md): milestones and outstanding work.
- [Accuracy](docs/ACCURACY.md), [testing](docs/TESTING.md), and
  [validation](docs/VALIDATION.md): implemented behavior, evidence, and known failures.
- [Test-build guide](docs/TEST_BUILD.md): launch options and persistence details.
- [CPU research](docs/CPU_RESEARCH.md), [upstream CPU tests](docs/UPSTREAM_CPU_TESTS.md),
  and [hardware test ROMs](docs/HARDWARE_TESTS.md): reference comparisons and reproduction.
- [Style guide](docs/STYLE.md): Google Rust style policy.

## License

Original project code is licensed under the [MIT license](LICENSE).
Third-party code, dependencies, algorithms, and test material retain their
own licenses and notices under [LICENSES/](LICENSES/). BIOS/game images, saves,
downloaded test assets, and generated traces stay out of version control.
