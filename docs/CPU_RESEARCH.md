# CPU validation research

## Scope and evidence

This work changes CPU execution and test tooling only. No peripherals, desktop
frontend, BIOS bypass, or game-specific workaround was added. The supplied BIOS
still stops at unimplemented POSTFLG I/O.

Evidence consulted:

1. [GBATEK](https://github.com/mgba-emu/gbatek/blob/gh-pages/gba.md), specifically
   ARM data processing, alignment, Thumb high-register/conditional-branch
   encodings, instruction cycle times, and GamePak Prefetch.
2. [ARM7TDMI Data Sheet, ARM DDI 0029E, August 1995](https://www.dwedit.org/files/ARM7TDMI.pdf),
   sections 4.5.4–4.5.6, 4.6, and 4.11.4–4.11.6.
3. [Solving the Mystery of ARM7TDMI Multiply Carry Flag](https://bmchtech.github.io/post/multiply/),
   a hardware-research account of the Booth/carry-save multiplier datapath.
4. [zaydlang/multiplication-algorithm](https://github.com/zaydlang/multiplication-algorithm/tree/29cef09501154a8d6ed63c52cde9c3e8be1b5034),
   revision `29cef09501154a8d6ed63c52cde9c3e8be1b5034`; `impl.h` blob
   `5dac436aed7eb937c8da48191ed38b94cff82df4`.
5. The pinned SingleStepTests vectors described in TESTING.md.

The multiply model is adapted from item 4, narrowed to flag calculation, and
rewritten in safe Rust with local state and u128 rotations. It is plainly marked
as altered; the original zlib license is retained in
`LICENSES/multiplication-algorithm.txt`. It is not a borrowed emulator backend.
The minimized vector constants retain upstream attribution and MIT notice in
`LICENSES/singlestep-tests.txt`.

No physical GBA measurements were collected in this workspace. Reference
agreement and a published hardware-research model are evidence, not a new
hardware certification.

## Confirmed corrections

### Multiply carry

C is not reliably zero after an ARM7 multiply. The carry model tracks radix-4
Booth digits, 33-bit carry-save stages, accumulator sign-extension inputs,
early termination, and the final carry-latch rotation. Products still use native
wrapping arithmetic; the datapath simulation runs only when flags are requested.

Thumb's incoming destination is the internal multiplier; ARM uses Rs.
Full-width short multiply has the property C = (multiplier[31:30] == 10b).
Long multiply and early termination require the partial-carry calculation.
N/Z use the product; V is preserved.

The previous implementation failed Thumb ALU case 165 with the right product
but the wrong C flag. A minimized asset-free regression now checks the product,
carry, and preservation of V. All 50,000 Thumb ALU cases now match, including
N/S access flags.

Carry-only regression constants come from these reference cases:

- arm_mul_mla: 790, 8896, 10018, 12254, 12687.
- arm_mull_mlal: 43, 1977, 2026, 15369, 20828, 24555, 31727, 33698.

They include short, signed/unsigned long, accumulating/non-accumulating,
set/clear carry, and three-/four-iteration cases. Broader real-hardware coverage
of one-/two-iteration carry and exact multiplier timing is still needed.

### Legacy PSR test forms

The ARM data sheet explicitly describes TEQP restoring the current SPSR in
privileged modes and doing nothing in User mode. The old implementation treated
it as ordinary TEQ flags. Regressions were observed failing before the correction
and passing afterward.

The interpreter now applies status restoration to legacy test encodings with
Rd=15, without writing the PC. User/System have no SPSR, so they leave status
unchanged. The wider family and System-mode edge behavior still need physical
verification; the reference model computes ordinary flags without an SPSR.
Those disagreements remain failures, not expected-pass annotations.

### Internal cycles and the next code access

Checking only registers/pipeline missed an instruction-fetch timing defect.
Register shifts and multiplies consumed I cycles but incorrectly left the next
code access Sequential. GBATEK's GamePak Prefetch section documents the
nonsequential fetch effect when prefetch is disabled.

The harness now seeds the initial code-access flag from each vector and checks
each transaction's N/S flag plus the final next-fetch state. Enabling those
assertions initially exposed 78,197 additional failing cases; correcting the
internal-cycle transition removed those additional failures.

Synthetic ROM tests verify the current baseline clock model charges an 8-clock
default word N fetch after these instructions, then resumes 6-clock S fetches
after a NOP. This does not verify contention or prefetch-buffer behavior.

### Undefined exceptions

Absent coprocessors and undefined/later-architecture instructions now enter
Undefined mode at BIOS vector 0x04. Entry saves CPSR in SPSR_und, saves ARM PC+4
or Thumb PC+2 in LR_und, clears Thumb, and masks IRQ. Unknown instructions no
longer cause a host-side unsupported-opcode error on these decoder paths.

Regressions cover entry, saved status, banked-register restoration, Thumb return,
and condition-failed instructions. All 150,000 cases in the three newly enabled
absent-coprocessor groups match. Exact exception cycle ordering remains a
hardware-validation task.

## Strict comparison results

The full harness now runs all 45 upstream groups: **2,121,019 matches and
128,981 failures out of 2,250,000 cases**. 27 groups match completely. Nothing is
silently excluded or normalized to make the total pass.

On the original 41-group scope, failures fell from 88,312 to 78,981 (9,331 fewer)
despite the stronger N/S assertions. The expanded total adds 50,000 deliberate
Thumb reserved-condition disagreements and 150,000 passing coprocessor cases.

The first diagnostic from every current failing case fits one of these patterns:

| First-diagnostic pattern | Cases | Evidence / current policy |
| --- | ---: | --- |
| Low PC snapshot bits differ | 22,338 | GBATEK says architectural PC is forcibly aligned; reference retains low bits. |
| Legacy PSR test without SPSR | 3,478 | User/System policy differs; TEQP User no-op is documented, broader behavior needs testing. |
| User-bank transfer with writeback | 466 | ARM data sheet 4.11.4 says not to use writeback in this form. |
| PC base with writeback | 4,907 | Unsupported/document-restricted PC addressing combinations. |
| Forbidden R15 operand | 15,428 | MRS, multiply, and SWP operands outside their documented register ranges. |
| Reserved mode write diagnostic | 6,742 | Current core rejects invalid processor modes instead of inventing register banks. |
| MSR T-bit write | 520 | MSR must not be used to switch instruction sets; core preserves T. |
| Reserved Thumb BX field | 25,102 | Bit 7 is reserved on ARMv4T; core vectors, reference performs BX. |
| Reserved Thumb condition 0xe | 50,000 | GBATEK marks it Undefined; core vectors, reference treats it as an always-taken branch. |

This is **first-diagnostic classification, not proof that these cases contain no
other differences**, and not a waiver for full hardware fidelity. The strict
test still fails. Hardware-derived tests are needed to establish deterministic
behavior for reserved/unsupported encodings before changing those policies.

Reproduce the report with the commands in TESTING.md. Current local artifacts:
`artifacts/cpu-validation-fixed-access.txt`, its TSV counterpart, and
`artifacts/cpu-triage-fixed-access.json`.

## Remaining validation before accepting the CPU/bus milestone

- Hardware-backed evidence for the policy differences above, especially PC
  alignment, MSR T writes, and reserved Thumb decoding.
- Complete transaction auditing beyond the first failure, including bus locking
  and code/data flags (N/S is now checked).
- Exact internal-cycle placement and exception sampling, BIOS read latch,
  open-bus composition, and GamePak prefetch.
- Hardware CPU/bus test ROMs with an explicit headless completion protocol.
- Broader early-termination multiplier tests.

The CPU/bus milestone remains open; reference-emulator matching alone is not
enough to claim hardware accuracy.
