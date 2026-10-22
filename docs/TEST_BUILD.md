# Desktop preview

This is a testable vertical slice, **not the accepted playable release**.
Video and keyboard input work; audio output is not implemented.

## Launch

Requirements: Linux X11 or XWayland, a 24/32-bit TrueColor display, and the
supplied local BIOS/ROM files. A native linker is needed when building normally.

```sh
sh scripts/play.sh
# Optional:
sh scripts/play.sh --scale 2
sh scripts/play.sh path/to/game.gba --save path/to/game.sav
```

Default scale is 3; supported integer scales are 1–6. Window resizing is disabled.
For explicit asset paths instead of the convenience launcher:

```sh
sh scripts/cargo.sh run --release -p gba-desktop -- \
  --bios bins/gba_bios.bin \
  --rom "bins/Pokemon - Emerald Version (USA, Europe).gba"
```

Arrows/WASD: D-pad. Z: A. X: B. Enter: Start. Backspace: Select. Q: L. E: R.
Space: pause. Hold Tab: fast-forward. F5: flush save. F12: screenshot under
`artifacts/` (binary PPM, never overwrite). Escape/window close: quit and flush.
Focus loss releases held keys. The keyboard map is read at startup.

Boot and intro use the normal BIOS/game code; allow time or hold Tab.
Enter skips the intro/starts the title; Z confirms menu choices/dialogue.

If there is no X11 display, the frontend reports a headless alternative.
For a virtual display, with Xvfb installed:

```sh
xvfb-run -a sh scripts/play.sh --scale 1 --frames 120
```

## Save and RTC policy

- Saves default to the ROM path with extension `.sav`; Emerald uses 131,072
  bytes. Wrong-sized existing saves are rejected rather than truncated.
- Save **in the game's menu**. F5 only flushes cartridge bytes; it is not a
  save state and cannot save arbitrary current game progress.
- Autosave waits for two wall-clock seconds without cartridge writes; F5 and
  normal exit also flush. Force-killing the process can lose pending changes.
- Save replacement is atomic, with the previous file retained as `.sav.bak`.
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

- Genuine BIOS boot reaches cartridge entry at `08000000`: 7,492,424 CPU
  activities, 75,998,009 model clocks. No BIOS bypass or high-level SWI shortcut.
- Supplied Emerald runs 4,000 frames with scripted Start/A presses and renders
  Professor Birch's “Hi! Sorry to keep you waiting!” new-game dialogue.
- Xvfb desktop smoke: window maps, injected F12 produces a screenshot, paused
  X11 window pixels match the core framebuffer byte-for-byte, and Escape
  exits successfully. A separate bounded 40-frame window run also exits cleanly.
- Standard synthetic checks, release tests, and all 235 isolated mGBA CPU-table
  cases pass. The full strict CPU-vector suite still fails; see TESTING.md.
- Save size validation, SRAM save round-trip/backup, write-failure recovery,
  sidecar alias rejection, Flash commands/banking, and RTC serial control reads
  have synthetic regressions. This does **not** establish an Emerald
  save/restart/continue round-trip.

Repeat the headless game smoke with `sh scripts/smoke-emerald.sh`. This needs
the local proprietary assets, is not part of CI, and creates a fresh PPM file.
Inspect that image: successful execution alone is not a visual assertion.

## Known limits and outstanding acceptance work

The renderer samples registers per scanline, not per dot. Video contention,
sprite evaluation limits, precise affine/mid-line effects, and other rendering
edge cases are not hardware-validated. DMA arbitration/startup and interrupt
sampling remain approximate. STOP shares HALT behavior; serial has no peer.

Sound registers/FIFO timing can request DMA, but there is no PSG synthesis,
mixed sample stream, or audio device output. Flash writes finish immediately
rather than exposing busy/toggle timing. GPIO RTC writes/alarms are incomplete.
There is no GamePak prefetch implementation, and CPU/reference disagreements
remain. Gameplay appearance is not evidence these gaps are resolved.

Not yet verified: player setup through the first controllable outdoor area,
first battle, Emerald menu save/restart/continue, a 30-minute interactive
session, reference-frame comparisons, or sustained real-time video/audio on a
named end-user host. Native Wayland without XWayland is unsupported.
