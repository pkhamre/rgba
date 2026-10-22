# Desktop preview

This is a testable vertical slice, **not the accepted playable release**.
Video, keyboard input, visible pause state, and Linux audio output are implemented.
Game-menu save/restart/Continue is locally verified. The first battle succeeded
in user testing; full-game and physical-hardware accuracy remain unverified.

## Launch

Requirements: Linux X11 or XWayland, a 24/32-bit TrueColor display, a local GBA
BIOS, and at least one supported ROM (in the library folder or provided with
`--rom`). A native linker is needed when building normally.

```sh
# Scan the XDG game folder; auto-start one game, choose when there are several
sh scripts/play.sh
# Launch a ROM directly instead
sh scripts/play.sh path/to/game.gba --save path/to/game.sav
```

Default scale is 3; supported integer scales are 1–6. Window resizing is disabled.
For explicit asset paths instead of the convenience launcher:

```sh
sh scripts/cargo.sh run --release -p gba-desktop -- \
  --bios "${XDG_DATA_HOME:-$HOME/.local/share}/rgba/bios.bin" \
  --rom "path/to/game.gba"
```

Arrows/WASD: D-pad. Z: A. X: B. Enter: Start. Backspace: Select. Q: L. E: R.
Space: pause. Hold Tab: fast-forward. F5: flush cartridge save. F6/F7: save/load
a machine state. F12: screenshot under `artifacts/` (binary PPM, never overwrite).
The launcher looks for `$XDG_DATA_HOME/rgba/bios.bin` (default
`~/.local/share/rgba/bios.bin`); `RGBA_BIOS` or `--bios PATH` overrides it. A
local untracked `bins/gba_bios.bin` is a compatibility fallback, not a bundled
asset.

Escape opens the options menu; Escape closes it and B backs out one menu level.
Back to Library flushes cartridge data before switching games. Exit or window
close quits and attempts to flush cartridge data. State save/load results show
a brief host-only notice. Keybinding capture rejects conflicts and includes
Reset Defaults. Focus loss releases held keys.

Without `--rom`, ROMs are scanned directly inside `$XDG_DATA_HOME/rgba/games`
(default `~/.local/share/rgba/games`); supported extensions are `.gba`, `.agb`,
and `.bin`. One game starts automatically; multiple games show a chooser. Copy
ROMs into the folder and restart to rescan. Saves/backups and states are stored
under `$XDG_DATA_HOME/rgba/saves` and `states`, named by ROM SHA-256. Settings
are under `$XDG_CONFIG_HOME/rgba/settings`. Missing ROM-side `.sav`, `.sav.bak`,
`.state`, and `.settings` files are copied on first launch and left in place;
central files take precedence on conflicts. To migrate sidecars from a ROM
outside the library folder, launch its original path once first, or copy the
sidecars alongside the ROM into the games folder.

Pause shows a PAUSED overlay and title indicator without modifying guest pixels.
The menu overlay is also host-only. The state file is versioned and ROM-bound;
load validates before replacing the running machine, then atomically syncs the
restored cartridge bytes to `.sav` while retaining `.sav.bak`. State load also
invalidates queued host PCM before later writes; samples already submitted to
the player or OS may still drain.

Boot and intro use the normal BIOS/game code; allow time or hold Tab.
Enter skips the intro/starts the title; Z confirms menu choices/dialogue.

If there is no X11 display, the frontend reports a headless alternative.
For a virtual display, with Xvfb installed:

```sh
xvfb-run -a sh scripts/play.sh path/to/game.gba --scale 1 --frames 120
```

## Save and RTC policy

- Saves are named by ROM SHA-256 under the XDG data directory; Emerald uses
  131,072 bytes. Wrong-sized existing saves are rejected rather than truncated.
- Save **in the game's menu**. F5 only flushes cartridge bytes; it is not a
  save state and cannot save arbitrary current game progress.
- Autosave waits for two wall-clock seconds without cartridge writes; F5 and
  normal exit also flush. Force-killing the process can lose pending changes.
- Save replacement is atomic, with the previous central file retained as
  `.sav.bak`.
  Existing temporary files are not overwritten. Save, backup, and temporary
  paths cannot alias either input image at session startup.
- On a write failure while running, the window stays open and the save remains
  dirty. Read the terminal error, correct the path/free space, and press F5
  before quitting. An exit-time failure cannot preserve unsaved progress in
  process memory after exit. Keep independent backups while testing.
- Desktop RTC is seeded from current Unix time (UTC) each launch, then advances
  by emulated clocks. Pausing stops it; fast-forward speeds it up. Headless RTC
  defaults to 2000-01-01 unless its host explicitly seeds it. RTC state is not
  separately persisted, and game-written dates/alarms are not implemented.

## Verified locally

- Genuine BIOS boot reaches cartridge entry at `08000000`: 7,610,482 CPU
  activities, 75,998,009 model clocks. No BIOS bypass or high-level SWI shortcut.
- Supplied Emerald runs 4,000 frames with scripted Start/A presses and renders
  Professor Birch's “Hi! Sorry to keep you waiting!” new-game dialogue.
- Xvfb desktop smoke: window maps, injected F12 produces a screenshot, and
  paused X11 window pixels match the core framebuffer byte-for-byte. The
  6,000-frame `scripts/validate_desktop.py` run passed interactive menu
  save/load/resume, duplicate-key rejection, persistent binding reload, F6/F7
  success/failure notices, and explicit Exit. It used audio off and an isolated
  save.
- A controlled 115,200-frame real-time desktop soak completed in 32 minutes
  19.5 seconds with zero core/host audio queue drops, unchanged save data,
  and stable sampled RSS. This used the earlier audio-buffer limit; the latest
  build passed a 6,000-frame repeat and a fixed-Pulse-buffer smoke. See
  VALIDATION.md for exact scope, builds, commands, and limits.
- Standard synthetic checks, release tests, and all 235 isolated mGBA CPU-table
  cases pass. The full strict CPU-vector suite still fails; see TESTING.md.
- Save size validation, SRAM save round-trip/backup, write-failure recovery,
  sidecar alias rejection, Flash commands/banking, and RTC serial control reads
  have synthetic regressions. An additional ordinary-input scenario now creates
  a real Emerald save, restarts/Continues, re-saves a newer generation, and
  Continues again with matching player/location; backups match the earlier file.

Repeat the headless game smoke with `sh scripts/smoke-emerald.sh`. This needs
the local proprietary assets, is not part of CI, and creates a fresh PPM file.
It uses a new disposable save path rather than loading your normal ROM-side save.
Inspect that image: successful execution alone is not a visual assertion.

## Known limits and outstanding acceptance work

The renderer samples registers per scanline, not per dot. Video contention,
sprite evaluation limits, precise affine/mid-line effects, and other rendering
edge cases are not hardware-validated. DMA arbitration/startup and interrupt
sampling remain approximate. STOP shares HALT behavior; serial has no peer.

PSG and Direct Sound now produce 32,768 Hz stereo PCM; Linux playback uses
pw-cat/pacat/aplay with bounded buffering and host DC filtering. Install one
client or choose `--audio off`. Audio quality on real speakers and exact PSG/
PWM/trigger edge behavior remain unverified; see AUDIO.md. Flash writes finish immediately
rather than exposing busy/toggle timing. GPIO RTC writes/alarms are incomplete.
There is no GamePak prefetch implementation, and CPU/reference disagreements
remain. Gameplay appearance is not evidence these gaps are resolved.

Remaining: a varied 30-minute human gameplay/listening session, reference-frame/
audio comparisons, physical peripheral tests, and performance on a named
end-user host. The automated soak is constrained indoor movement, not broad
gameplay coverage. Native Wayland without XWayland is unsupported.
Physical-hardware accuracy, broad gameplay coverage, and real-speaker audio
remain outstanding. State-file compatibility across emulator releases is not
promised; incompatible versions are rejected rather than migrated.
