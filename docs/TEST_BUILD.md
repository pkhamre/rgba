# Runnable test build work

The first vertical slice adds real BIOS boot, LCD line timing, IRQ entry,
timers/cascade, DMA transfers, keypad state and a scanline software renderer.
The supplied BIOS reaches cartridge entry at PC 08000000 without bypassing boot.
The first game run currently reaches Flash probing at frame 281; cartridge
support and the desktop window are the next implementation steps in this build.

Headless frame mode is available with `--frames N --screenshot output.ppm`.
Input can be scripted with `--input FRAME:start` and `--input FRAME:none`.

Accuracy limits: this renderer samples registers once per scanline, not per dot.
DMA starts at CPU activity boundaries; arbitration/startup edge cases are not
hardware-perfect. Audio register/FIFO timing is modeled enough to request DMA,
but audio output and PSG synthesis are not implemented. Serial registers are
latched with no connected peer. STOP currently shares HALT behavior. These are
explicit development limitations, not hardware-accuracy claims.
