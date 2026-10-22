# Accuracy ledger

**Target: hardware accuracy. Current status: incomplete CPU/bus development.**
Nothing here is a cycle-perfect or playable-Emerald claim.

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
- Only static unpressed KEYINPUT and baseline WAITCNT I/O are implemented.
  Most I/O faults explicitly. ROM GPIO is not intercepted.
- No scheduler, IRQ delivery, timers, DMA, HALT/STOP, PPU, APU, Flash/RTC,
  persistent saves, or desktop frontend.
- Memory quirks have synthetic tests, not exhaustive physical-hardware tests.

## Strict reference baseline

All 45 groups × 50,000 cases are checked. Assertions cover User/banked registers,
CPSR/SPSRs, pipeline, transaction order/address/width/data, transaction N/S flags,
and final next-fetch N/S state. Code/data flags and LOCK are not checked.

**2,121,019 matches; 128,981 failures.** 27 groups match completely. The strict
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
| arm_msr_reg | 42,738 |
| arm_mul_mla | 45,472 |
| arm_mull_mlal | 44,782 |
| arm_swp | 45,898 |
| thumb_bx | 18,316 |
| thumb_ldm_stm | 49,950 |
| thumb_push_pop | 49,974 |
| thumb_undefined_bcc | 0 |

On the original 41-group scope, 78,981 cases fail versus the prior 88,312.
The larger total includes newly enabled groups; see CPU_RESEARCH.md rather than
interpreting these counts as a hardware-compatibility percentage.

No hardware test ROM or mGBA differential run has been completed. BIOS execution
has not reached the cartridge entry point. Gameplay, rendering, audio, and
sustained real-time performance are unverified.
