//! ARM7TDMI multiply carry reconstruction.
//!
//! Adapted and reduced to carry calculation from zaydlang's hardware-researched
//! multiplication algorithm. This is an altered Rust implementation, not the
//! original C code. See LICENSES/multiplication-algorithm.txt and
//! docs/CPU_RESEARCH.md for attribution, derivation, and validation limits.

use crate::alu;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MultiplyKind {
    Short,
    LongSigned,
    LongUnsigned,
}

/// Reconstruct the barrel-shifter carry from the Booth/carry-save datapath.
/// Product computation remains native integer arithmetic in the interpreter.
pub(crate) fn carry(
    multiplicand: u32,
    multiplier: u32,
    accumulator: u64,
    kind: MultiplyKind,
) -> bool {
    const MASK_33: u64 = (1 << 33) - 1;
    const MASK_34: u64 = (1 << 34) - 1;
    let signed = kind != MultiplyKind::LongUnsigned;
    let multiplicand = if signed {
        (i64::from(multiplicand as i32) as u64) & MASK_34
    } else {
        u64::from(multiplicand)
    };
    let mut remaining = if signed {
        (i64::from(multiplier as i32) as u64) & MASK_34
    } else {
        u64::from(multiplier)
    };
    let iterations = alu::multiply_cycles(multiplier, signed) as u32;

    // The bootstrap consumes multiplier bit zero separately. The accumulator
    // supplies the other initial CSA input, including its high-bit stream.
    let initial_carry = if multiplier & 1 != 0 {
        !multiplicand
    } else {
        0
    };
    let mut sum = accumulator >> 1;
    let mut carries = initial_carry >> 1;
    let mut accumulator_high = accumulator >> 34;
    let mut carry_latch = u128::from(initial_carry & 1).rotate_right(1);

    for _ in 0..iterations {
        let mut finalized_carries = 0;
        for position in 0..4 {
            sum &= MASK_33;
            carries &= MASK_33;
            let digit = (remaining >> (position * 2)) & 7;
            let (recoded, correction) = match digit {
                0 | 7 => (0, 0),
                1 | 2 => (multiplicand, 0),
                3 => (multiplicand << 1, 0),
                4 => (!(multiplicand << 1), 1),
                5 | 6 => (!multiplicand, 1),
                _ => unreachable!("three-bit Booth digit"),
            };
            let addend = recoded & MASK_33;
            let next_sum = sum ^ carries ^ addend;
            let next_carries =
                (((sum & carries) | (sum & addend) | (carries & addend)) << 1) | correction;
            finalized_carries |= (next_carries & 3) << (position * 2);

            // Two low bits are final after each radix-4 stage. High bits encode
            // sign extension and the next two accumulator bits without ripple.
            let extension = (accumulator_high & 1)
                + u64::from(carries & (1 << 32) == 0)
                + u64::from(recoded & (1 << 33) == 0);
            sum = (next_sum >> 2) | (extension << 31);
            carries = (next_carries >> 2) | (u64::from(accumulator_high & 2 == 0) << 32);
            accumulator_high >>= 2;
        }
        let cycle_carries = (carries << 8) | finalized_carries;
        carry_latch |= u128::from(cycle_carries & 0xff);
        carry_latch = carry_latch.rotate_right(8);
        carries = cycle_carries >> 8;
        // Arithmetic right shift in the 33-bit multiplier datapath.
        remaining = ((remaining << 31) as i64 >> 39) as u64 & MASK_33;
    }

    carry_latch |= u128::from(carries);
    let correction = 31 - ((iterations * 8) % 32);
    carry_latch = carry_latch.rotate_right(correction);
    let flag_bit = if kind == MultiplyKind::Short && iterations == 4 {
        95
    } else {
        127
    };
    (carry_latch >> flag_bit) & 1 != 0
}

#[cfg(test)]
mod tests {
    use super::MultiplyKind;

    #[test]
    fn carry_matches_minimized_reference_cases() {
        // Extracted from the pinned MIT-licensed SingleStepTests vectors.
        // Case indices and the retained license are recorded in CPU_RESEARCH.md.
        use MultiplyKind::{LongSigned, LongUnsigned, Short};

        let cases = [
            (
                Short,
                0x327e_2d76,
                0x6cf6_988e,
                0x0000_0000_4e8c_e7bd,
                false,
            ),
            (Short, 0xb4d9_0966, 0xd86d_eb9e, 0, false),
            (Short, 0xb155_4c3f, 0xb155_4c3f, 0, true),
            (Short, 0x318d_2a7c, 0xff77_b63c, 0, true),
            (Short, 0x54cf_30f2, 0x82d5_ae42, 0x0000_0000_f620_cf1b, true),
            (LongUnsigned, 0x6a44_312c, 0xa157_5bb6, 0, false),
            (LongSigned, 0xc53d_5725, 0x9809_eac3, 0, false),
            (
                LongUnsigned,
                0x3425_1f1f,
                0x2f66_ced4,
                0x2ebd_3600_2f66_ced4,
                false,
            ),
            (
                LongSigned,
                0xdb63_a477,
                0x270a_8ab4,
                0x6036_4a80_270a_8ab4,
                false,
            ),
            (LongUnsigned, 0xead8_743f, 0x7999_d94b, 0, true),
            (LongSigned, 0xea08_bd72, 0x7a49_ae2d, 0, true),
            (
                LongUnsigned,
                0xffc8_0297,
                0xd231_ec41,
                0x5755_d61b_eeb4_ff92,
                true,
            ),
            (
                LongSigned,
                0xc1a5_f31c,
                0x9269_1738,
                0xdcb0_3395_f967_d403,
                true,
            ),
        ];
        for (kind, multiplicand, multiplier, accumulator, expected) in cases {
            assert_eq!(
                super::carry(multiplicand, multiplier, accumulator, kind),
                expected,
                "{kind:?}: {multiplicand:08x} * {multiplier:08x} + {accumulator:016x}"
            );
        }
    }

    #[test]
    fn short_full_width_carry_comes_from_multiplier_top_bits() {
        for multiplicand in [0, 1, 0x1234_5678, 0x8000_0000, u32::MAX] {
            for multiplier in [
                0x1234_5678,
                0x4000_0000,
                0x8000_0000,
                0xbfff_ffff,
                0xc123_4567,
            ] {
                for accumulator in [0, 0x1234_5678, u32::MAX] {
                    assert_eq!(
                        super::carry(
                            multiplicand,
                            multiplier,
                            u64::from(accumulator),
                            MultiplyKind::Short
                        ),
                        multiplier >> 30 == 2,
                        "four-iteration short carry property: {multiplicand:08x}, {multiplier:08x}, {accumulator:08x}"
                    );
                }
            }
        }
    }
}
