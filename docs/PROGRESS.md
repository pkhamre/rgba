# Implementation progress

## Milestone 0 — foundation

Implemented and locally checked: pinned stable-Rust Cargo workspace and lockfile;
core/headless separation; Google Rust style policy; rustfmt/Clippy/public-doc and
safe-Rust lints; asset validation; bounded traces; CI configuration; synthetic
tests; and pinned/licensed reference-vector fetching with integrity verification.

GitHub Actions is configured, not executed here. The local isolated musl setup
works without a C compiler. No graphical session is available.

## Milestone 1 — CPU and bus: still open

Implemented ARM/Thumb interpreter, pipeline/PC semantics, banked registers,
status/exception entry and return, memory map/access quirks, and baseline N/S/I
timing. The independent reference harness checks all 45 groups, now including
transaction N/S access flags.

Latest completed validation work:

- Replaced constant-zero multiply carry with an attributed, licensed adaptation
  of a hardware-researched Booth/carry-save model.
- Fixed legacy PSR-test status restoration, with failing-before/passing-after
  regressions and primary ARM data-sheet evidence.
- Fixed internal-cycle instructions leaving the next code fetch Sequential.
  ROM clock tests and reference N/S assertions cover the correction.
- Added Undefined and absent-coprocessor vectoring and Thumb exception return.
  All three coprocessor groups pass.
- Added single-case replay, complete original-state inspection, failure TSVs,
  and asset-free tests for classification/inspection tooling.
- Categorized every current first diagnostic without suppressing any failures.

Standard checks: 84 Rust unit/integration tests + one doctest + six Python tests
pass. Full strict comparison: 2,121,019 matches / 128,981 failures; 27 of 45
groups match completely. On the original scope there are 9,331 fewer failures.
See CPU_RESEARCH.md for sources, exact categories, and limitations.

The CPU/bus milestone is not accepted as hardware-accurate. Classified first
diagnostics do not prove complete correctness or resolve physical reserved-opcode
behavior. The supplied BIOS still stops after four instructions at POSTFLG.

## Next work

1. Acquire/verify hardware-derived CPU and bus tests, with licensing and a
   headless result protocol. Investigate documented/reference disagreements;
   do not change aligned PC behavior just to match internal reference snapshots.
2. Audit full transactions beyond first failures: code/data/lock signals,
   internal-cycle placement, BIOS latch, open bus, and GamePak prefetch.
3. Expand early-termination multiply coverage and exception timing validation.
4. Then milestone 2: deterministic event ordering, IRQ, timers, DMA, system I/O
   including POSTFLG, display timing, and genuine BIOS boot.
5. Continue the agreed graphics/desktop, Flash/RTC/saves, audio, and gameplay plan.

No peripherals, BIOS bypass, or game-specific shortcut were added during this CPU
validation work. Emerald is not playable; performance and interactive stability
remain unverified.
