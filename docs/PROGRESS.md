# Implementation progress

## Foundation and CPU/bus

Pinned Rust workspace, Google Rust style, safe-Rust/public-doc lints, synthetic
CI checks, licensed reference preparation, and small logical commits are in
place. Local debug/release checks pass; GitHub Actions was not run here.

Standard result: **154 Rust tests pass (including one doctest), 3 are ignored,
and 23 Python tests pass**. Both external tests remain optional/ignored by
default.
The isolated mGBA baseline passes 235/235 cases. Strict CPU comparison still
fails unchanged: **2,124,072 matches / 125,928 failures** out of 2,250,000,
27 of 45 groups fully matching. No reference mismatches are waived.

ARM/Thumb, banked state, pipeline, exceptions, memory map, baseline N/S/I timing,
WAITCNT, and multiply carry reconstruction are present. CPU/bus accuracy
acceptance remains open: physical reserved-encoding evidence, complete bus
signals/LOCK, precise cycle ordering, BIOS latch, open bus, prefetch, and
contention. See CPU_RESEARCH.md and ACCURACY.md.

## Runnable preview and requested follow-up

Provisional peripherals/presentation span milestones 2–5; that does not accept
their hardware-validation exit criteria.

Implemented:
- LCD line/status/IRQ timing, IRQ entry, timers/cascade, DMA, keypad, HALT,
  system I/O, and scanline software graphics.
- Genuine BIOS boot and SWI execution, without a boot shortcut.
- Banked Flash/SRAM, GPIO RTC reads/control, atomic saves/backups/alias guards.
- X11/XWayland presentation, integer scaling, native wall-clock pacing, and
  an Escape-accessible options menu with explicit Exit, state actions, and a
  Keybindings/Reset Defaults page. Brief host-only state-result notices are
  shown for quick saves and loads.
- ROM-specific key remapping for all GBA and emulator actions, duplicate-key
  rejection, settings persistence, and F6/F7 quick state hotkeys. Defaults keep
  existing controls; Escape now opens the menu instead of quitting.
- Visible host-only PAUSED panel and title indicator; guest pixels/state are
  not modified. Paused window stability is tested under Xvfb.
- Clocked square/sweep/envelope, wave/noise PSG, real timer-fed Direct Sound
  FIFOs/DMA, stereo routing/mixing, and bounded 32,768 Hz PCM output.
- Linux playback through pw-cat/pacat/aplay, host DC filtering, bounded queues,
  nonfatal device errors, mute during pause/fast-forward, and WAV capture.

## Verified evidence

- BIOS reaches `08000000` at 75,998,009 model clocks. The audio scheduler now
  splits HALT into smaller activities: 7,610,482 activities versus the older
  7,492,424, with unchanged boot clocks (not extra guest instructions).
- Emerald reaches Professor Birch in 4,000 scripted frames.
- The user reports successful first-battle gameplay in the previous preview.
- Ordinary input now creates a real game-menu save in Littleroot, starts a new
  process, Continues with matching AAAAAA/location, re-saves generation 2,
  and Continues again. The host backup matches generation 1 exactly. New-core
  generation 1 matches the pre-audio save byte-for-byte.
- PCM captures are stereo/32,768 Hz, nonconstant, and have deterministic counts
  with zero core queue drops. A capture exposed/fixed muted-PSG DC clipping.
- A real-time Xvfb/ALSA-null desktop soak completed **115,200 frames**,
  **1,928.760 emulated seconds / 1,939.539 wall seconds**, **63,201,600 PCM
  frames**, zero core/host drops, unchanged save, clean exit, and stable sampled
  RSS (18,132 KiB). It exercised Continue, pause/F12/F5, and repeated indoor
  left/right movement, not varied game progression.
- That long run used build 1097388's two-packet buffer. A later Pulse-null test
  exposed burst drops; an eight-packet ceiling and explicit process interval
  fixed the 600-frame repeat. The latest build passed a separate 6,000-frame
  desktop repeat: 3,291,750 PCM frames, zero drops, pause checks and save intact.
  Do not attribute the older long run to the newer buffer configuration.

Reproduction/build details: VALIDATION.md. Sound model/transport limits:
AUDIO.md. Launch/controls/save/RTC policies: TEST_BUILD.md.

## PipeWire / auto follow-up

A user exposed a headerless-PCM decoding failure despite working MP3 playback.
PipeWire now receives a self-describing, indefinitely sized AU stream. Native
PipeWire 0.3.65 virtual-sink probes passed for explicit/auto selection. Auto
also recovers from late process/writer failure, including while paused; a
controlled late-error test skipped missing pacat and reached real ALSA-null.
Startup means process started, not confirmed playback. See AUDIO.md.

ALSA underruns on the user's real device remain unresolved by this change.
Queue-drop counters are not hardware-underrun counters; the previous null-sink
soak does not establish real-device pacing or speaker quality.

## Remaining work

The approved release checkpoint remains open. There has been no physical-GBA
capture, full hardware-test-ROM execution, reference video/audio comparison, or
speaker listening test here. The controlled soak is not a varied 30-minute human
playthrough, and performance on the user's host is not certified.

The options/state implementation has unit coverage for state encoding, ROM
binding and corruption rejection; atomic Session slots and cartridge-save sync;
settings round trips and duplicate rejection; menu navigation/capture; and
host-only overlays. Selectable speed controls are deferred at the user's
request; Tab fast-forward remains. The isolated-ROM Xvfb acceptance run passed
at scale 3: 6,000 frames / 100.222 emulated seconds in 106.919 wall seconds. It
covered menu state save/load/resume, duplicate-key rejection, keybinding
persistence/reload, F6/F7 success and failure notices, and explicit Exit at
frame 4 of a second launch. Both isolated saves were unchanged. Audio was
disabled, so this is not audio-device evidence.

Next: hardware-backed CPU/bus/device/audio tests, reference captures, real-device
listening/latency checks, and broader gameplay validation.
