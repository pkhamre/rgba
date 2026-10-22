# Rust GBA emulator implementation plan

## Agreed requirements

- Follow the Google Rust Style Guide: https://google.github.io/styleguide/rustguide.html (see `docs/STYLE.md`).

- Linux desktop emulator written in Rust.
- Implement our own ARM7TDMI CPU and GBA hardware, not a wrapper around another emulator.
- Target hardware accuracy, including pipeline and bus timing, DMA arbitration, and rendering timing. No claim of cycle accuracy without supporting tests.
- First release: playable Pokémon Emerald with audio, keyboard controls, and persistent cartridge saves.
- Rust host-integration crates, public hardware test ROMs, and mGBA comparisons are allowed, subject to licensing.
- Primary specification: https://github.com/mgba-emu/gbatek/blob/gh-pages/gba.md (GBA and ARMv4T sections only; exclude later ARM/DS features).

## Verified inputs and prerequisites

- `bins/gba_bios.bin`: 16,384 bytes; SHA-256 `fd2547724b505f487e6dcb29ec2ecff3af35a841a77ab2e85fd87350abd36570`. Hash recorded; authenticity is not independently certified.
- `bins/Pokemon - Emerald Version (USA, Europe).gba`: 16,777,216 bytes; title `POKEMON EMER`, game code `BPEE`; SHA-256 `a9dec84dfe7f62ab2220bafaef7479da0929d066ece16a6885f6226db19085af`.
- ROM contains `FLASH1M_V103` and `SIIRTC_V001` signatures. Plan for 128 KiB banked Flash and cartridge GPIO RTC; verify behavior against tests and the cartridge protocol.
- `cargo` and `rustc` are not currently on PATH. Check existing installations, then arrange a stable Rust toolchain before building.
- Check availability of Linux windowing/audio, development packages, and graphical-session access. A headless environment can validate core behavior but cannot establish interactive audio/display quality.
- Keep BIOS, game ROM, downloaded test binaries, and generated saves out of version control. Record hashes locally for reproducible runs without redistributing proprietary inputs.

## Architecture

Cargo workspace with:

- `crates/gba-core`: deterministic, host-independent emulation library; CPU, bus, scheduler, interrupts, DMA, timers, PPU, APU, cartridge, and keypad.
- `crates/gba-desktop`: Linux window, keyboard mapping, frame presentation, audio device integration, ROM/BIOS paths, and save-file lifecycle.
- `crates/gba-headless`: bounded execution, scripted input, trace capture, framebuffer/audio artifacts, and regression tests.
- `tests/` and `docs/`: test manifests, hardware behavior notes, reference links, and compatibility results.

Use integer master-clock timestamps and deterministic device ordering. CPU fetch/data accesses go through a timing-aware bus that records sequential/nonsequential access and ownership. Model ARM7TDMI pipeline state explicitly. DMA can take the bus while timers, display, and sound continue progressing. Keep cartridge RTC host-time policy outside the deterministic core.

Graphics should expose display fetch/render events at the granularity needed for register-write effects and contention. Do not make a whole-frame or scanline-only renderer the final accuracy model. Presentation and audio buffering must not control emulated hardware time.

Start with an interpreter and software rendering; defer JIT, parallel emulation, and GPU emulation. Choose frontend crates after checking toolchain/system-library support and current crate documentation.

## Ordered implementation milestones

### 0. Reproducible foundation

- Establish toolchain and Linux host prerequisites; pin the toolchain and commit dependency lockfile.
- Create workspace, ignore rules, CI, formatting/lint checks, and asset-path configuration.
- Inventory licensed test suites and reference tools, recording versions and expected results.
- Define trace schemas, cycle counters, execution limits, and test fixture conventions.
- Exit: clean build, unit tests, and headless CLI input validation; missing/invalid assets produce useful errors.

### 1. Bus and ARM7TDMI execution

- Implement memory regions, mirroring, access widths, alignment, byte-write quirks, BIOS visibility/protection, and open-bus behavior.
- Implement ARMv4T ARM and Thumb instruction sets, flags, barrel shifter edge cases, banked registers, PSRs, exceptions, and ARM/Thumb transitions.
- Model fetch/decode/execute pipeline effects, PC semantics, branch refills, sequential/nonsequential accesses, waitstates, and instruction internal cycles.
- Test arithmetic boundaries, misaligned transfers, block transfers, exception returns, and undefined instructions explicitly.
- Exit: selected CPU and memory tests pass with documented timing assertions. Do not silently treat missing valid instructions as NOPs.

### 2. System timing and genuine BIOS boot

- Build shared event scheduling, reset state, interrupt controller, HALT/STOP behavior, timers/cascade, WAITCNT/prefetch, and all four DMA channels.
- Implement DMA priority/arbitration, triggers, repeat/reload, transfer timing, and interrupt generation.
- Implement display timing/status/interrupts early enough for real BIOS execution.
- Execute the supplied BIOS, including SWIs through its exception vectors; do not replace BIOS calls with high-level shortcuts on the accuracy path.
- Exit: hardware tests cover IRQ, timer, and DMA edge cases; BIOS reaches the cartridge entry point reproducibly with traces available.

### 3. Accurate graphics and interactive display

- Implement display memory/register rules, modes 0–5, text/affine backgrounds, sprites, priorities, windows, mosaic, blending, and brightness.
- Model line/dot timing, affine state updates, mid-frame writes, sprite evaluation constraints, and video-memory contention; validate disputed details with hardware tests.
- Add Linux presentation with integer scaling and keyboard input; keep headless framebuffer capture equivalent to desktop core output.
- Exit: selected graphics tests pass; BIOS animation and Emerald title/intro render without known major defects, verified with frame captures and reference comparisons.

### 4. Cartridge persistence, RTC, and gameplay

- Implement the detected Flash protocol, banking, IDs, erase/program operations, and relevant timing.
- Implement GPIO RTC commands and time progression with deterministic test-time and configurable real-time policies.
- Persist cartridge saves atomically; validate file size and preserve recoverable backups on failure. Cartridge saves are distinct from emulator save states.
- Add scripted key sequences and validate new-game flow, menus, movement, and a battle.
- Exit: create an in-game save, close the emulator, restart, and continue successfully with matching player/location data; RTC behavior is tested.

### 5. Accurate sound and real-time pacing

- Implement PSG square/wave/noise channels, frame sequencing, envelopes/sweep/length, wave RAM behavior, Direct Sound FIFOs, timer/DMA coupling, mixing, and bias behavior.
- Test peripheral side effects before audible output is complete; sound timing must not be deferred where it affects gameplay execution.
- Resample core output for the Linux audio device, handle bounded buffering, and pace without changing guest timing.
- Exit: deterministic audio tests pass; Emerald has working music/effects without persistent underruns or audible timing defects during an interactive run.

### 6. Release validation and performance

- Run the selected CPU, bus, DMA, timer, display, audio, Flash, and RTC suites; publish exact passes, failures, and unsupported cases.
- Compare bounded traces and frame/audio artifacts with mGBA where practical. Reference-emulator agreement is useful evidence, not proof of hardware accuracy.
- Profile and optimize only after correctness baselines exist; rerun regressions after each optimization.
- Validate sustained full-speed execution near the GBA's native ~59.73 frames/second on a named Linux host.
- Document launch commands, controls, save/RTC policy, test reproduction, known limitations, and dependency licenses.

## First-release acceptance checkpoint

Using the supplied assets and the normal BIOS boot path:

1. Boot through the BIOS into Emerald, with usable video, audio, and controls.
2. Start a new game, complete initial setup, reach the first controllable outdoor area, and complete the first battle.
3. Save using the game's menu; terminate and relaunch; continue from that save with the expected progress.
4. Run an additional 30-minute interactive gameplay session without crashes, hangs, major rendering failures, or persistent audio underruns.
5. Meet real-time speed on the recorded validation host and publish hardware-test results and remaining accuracy gaps.

This checkpoint is a proposed concrete definition of “playable,” not a claim that the entire game is verified. Full-game completion and exhaustive hardware accuracy are later validation work.

## Explicit initial exclusions

- GB/GBC and Nintendo DS emulation.
- Link-cable/wireless multiplayer and accessory emulation beyond Emerald's RTC.
- Browser, Windows, and macOS frontends.
- Rewind, cheats, debugger GUI, gamepad support, and JIT.
- Guaranteed compatibility with all GBA games or an unqualified cycle-perfect claim.

## Agreed follow-up: desktop options and save states

- Escape opens an in-window options menu. Escape from the menu returns to gameplay; an explicit Exit item flushes the cartridge save and quits.
- Provide one atomic quick-save-state slot beside the ROM. State files are versioned and bound to the loaded ROM; loading validates fully before replacing the running machine.
- A successful load restores the complete emulated machine and atomically synchronizes its cartridge backup to the configured `.sav`, preserving existing backup behavior. Cartridge saves and emulator states remain separate files.
- Provide persistent runtime speed selection at 1x, 2x, and 4x; a fresh launch starts at 1x.
- Provide a Keybinds submenu for all GBA buttons and emulator actions, including menu navigation, pause, fast-forward, state save/load, and exit. Persist bindings in a settings file beside the ROM. Reject duplicate bindings with a clear explanation and offer Reset Defaults.
- Implement/test core state round trips and corrupt/incompatible state rejection before desktop integration. Then add settings persistence, key capture/conflict handling, menu UI, speed pacing, and desktop validation in separate logical steps.

## Current implementation status

See `docs/PROGRESS.md`, `docs/TESTING.md`, and `docs/ACCURACY.md`. The foundation
is implemented and locally checked. The CPU/bus milestone is still in progress:
standard checks pass, but independent reference mismatches remain. Provisional
parts of milestones 2–4 now provide genuine BIOS boot, rendered Emerald intro/
new-game dialogue, keyboard controls, Flash/RTC, saves, and an X11 desktop.
This user-requested testable vertical slice does not accept those milestones'
accuracy criteria or the first-release checkpoint. Audio synthesis/playback is
now implemented provisionally; real game-menu save/Continue cycles pass locally,
and the first battle succeeded in user testing. A controlled real-time desktop
soak passed; varied human gameplay/listening and hardware/reference validation
remain open. A desktop options menu, persistent key remapping, speed selection, and emulator save states are now agreed follow-up work; implementation has not started.

## Work policy

Normally implement one milestone at a time with tests and a short status report before moving on. The explicitly requested runnable preview may span provisional milestone work, with all unaccepted exit criteria kept open in the progress/accuracy ledgers. Record provisional behavior and failing tests explicitly. Avoid game-specific hacks that bypass hardware semantics. Revisit the plan if hardware evidence contradicts an architectural assumption. The plan was approved before implementation; subsequent work follows these milestones.
