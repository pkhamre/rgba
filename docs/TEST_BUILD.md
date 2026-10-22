# Runnable test build work

The first vertical slice adds real BIOS boot, LCD line timing, IRQ entry,
timers/cascade, DMA transfers, keypad state and a scanline software renderer.
The supplied BIOS reaches cartridge entry at PC 08000000 without bypassing boot.
Flash probing now works, and the supplied game renders its Game Freak intro.
Banked 128 KiB Flash, chip ID, erase/program commands, GPIO RTC reads/control,
and atomic sidecar saves are implemented. Desktop integration is next.
Save writes expose a monotonic revision counter so the frontend can wait for a
quiet period before autosaving. Save, backup, and temporary paths are checked
against both input images, with synthetic alias and revision regressions.

Headless frame mode is available with `--frames N --screenshot output.ppm`.
Input can be scripted with `--input FRAME:start` and `--input FRAME:none`.

Accuracy limits: this renderer samples registers once per scanline, not per dot.
DMA starts at CPU activity boundaries; arbitration/startup edge cases are not
hardware-perfect. Audio register/FIFO timing is modeled enough to request DMA,
but audio output and PSG synthesis are not implemented. Serial registers are
latched with no connected peer. STOP currently shares HALT behavior. These are
explicit development limitations, not hardware-accuracy claims. Flash writes
complete instantly rather than exposing busy/toggle timing. RTC date/time reads
advance from a frontend seed; game-written date/time changes and alarm/IRQ modes
are not yet supported.
