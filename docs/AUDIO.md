# Audio implementation and validation

Core audio is generated from emulated master clocks, not wall-clock callbacks.
The output API drains signed 16-bit stereo frames at 32,768 Hz (one frame per
512 master clocks). Storage is bounded to half a second; slow/absent consumers
discard oldest frames and expose a drop count rather than growing memory.

Implemented from GBATEK's GBA Sound Controller sections:
- Square channels with duty/frequency, length, volume envelopes and channel-1
  sweep/overflow checks.
- Two-bank 4-bit wave playback, volume/75% mode, and noise shift registers.
- 512 Hz frame sequencer, length at 256 Hz, sweep at 128 Hz, envelopes at 64 Hz.
- Two real 32-byte signed Direct Sound FIFOs, timer-selected consumption,
  reset strobes, and low-watermark DMA requests.
- Per-side PSG/Direct Sound routing, volume, bias clipping and PWM quantization.
- Dynamic channel-active status and sound-register read masks.

Reference: https://github.com/mgba-emu/gbatek/blob/gh-pages/gba.md
(Sound Controller, channel 1–4, DMA sound, and control registers).
No third-party emulator/audio core is used.

Synthetic checks cover FIFO byte ordering/sign/reset/capacity, timer/DMA
requests, exact sample counts, bounded queues, deterministic clock partitioning,
length expiry, envelopes, wave bank access, noise shifts and sweep overflow.
These checks do not establish physical sound fidelity.

Known approximations: fixed 32,768 Hz host output even when SOUNDBIAS requests
higher PWM rates; no analog PWM reconstruction; wave RAM uses indexed banks,
not physical rotating-register readback. Envelope/sweep/length trigger edge
quirks, DAC offsets, powered-off behavior, FIFO underrun behavior, exact
timer/DMA arbitration, and anti-aliasing still need hardware-backed tests.
The current mixer treats master-disabled output as silence. Do not describe
this initial audio implementation as hardware-perfect.
