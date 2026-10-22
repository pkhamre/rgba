# Supplied-game validation

These tests use the documented local BIOS/Emerald hashes. Images, PCM, saves,
logs and reports remain under ignored `artifacts/`; none are committed.
They do not replace hardware test ROMs, physical measurements, or listening.

## Cartridge save / restart / Continue

```sh
python3 scripts/validate_emerald.py --output artifacts/my-save-validation
```

The output directory must be new. The runner creates a disposable `.sav`,
never your normal ROM-side save. It drives the host example through ordinary
keys from `tests/scenarios/emerald-new-save.txt`; no guest memory, save states,
or BIOS shortcuts are injected. The first scenario names the player AAAAAA,
exits the truck, enters the Littleroot house, and saves through the game's menu.
It then starts a new process, selects Continue, saves again, and restarts again.

Local result with the new audio core:
- First game-generated Flash save: 131,072 bytes, complete sections 0–13,
  generation 1; game displays “AAAAAA saved the game!” in Littleroot Town.
- Continue shows AAAAAA / 0 badges / 0:03 and restores the same house/player.
- Re-saving advances the game generation to 2. `.sav.bak` is byte-identical
  to generation 1. Another process loads generation 2 and restores the world.
- Stages execute 23,380 / 5,220 / 3,810 frames, with zero core PCM queue drops.
- The new-audio-core first save matches the earlier pre-audio save byte-for-byte.

The structural Flash check is deliberately not a full checksum implementation.
Captured PPMs require visual inspection; the Python script does not perform OCR.
Local captures were inspected. A passing process alone is not a visual assertion.
WAV capture covers startup/new-game audio and the Continue path. Deterministic
PCM count must equal `master_clocks / 512`; a dropped core queue fails the test.

For direct validation sessions:

```sh
sh scripts/cargo.sh run --release -p gba-host --example scripted_session -- \
  bins/gba_bios.bin "bins/Pokemon - Emerald Version (USA, Europe).gba" \
  artifacts/disposable.sav
```

Stdin commands: `run FRAMES KEYMASK`, `shot PATH`, `wav PATH`, `stop-wav`,
`flush`, `quit`. Each run is capped at 200,000 frames, key masks at 0–1023.
Blank/comment lines are allowed. WAV/PPM files use exclusive creation.
`--soak-frames 107520` on the Python save validator adds about 30 minutes of
**emulated** time; it does not establish 30 minutes of real-time presentation.

## Desktop, pause and audio transport

Use a dedicated virtual X server, not your live desktop. Prerequisites: Xvfb,
libX11 for the optional Python capture harness, and `aplay` for ALSA-null output.
The actual emulator frontend uses Rust X11 transport and does not link libX11.

```sh
xvfb-run -a python3 scripts/validate_desktop.py \
  --save artifacts/my-save-validation/validation.sav \
  --output artifacts/my-desktop-validation
```

Default: 115,200 frames, about 32 minutes at native pacing, including boot and
Continue. It copies the input save, uses an isolated ALSA-null configuration,
checks the paused title/panel and frozen window pixels, injects F12/F5 and
left/right movement, checks clean bounded exit, and records PCM/drop counts,
wall/emulated time, RSS, executable hash, and unchanged-save status.
`_NET_WM_PID` identifies the child window; screenshots include the process ID.

Use `--frames 6000` for a roughly 102-second smoke run. That shorter run passed
on the latest build: 6,000 frames, 3,291,750 PCM frames, zero core/host drops,
pause/frozen-display checks passed, isolated save unchanged.

A longer real-time run completed 115,200 frames: 1,928.760 emulated seconds
in 1,939.539 wall seconds (32 minutes 19.5 seconds), 63,201,600 PCM frames,
zero core/host queue drops, sampled RSS 18,132 KiB, clean exit, save unchanged.
It loaded Continue and repeatedly moved left/right inside the Littleroot house.
This is a controlled soak, not a varied 30-minute human playthrough.

The long run used build 1097388 (the original two-packet host limit).
The later Pulse-specific burst fix increased that limit to eight and added
20 ms Pulse processing requests; core/gameplay code did not change. The latest
build, including process-isolated capture metadata, passed the 6,000-frame
repeat above; the fixed Pulse path passed a separate 600-frame null-server run.
Do not attribute a 32-minute test of the newer buffer configuration to the
earlier executable.

The script's default ALSA null device never reaches speakers. `--audio pulse`
or `--audio pipewire` can exercise a configured server instead. A separate
600-frame PulseAudio-null test passed with zero drops after the burst-buffer
fix; see AUDIO.md for the failing-before/passing-after result.

The user's first battle succeeded in the previous preview (user report).
That is useful gameplay evidence, not a physical-console test or a claim that
the entire game, all rendering, or audio quality is verified.
