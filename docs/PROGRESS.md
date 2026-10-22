# Implementation progress

## Foundation

Pinned Rust workspace, lockfile, Google Rust style policy, safe-Rust/public-doc
lints, bounded runners, synthetic CI checks, licensed reference preparation,
and small logical commits are in place. GitHub Actions is configured, not run
here. The local isolated musl build uses an ignored Zig linker for dependency
build scripts; normal Linux hosts use a native linker.

## CPU/bus: implementation present, accuracy acceptance still open

ARM/Thumb interpreter, banked state, explicit pipeline, exceptions, memory map,
baseline N/S/I timing, WAITCNT, multiply carry reconstruction, and independent
CPU replay are implemented. BIOS initialization exposed an SPSR storage defect:
arbitrary saved bits must be storable without switching the current mode.
That correction has a synthetic BIOS regression and removes 3,053 strict
reference failures.

Current standard checks: **106 Rust unit/integration tests, one doctest, and
13 Python tests pass**. Both optional external tests are ignored by default.
The isolated mGBA baseline passes 235/235 cases. Strict SingleStepTests comparison
still fails: **2,124,072 matches / 125,928 failures** out of 2,250,000; 27 of 45
groups match fully. The first diagnostics remain documented, not waived.
See CPU_RESEARCH.md and UPSTREAM_CPU_TESTS.md for provenance and limits.

Physical reserved-encoding evidence, exact transactions/code-data/LOCK signals,
internal-cycle placement, BIOS latch, open bus, prefetch, and contention remain
open. No physical-GBA capture or complete hardware-test ROM was run.

## Testable vertical slice: provisional milestones 2–4

The request for something testable led to provisional devices/presentation while
CPU/bus accuracy work remains open. This does not accept the later milestones'
hardware-validation exit criteria.

Implemented:
- LCD line timing/status/IRQ, interrupt entry, timers/cascade, DMA triggers,
  keypad, HALT progression, system I/O, and a scanline software renderer.
- Normal BIOS boot to cartridge entry, with no boot/SWI bypass.
- Banked 128 KiB Flash, ID/program/erase, GPIO RTC reads/control, SRAM, and
  shared host save loading/atomic replacement/backups/error handling.
- Linux X11/XWayland desktop, integer scaling, keyboard controls, pause,
  fast-forward, screenshots, save flushing, and native frame-rate pacing.
- Headless frame execution, screenshots, and scripted held-key changes.

Verified with the supplied local assets:
- BIOS reaches `08000000` after 7,492,424 CPU activities / 75,998,009 clocks.
- 4,000 scripted frames reach Professor Birch's new-game introduction.
- Xvfb maps the desktop window; injected F12 works; captured window pixels
  match the core framebuffer; Escape and bounded window runs exit successfully.
- Synthetic save round-trip/backup/error tests pass; a real in-game save/
  continue cycle has **not** yet been established.

See TEST_BUILD.md for launch commands and explicit timing/rendering/RTC limits.
No audio output or PSG synthesis exists. No hardware-accuracy claim follows
from a successful game screenshot.

## Next work and release gate

1. Continue independent CPU/bus and hardware-backed peripheral validation.
2. Complete sound generation/mixing and Linux audio integration; test FIFO/
   DMA/timer interactions before claiming audible correctness.
3. Exercise player setup, controllable movement, the first battle, and genuine
   Emerald save/restart/continue; turn findings into regressions.
4. Compare video/audio against independent evidence; improve scanline/dot and
   arbitration accuracy rather than treating provisional rendering as final.
5. Profile on a named host and complete the 30-minute interactive release test.

The approved first-release acceptance checkpoint remains open. This preview is
ready for user testing, not certified for full-game compatibility or safe use of
a sole valuable save.
