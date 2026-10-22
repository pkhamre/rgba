//! Producer timing diagnostics, not measurements of audio-device underruns.

use std::time::{Duration, Instant};

/// Observed PCM submission timing while an unmuted player is selected.
#[derive(Clone, Copy, Debug, Default)]
pub struct AudioTiming {
    /// Valid frames submitted, whether or not a full host queue accepted them.
    pub submitted_frames: u64,
    /// Longest interval between nonempty, unmuted submissions.
    pub max_submission_gap: Duration,
    /// Submission intervals longer than 40 ms (not a device XRUN count).
    pub gaps_over_40_ms: u64,
    timed_frames: u64,
    span: Duration,
}

impl AudioTiming {
    /// Observed producer rate over consecutive active submissions, excluding
    /// pause/fast-forward and restart intervals. Not the device's sample clock.
    #[must_use]
    pub fn submission_rate_hz(&self) -> Option<f64> {
        (!self.span.is_zero()).then(|| self.timed_frames as f64 / self.span.as_secs_f64())
    }
}

#[derive(Default)]
pub(crate) struct SubmissionTiming {
    last: Option<Instant>,
    snapshot: AudioTiming,
}

impl SubmissionTiming {
    pub(crate) fn record(&mut self, now: Instant, frames: usize) {
        self.snapshot.submitted_frames += frames as u64;
        if let Some(last) = self.last {
            let gap = now.saturating_duration_since(last);
            self.snapshot.timed_frames += frames as u64;
            self.snapshot.span += gap;
            self.snapshot.max_submission_gap = self.snapshot.max_submission_gap.max(gap);
            self.snapshot.gaps_over_40_ms += u64::from(gap > Duration::from_millis(40));
        }
        self.last = Some(now);
    }

    pub(crate) fn reset_gap(&mut self) {
        self.last = None;
    }
    pub(crate) fn snapshot(&self) -> AudioTiming {
        self.snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::SubmissionTiming;
    use std::time::{Duration, Instant};

    #[test]
    fn measures_normal_stalls_but_excludes_pause_and_backend_restart() {
        let mut timing = SubmissionTiming::default();
        let start = Instant::now();
        timing.record(start, 549);
        timing.record(start + Duration::from_millis(17), 548);
        timing.record(start + Duration::from_millis(77), 549);
        timing.reset_gap();
        timing.record(start + Duration::from_secs(300), 548);
        let stats = timing.snapshot();
        assert_eq!(stats.submitted_frames, 2194);
        assert_eq!(stats.max_submission_gap, Duration::from_millis(60));
        assert_eq!(stats.gaps_over_40_ms, 1);
        assert!((stats.submission_rate_hz().expect("timed input") - 1097.0 / 0.077).abs() < 0.001);
        assert!(
            SubmissionTiming::default()
                .snapshot()
                .submission_rate_hz()
                .is_none()
        );
    }
}
