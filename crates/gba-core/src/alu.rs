//! ARM7TDMI arithmetic and barrel shifter primitives.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArithmeticResult {
    pub(crate) value: u32,
    pub(crate) carry: bool,
    pub(crate) overflow: bool,
}

pub(crate) fn add_with_carry(left: u32, right: u32, carry: bool) -> ArithmeticResult {
    let wide = u64::from(left) + u64::from(right) + u64::from(carry);
    let signed = i64::from(left as i32) + i64::from(right as i32) + i64::from(carry);
    ArithmeticResult {
        value: wide as u32,
        carry: wide >> 32 != 0,
        overflow: signed != i64::from(wide as u32 as i32),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShiftKind {
    LogicalLeft,
    LogicalRight,
    ArithmeticRight,
    RotateRight,
}

impl ShiftKind {
    pub(crate) fn decode(bits: u32) -> Self {
        match bits & 3 {
            0 => Self::LogicalLeft,
            1 => Self::LogicalRight,
            2 => Self::ArithmeticRight,
            3 => Self::RotateRight,
            _ => unreachable!("masked shift kind"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShiftSource {
    Immediate,
    Register,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShiftResult {
    pub(crate) value: u32,
    pub(crate) carry: bool,
}

pub(crate) fn shift(
    value: u32,
    kind: ShiftKind,
    mut amount: u32,
    carry: bool,
    source: ShiftSource,
) -> ShiftResult {
    if amount == 0 {
        if source == ShiftSource::Register || kind == ShiftKind::LogicalLeft {
            return ShiftResult { value, carry };
        }
        if kind == ShiftKind::RotateRight {
            return ShiftResult {
                value: (u32::from(carry) << 31) | (value >> 1),
                carry: value & 1 != 0,
            };
        }
        amount = 32;
    }
    match kind {
        ShiftKind::LogicalLeft => ShiftResult {
            value: if amount < 32 { value << amount } else { 0 },
            carry: amount <= 32 && (value >> (32 - amount)) & 1 != 0,
        },
        ShiftKind::LogicalRight => ShiftResult {
            value: if amount < 32 { value >> amount } else { 0 },
            carry: amount <= 32 && (value >> (amount - 1)) & 1 != 0,
        },
        ShiftKind::ArithmeticRight => ShiftResult {
            value: ((value as i32) >> amount.min(31)) as u32,
            carry: (value >> (amount.min(32) - 1)) & 1 != 0,
        },
        ShiftKind::RotateRight => {
            let result = value.rotate_right(amount);
            ShiftResult {
                value: result,
                carry: result >> 31 != 0,
            }
        }
    }
}

/// Number of internal multiply cycles; unsigned long multiply cannot sign-extend.
pub(crate) fn multiply_cycles(multiplier: u32, signed: bool) -> u64 {
    for bytes in 1..4 {
        let upper = multiplier >> (bytes * 8);
        if upper == 0 || (signed && upper == (u32::MAX >> (bytes * 8))) {
            return bytes;
        }
    }
    4
}

#[cfg(test)]
mod tests {
    use super::{ShiftKind, ShiftSource};

    #[test]
    fn arithmetic_boundaries_and_borrow() {
        let result = super::add_with_carry(u32::MAX, 0, true);
        assert_eq!(
            (result.value, result.carry, result.overflow),
            (0, true, false)
        );
        let result = super::add_with_carry(0x7fff_ffff, 0, true);
        assert_eq!(
            (result.value, result.carry, result.overflow),
            (0x8000_0000, false, true)
        );
        let result = super::add_with_carry(0, !1, true);
        assert_eq!(
            (result.value, result.carry, result.overflow),
            (u32::MAX, false, false)
        );
        let result = super::add_with_carry(0x8000_0000, !1, true);
        assert_eq!(
            (result.value, result.carry, result.overflow),
            (0x7fff_ffff, true, true)
        );
    }

    #[test]
    fn arithmetic_matches_wide_integer_oracle() {
        let mut random = 0x1234_5678u32;
        for _ in 0..10000 {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            let left = random;
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            let right = random;
            for carry in [false, true] {
                let result = super::add_with_carry(left, right, carry);
                let unsigned = u64::from(left) + u64::from(right) + u64::from(carry);
                let signed = i64::from(left as i32) + i64::from(right as i32) + i64::from(carry);
                assert_eq!(u64::from(result.value), unsigned & 0xffff_ffff);
                assert_eq!(result.carry, unsigned > u64::from(u32::MAX));
                assert_eq!(
                    result.overflow,
                    signed < i64::from(i32::MIN) || signed > i64::from(i32::MAX)
                );
            }
        }
    }

    #[test]
    fn immediate_zero_has_special_meanings() {
        let lsr = super::shift(
            0x8000_0001,
            ShiftKind::LogicalRight,
            0,
            false,
            ShiftSource::Immediate,
        );
        assert_eq!((lsr.value, lsr.carry), (0, true));
        let asr = super::shift(
            0x8000_0001,
            ShiftKind::ArithmeticRight,
            0,
            false,
            ShiftSource::Immediate,
        );
        assert_eq!((asr.value, asr.carry), (u32::MAX, true));
        let rrx = super::shift(3, ShiftKind::RotateRight, 0, true, ShiftSource::Immediate);
        assert_eq!((rrx.value, rrx.carry), (0x8000_0001, true));
    }

    #[test]
    fn register_shift_boundaries() {
        let cases = [
            (ShiftKind::LogicalLeft, 31, 0x8000_0000, false),
            (ShiftKind::LogicalLeft, 32, 0, true),
            (ShiftKind::LogicalLeft, 33, 0, false),
            (ShiftKind::LogicalRight, 31, 1, false),
            (ShiftKind::LogicalRight, 32, 0, true),
            (ShiftKind::LogicalRight, 33, 0, false),
            (ShiftKind::ArithmeticRight, 255, u32::MAX, true),
            (ShiftKind::RotateRight, 32, 0x8000_0001, true),
            (ShiftKind::RotateRight, 33, 0xc000_0000, true),
        ];
        for (kind, amount, value, carry) in cases {
            let result = super::shift(0x8000_0001, kind, amount, false, ShiftSource::Register);
            assert_eq!(
                (result.value, result.carry),
                (value, carry),
                "{kind:?} {amount}"
            );
        }
        for kind in [
            ShiftKind::LogicalLeft,
            ShiftKind::LogicalRight,
            ShiftKind::ArithmeticRight,
            ShiftKind::RotateRight,
        ] {
            let result = super::shift(0x8000_0001, kind, 0, true, ShiftSource::Register);
            assert_eq!((result.value, result.carry), (0x8000_0001, true));
        }
    }

    #[test]
    fn multiply_termination_distinguishes_unsigned_long() {
        assert_eq!(super::multiply_cycles(0xff, true), 1);
        assert_eq!(super::multiply_cycles(0x100, true), 2);
        assert_eq!(super::multiply_cycles(u32::MAX, true), 1);
        assert_eq!(super::multiply_cycles(u32::MAX, false), 4);
    }
}
