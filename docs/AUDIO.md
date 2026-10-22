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
quirks, frequency-write retiming, DAC offsets, powered-off behavior, FIFO underrun behavior, exact
timer/DMA arbitration, and anti-aliasing still need hardware-backed tests.
The current mixer treats master-disabled output as silence. Do not describe
this initial audio implementation as hardware-perfect.

## Linux output

Desktop defaults to `--audio auto`: pw-cat (PipeWire), pacat (PulseAudio),
then aplay (ALSA). Select `--audio pipewire|pulse|alsa` to restrict selection
to one player, or `--audio off` to disable host output. No shell is invoked
and no native audio SDK is needed to compile the emulator.

A worker applies host-side DC filtering and writes signed-16 stereo PCM:
- PipeWire receives self-describing AU: 24-byte header, 32,768 Hz, two channels,
  big-endian PCM16, standard unknown-length marker. AU works on unseekable stdin,
  has no RIFF length ceiling, and does not need version-specific raw-input flags.
- Pulse/ALSA receive raw little-endian samples with explicit rate/channel flags.
- The player resamples to the actual device rate. Eight packets of up to 2,048
  frames are bounded; full queues reject output rather than block guest timing.
- 80 ms device latency is requested, not guaranteed. Pulse also requests 20 ms
  processing intervals. Normal desktop packets contain about 549 frames.
- Pause/fast-forward mute new output; existing device buffers drain. Shutdown
  stops/reaps the player and bounds waiting for its writer thread.

The initial two-packet ceiling dropped 16,458 frames in a 600-frame Pulse-null
run (51,022 with a smaller process interval). Eight packets removed drops in
that repeat; it is not proof of real-device underrun-free playback.

### PipeWire framing and auto recovery

A user reported working MP3 playback but `sndfile: ... Format not recognised`
for the emulator. Our headerless stream plus format/rate options did not select
raw decoding on that user's pw-cat. CLI behavior varies by version; this was
our integration defect, not evidence of a stopped/missing PipeWire service.

AU framing removes that ambiguity. Local validation uses PipeWire/pw-cat 0.3.65,
not the user's unreported version: native explicit and auto probes each ran for
10 seconds through a real server/48 kHz virtual sink, manually linked stereo
ports, with zero queue drops and no decoder failure. A separate 600-frame
Emerald desktop run in auto mode used pw-cat and generated 329,175 PCM frames
with zero core/host queue drops and no format error. Missing RTKit/D-Bus in the
isolated fixture produced scheduling warnings, not a format failure. This is
not a same-version reproduction of the original raw-mode error.

Auto now supervises process exit and writer errors on every poll/submission,
including empty packets while paused. A player that fails after the original
40 ms startup window is stopped/reaped and selection advances once to each
remaining candidate. Missing intermediate executables are skipped. Explicit
selection never silently changes backend. Exhaustion reports silence once;
there is no infinite respawn loop. A switch may introduce a short gap and
bounded process-start overhead.

Startup logs say a player **started**, not that playback is confirmed. A live
but unrouted/muted process cannot be diagnosed by this supervisor alone.
Host drop counts describe queue rejections/unavailable output, not device XRUNs,
in-flight loss in a dying process, or confirmed delivery to speakers. Zero host
drops can coexist with ALSA underruns. Hardware-clock pacing/underrun work is
separate and is not claimed fixed by this framing/recovery change.

Regression tests cover AU metadata/endianness, filtering, immediate and delayed
child exit, failure with closed stdin while the child stays alive, paused
recovery, explicit-backend exhaustion, missing tools and stalled consumers.
An end-to-end injected late failure after the AU header fell through a missing
pacat executable to real aplay/null; explicit pipewire correctly stayed silent
and made the probe fail instead of claiming success.

### Startup reserve and producer timing

A user still heard normal-gameplay skips through pw-cat after the framing fix.
Inspection found no actual startup reserve: queue capacity was available, but
the writer sent each frame immediately. The writer now collects 4,096 frames
(125 ms) of real PCM before delivery. This adds startup/resume latency; it does
not insert silence, duplicate samples, change guest clocks, or resample.

Mute transitions tag packets with an epoch. Old queued packets are discarded,
and resume/fast-forward release requires a fresh reserve and filter reset,
even if the writer received no messages while muted. Storage stays bounded:
at most 4,095 pending frames plus one accepted packet (maximum 2,048) before
priming, in addition to the existing bounded input channel.

Synthetic checks cover exact ordering, bounded overshoot, re-priming, old-epoch
rejection, and a modelled 62.5 ms producer stall. The model misses deadlines
without a reserve and meets them with it; this is not a hardware measurement.

Exit/probe logs now report `max_submit_gap_ms`, `gaps_over_40ms`, and
`producer_hz` against the required 32,768. Pause/fast-forward/restart gaps are
excluded. These measure PCM submission, not DAC deadlines or device XRUNs.
A producer persistently below the target cannot be fixed by a finite reserve.

This exposed a local default-3x-scale performance problem: a 600-frame
Xvfb/PipeWire run delivered about 26,121 frames/sec (12.695 s wall time for
10.046 s emulated time), despite zero queue drops. Pixel-presentation overhead
is being investigated separately; this reserve alone is not a solution to
sustained underproduction.

A native virtual-sink probe with injected 60 ms stalls every second completed
before and after priming with zero host queue drops; the new timing metrics
correctly reported eight gaps around 75.7 ms. Thus those queue counts alone do
not demonstrate elimination of audible skips. Device XRUNs/speakers were not
measured in that fixture.

### Reproduce without BIOS or ROM

A quiet 256 Hz tone (right channel at half volume, mono-safe):
```sh
sh scripts/cargo.sh run --release -p gba-host --example audio_probe -- auto 3
sh scripts/cargo.sh run --release -p gba-host --example audio_probe -- pipewire 3
# Diagnostic: inject a 60 ms producer stall once per second (max 500 ms):
sh scripts/cargo.sh run --release -p gba-host --example audio_probe -- pipewire 10 60
```
The probe reports the final player, rejects exhausted output, and fails on host
queue drops. Speakers, sound quality, device XRUNs, and long-session drift still
need real-device testing. Earlier 120-frame ALSA-null, real-time desktop soaks,
and latest-build distinctions are documented in VALIDATION.md; do not extend
those earlier results to the new PipeWire transport automatically.

References:
- https://docs.pipewire.org/1.6/page_man_pw-cat_1.html
- https://manpages.debian.org/bookworm/pipewire-bin/pw-cat.1.en.html
- https://manpages.debian.org/bookworm/pulseaudio-utils/pacat.1.en.html
- https://manpages.debian.org/bookworm/alsa-utils/aplay.1.en.html
- https://www.mmsp.ece.mcgill.ca/Documents/AudioFormats/AU/AU.html
- libsndfile 1.2.0 `src/au.c` (format reference only; no code imported).
