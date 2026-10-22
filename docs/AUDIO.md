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

## Linux output

Desktop audio defaults to `--audio auto`. It tries installed `pw-cat`
(PipeWire), `pacat` (PulseAudio), then `aplay` (ALSA). Select a backend with
`--audio pipewire|pulse|alsa`, or disable output with `--audio off`.
No shell is invoked and no native audio SDK is needed to compile the emulator.
If tools/devices are unavailable, the window continues silently with a terminal
warning. Device/player failures do not stop guest execution.

A worker applies a deterministic host-side DC-blocking high-pass filter and
writes stereo signed-16 little-endian PCM. The player resamples to the device
rate. At most two packets (each capped at 2,048 frames) are queued; a full queue
drops output instead of blocking or changing emulated time. Drop counts are
reported on exit. OS/player buffers add latency beyond this queue; 80 ms is
requested, not guaranteed. Pause and fast-forward mute new packets; previously
buffered sound drains. Player processes are stopped/reaped on exit.

CLI references checked:
https://docs.pipewire.org/1.6/page_man_pw-cat_1.html
https://manpages.debian.org/bookworm/pipewire-bin/pw-cat.1.en.html
https://manpages.debian.org/bookworm/pulseaudio-utils/pacat.1.en.html
https://manpages.debian.org/bookworm/alsa-utils/aplay.1.en.html

Host regressions cover PCM WAV headers/endianness/exclusive creation, DC removal,
missing players, and a stalled consumer not blocking submission or shutdown.
A 120-frame Xvfb + ALSA-null run produced 65,835 PCM frames with zero core/host
queue drops. Null-device output tests transport, not speakers or sound quality.
