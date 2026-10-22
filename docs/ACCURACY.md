# Accuracy ledger

**Target: hardware accuracy. Current status: testable, incomplete desktop preview.**
Normal BIOS boot and Emerald's new-game introduction work. This is not a
cycle-perfect, full-game, or accepted playable-release claim. Audio is now
implemented provisionally; listening and physical sound fidelity remain open.

## References and findings

- Primary specification: [GBATEK](https://github.com/mgba-emu/gbatek/blob/gh-pages/gba.md).
- ARM7TDMI data sheet and hardware-researched multiply model: see
  [CPU_RESEARCH.md](CPU_RESEARCH.md) for exact sources and retained licenses.
- CPU comparison: SingleStepTests/ARM7TDMI, revision
  `e3097d88d428752736b949d94c094d5da50f0db6`, MIT.
- Style: https://google.github.io/styleguide/rustguide.html.

Do not import ARM9/ARMv5/DS behavior into ARMv4T. Record conflicts between
documentation, emulator models, and hardware instead of assuming agreement is
proof.

## Implemented with synthetic regression coverage

- ARM/Thumb arithmetic, logical operations, shifts, flags, condition codes,
  branches, BL/BX, loads/stores, signed transfers, swap and block transfers.
- Explicit pipeline, architectural PC alignment, banked registers, CPSR/SPSRs,
  SWI entry/return, legacy PSR tests, and Undefined entry/return.
- Hardware-research-based multiply carry reconstruction, native products,
  and internal-cycle-induced nonsequential code fetches.
- Little-endian BIOS, EWRAM, IWRAM, palette, VRAM, OAM, ROM windows and mirrors;
  selected byte-write, forced-alignment, and load-rotation quirks.
- Baseline N/S/I charging, WAITCNT durations, and 128 KiB ROM boundaries.
- Terminal device faults prevent resuming a partially failed instruction.
- Arbitrary SPSR bit storage without selecting an invalid current mode; a
  synthetic BIOS sequence covers the initialization case.
- LCD line/status/IRQ timing, IRQ entry, timers/cascade, DMA, keypad, HALT,
  software scanline rendering, banked Flash/SRAM, and GPIO RTC control reads.
- Host save-size checks, atomic replacement/backups, alias protection, and
  write-failure dirty-state retention; desktop integer-scale presentation and
  headless screenshots share the same core framebuffer.
- Core save-state snapshot round trips CPU/pipeline, mutable memory and device
  state, RTC and cartridge contents; it excludes immutable BIOS/ROM bytes and
  binds the versioned, checksummed file to a SHA-256 ROM identity. Files are
  capped at 4 MiB and decoded sequences are bounded. Corrupt, incompatible, and
  wrong-ROM inputs are rejected without changing the machine. The host session
  provides an adjacent `.state` quick slot, atomic writes, bounded reads, and
  restore-time atomic cartridge-save synchronization with `.sav.bak` retention.
  Desktop menu/keybind customization and state-result notices are implemented.
  The XDG game library scans ROMs, stores data by SHA-256 identity, and preserves
  existing central files during legacy-sidecar migration. Selectable speed is
  deferred; Tab fast-forward remains.

## Renderer optimization regressions

The PPU uses direct indexing for in-range halfword reads and keeps the previous
wrapping fallback. The compositor selects only the first two layers instead of
sorting every layer. Tests compare reads with the original modulo implementation
and front-layer selection with stable full sorting, including ties.

A fresh-save 4,000-frame scripted Emerald run remains byte-identical to the
previous PPM (1,123,584,003 clocks, PC 080008c8). The full strict CPU comparison
still has 2,124,072 matches and 125,928 failures. These are unchanged-behavior
regressions, not new evidence of physical rendering or timing accuracy.

## Explicit gaps

- Reserved/unsupported encoding policies and PC snapshot differences remain.
  All first-diagnostic patterns are catalogued in CPU_RESEARCH.md; this does
  not prove there are no additional differences in those cases.
- Multiply carry follows a published hardware-research model and matches the
  reference on documented register combinations. No physical measurements were
  performed here; early-termination coverage and timing still need expansion.
- User/System legacy PSR-test behavior, reserved mode writes, MSR T writes,
  invalid R15 combinations, and reserved Thumb fields are not fully established
  by physical tests. The core does not adopt reference behavior blindly.
- Undefined/absent-coprocessor vectoring exists. Exact exception-cycle ordering
  and sampling are not hardware-validated.
- N/S transitions are now asserted against vectors. Bus locking, contention,
  prefetch acceleration, and complete physical bus scheduling remain missing.
- BIOS protection/latch exist; instruction-versus-data latch behavior and reset
  latch contents need hardware tests.
- Open bus is a last-physical-read approximation, not full ARM/Thumb pipeline
  composition.
- Video contention and bitmap-mode VRAM byte restrictions are missing.
  Rendering is scanline-based, not a final dot-accurate model. Affine state,
  mid-line effects, sprite evaluation limits, and mode edge cases need tests.
- Device advancement and DMA arbitration occur at CPU-activity boundaries;
  precise startup, priority/preemption, bus ownership, and IRQ sampling need
  hardware validation. STOP is currently HALT; serial has no peer.
- PSG/Direct Sound mixing and Linux playback now exist. PWM rate conversion,
  wave RAM shift-register readback, frequency-write/trigger timing quirks,
  analog filtering/aliasing and FIFO edge behavior remain approximate.
  See AUDIO.md; synthetic/transport tests do not establish audible fidelity.
- Flash program/erase completes instantly; busy/toggle timing is missing.
  RTC reads/control work, but date/time writes, alarms, and persistence do not.
- Memory quirks have synthetic tests, not exhaustive physical-hardware tests.

## Strict reference baseline

All 45 groups × 50,000 cases are checked. Assertions cover User/banked registers,
CPSR/SPSRs, pipeline, transaction order/address/width/data, transaction N/S flags,
and final next-fetch N/S state. Code/data flags and LOCK are not checked.

**2,124,072 matches; 125,928 failures.** 27 groups match completely. The strict
command still fails; diagnostic classification does not suppress failures.

| Incomplete group | Matching / 50,000 |
| --- | ---: |
| arm_bx | 44,064 |
| arm_data_proc_immediate | 48,078 |
| arm_data_proc_immediate_shift | 48,034 |
| arm_data_proc_register_shift | 48,155 |
| arm_ldm_stm | 44,818 |
| arm_ldr_str_immediate_offset | 48,214 |
| arm_ldr_str_register_offset | 48,219 |
| arm_ldrh_strh | 48,221 |
| arm_ldrsb_ldrsh | 47,666 |
| arm_mrs | 48,420 |
| arm_msr_reg | 45,791 |
| arm_mul_mla | 45,472 |
| arm_mull_mlal | 44,782 |
| arm_swp | 45,898 |
| thumb_bx | 18,316 |
| thumb_ldm_stm | 49,950 |
| thumb_push_pop | 49,974 |
| thumb_undefined_bcc | 0 |

On the original 41-group scope, 75,928 cases fail versus the prior 88,312.
The larger total includes newly enabled groups; see CPU_RESEARCH.md rather than
interpreting these counts as a hardware-compatibility percentage.

The isolated mGBA suite baseline passes 235 cases (carry/shifter/multiply-long);
see UPSTREAM_CPU_TESTS.md for source provenance and limitations. No complete
hardware test ROM, mGBA-emulator differential run, or fresh physical capture
has been completed. BIOS boot reaches cartridge entry; 4,000 scripted Emerald
frames reach Professor Birch's introduction. Xvfb window pixels match the core
framebuffer, which verifies host transport rather than rendering accuracy.
First battle succeeded in user testing. Real game save/Continue/re-save/Continue
is locally verified. A controlled 32-minute real-time desktop soak and shorter
latest-build repeat passed, using Xvfb/null audio; see VALIDATION.md for build
and scope distinctions. Reference frames, speaker quality, broad interactive
coverage, and sustained performance on an end-user machine remain unverified.
See TEST_BUILD.md for the runnable preview and its explicit limits.
