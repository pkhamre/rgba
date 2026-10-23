//! User-selected host pacing speed; guest emulation timing is unchanged.

use std::time::Duration;

const FRAME_TIME: Duration = Duration::from_nanos(16_742_706);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Speed {
    #[default]
    One = 1,
    Two = 2,
    Four = 4,
    Eight = 8,
}

impl Speed {
    pub(crate) const ALL: [Self; 4] = [Self::One, Self::Two, Self::Four, Self::Eight];

    pub(crate) const fn multiplier(self) -> u32 {
        self as u32
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::One => "1X",
            Self::Two => "2X",
            Self::Four => "4X",
            Self::Eight => "8X",
        }
    }

    pub(crate) fn interval(self) -> Duration {
        FRAME_TIME.div_f64(self.multiplier() as f64)
    }

    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "1" => Ok(Self::One),
            "2" => Ok(Self::Two),
            "4" => Ok(Self::Four),
            "8" => Ok(Self::Eight),
            _ => Err(format!(
                "unsupported speed {value:?}; expected 1, 2, 4, or 8"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Speed;
    use std::time::Duration;

    #[test]
    fn supports_only_the_four_configured_multipliers() {
        assert_eq!(Speed::ALL.map(Speed::multiplier), [1, 2, 4, 8]);
        assert_eq!(Speed::One.interval(), Duration::from_nanos(16_742_706));
        assert_eq!(Speed::Two.interval(), Duration::from_nanos(8_371_353));
        assert_eq!(Speed::Four.interval(), Duration::from_nanos(4_185_676));
        assert_eq!(Speed::Eight.interval(), Duration::from_nanos(2_092_838));
        for speed in Speed::ALL {
            assert_eq!(Speed::parse(&speed.multiplier().to_string()), Ok(speed));
        }
        assert!(Speed::parse("3").is_err());
        assert!(Speed::parse("0").is_err());
    }
}
