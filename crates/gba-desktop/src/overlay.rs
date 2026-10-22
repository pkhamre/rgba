//! Host-only overlays; never change the emulated framebuffer.

pub(crate) fn notice(pixels: &[u32], message: &str, success: bool) -> Vec<u32> {
    let mut output = pixels.to_vec();
    let width = message.chars().count() * 6 + 16;
    let left = (240 - width) / 2;
    let (top, bottom) = (8, 29);
    let border = if success { 0x70e090 } else { 0xff7070 };
    for y in top..=bottom {
        for x in left..left + width {
            output[y * 240 + x] = if y == top || y == bottom || x == left || x == left + width - 1 {
                border
            } else {
                0x101820
            };
        }
    }
    crate::menu::draw_text(&mut output, left + 8, top + 7, message, border);
    output
}

pub(crate) fn paused(pixels: &[u32]) -> Vec<u32> {
    let mut output = pixels.to_vec();
    for y in 68..91 {
        for x in 92..148 {
            output[y * 240 + x] = if y == 68 || y == 90 || x == 92 || x == 147 {
                0xffffff
            } else {
                0x101820
            };
        }
    }
    // Five-bit rows of the six letters in PAUSED.
    let letters = [
        [30, 17, 17, 30, 16, 16, 16],
        [14, 17, 17, 31, 17, 17, 17],
        [17, 17, 17, 17, 17, 17, 14],
        [15, 16, 16, 14, 1, 1, 30],
        [31, 16, 16, 30, 16, 16, 31],
        [30, 17, 17, 17, 17, 17, 30],
    ];
    for (letter, rows) in letters.iter().enumerate() {
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    output[(76 + row) * 240 + 102 + letter * 6 + column] = 0xffffff;
                }
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    #[test]
    fn state_notice_is_host_only_and_uses_success_and_failure_colors() {
        let original = vec![0x123456; 240 * 160];
        let success = super::notice(&original, "STATE SAVED", true);
        let failure = super::notice(&original, "LOAD FAILED", false);
        assert_eq!(original[8 * 240 + 79], 0x123456);
        assert_eq!(success[8 * 240 + 79], 0x70e090);
        assert_eq!(failure[8 * 240 + 79], 0xff7070);
        assert_eq!(success[0], original[0]);
    }

    #[test]
    fn paused_label_does_not_modify_guest_pixels() {
        let original = vec![0x123456; 240 * 160];
        let visible = super::paused(&original);
        assert_eq!(original[68 * 240 + 92], 0x123456);
        assert_eq!(visible[68 * 240 + 92], 0xffffff);
        assert_eq!(visible[76 * 240 + 102], 0xffffff);
        assert_eq!(visible[70 * 240 + 100], 0x101820);
        assert_eq!(visible[0], original[0]);
    }
}
