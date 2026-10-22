//! Wall-clock pacing only. Never changes guest clocks or skips guest frames.

use std::time::Duration;

#[derive(Default)]
pub(crate) struct FramePacing {
    audio_lead: Duration,
}

impl FramePacing {
    pub(crate) fn wait(&mut self, until_deadline: Duration, needs_audio: Option<bool>) -> Duration {
        // Recover a small real PCM reserve when the player has little pending
        // input. Slew the offset rather than alternate bursts and long sleeps.
        // Even an unpaced/null sink cannot drive unlimited emulation speed.
        self.audio_lead = match needs_audio {
            Some(true) => {
                (self.audio_lead + Duration::from_millis(2)).min(Duration::from_millis(80))
            }
            Some(false) => self.audio_lead.saturating_sub(Duration::from_micros(250)),
            None => Duration::ZERO,
        };
        until_deadline.saturating_sub(self.audio_lead)
    }
}

#[cfg(test)]
mod tests {
    use super::FramePacing;
    use std::time::Duration;

    #[test]
    fn audio_catchup_is_bounded_and_slewed_and_off_uses_normal_pacing() {
        let mut pacing = FramePacing::default();
        assert_eq!(
            pacing.wait(Duration::from_millis(17), Some(true)),
            Duration::from_millis(15)
        );
        for _ in 0..100 {
            pacing.wait(Duration::from_millis(100), Some(true));
        }
        assert_eq!(pacing.audio_lead, Duration::from_millis(80));
        assert_eq!(
            pacing.wait(Duration::from_millis(100), Some(false)),
            Duration::from_micros(20250)
        );
        assert_eq!(
            pacing.wait(Duration::from_millis(17), None),
            Duration::from_millis(17)
        );
        assert!(pacing.audio_lead.is_zero());
    }

    #[test]
    fn unpaced_consumer_cannot_cause_runaway_fast_forward() {
        let mut pacing = FramePacing::default();
        let mut wall = Duration::ZERO;
        let mut deadline = Duration::ZERO;
        for _ in 0..1000 {
            wall += Duration::from_micros(500);
            deadline += Duration::from_millis(16);
            wall += pacing.wait(deadline.saturating_sub(wall), Some(true));
            assert!(deadline.saturating_sub(wall) <= Duration::from_millis(80));
        }
        assert!(wall >= Duration::from_millis(15920));
    }
}
