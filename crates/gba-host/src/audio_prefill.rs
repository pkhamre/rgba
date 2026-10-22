//! Bounded startup/resume reservoir. Capacity alone does not create audio lead.

/// One eighth of a second of real PCM before starting delivery.
pub(crate) const PREFILL_FRAMES: usize = 4096;

#[derive(Default)]
pub(crate) struct AudioPrefill {
    pending: Vec<[i16; 2]>,
    ready: bool,
}

impl AudioPrefill {
    pub(crate) fn reset(&mut self) -> usize {
        let discarded = self.pending.len();
        self.pending.clear();
        self.ready = false;
        discarded
    }

    pub(crate) fn push(&mut self, samples: Vec<[i16; 2]>) -> Option<Vec<[i16; 2]>> {
        if self.ready {
            return Some(samples);
        }
        self.pending.extend(samples);
        if self.pending.len() < PREFILL_FRAMES {
            return None;
        }
        self.ready = true;
        Some(std::mem::take(&mut self.pending))
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioPrefill, PREFILL_FRAMES};

    #[test]
    fn holds_real_samples_until_reserve_exists_then_preserves_order() {
        let mut reservoir = AudioPrefill::default();
        let mut expected = Vec::new();
        for index in 0..7 {
            let packet = vec![[index, -index]; 549];
            expected.extend_from_slice(&packet);
            assert!(reservoir.push(packet).is_none());
        }
        let packet = vec![[7, -7]; 549];
        expected.extend_from_slice(&packet);
        let ready = reservoir.push(packet).expect("prefill complete");
        assert_eq!(ready, expected);
        assert!(ready.len() >= PREFILL_FRAMES);
        assert_eq!(reservoir.push(vec![[8, -8]; 10]), Some(vec![[8, -8]; 10]));
    }

    #[test]
    fn reserve_covers_a_modelled_62_ms_producer_stall_without_silence_padding() {
        fn missed_deadlines(prefill: bool) -> usize {
            let mut reservoir = AudioPrefill::default();
            let mut queued = 0;
            let mut next_packet = 0;
            let mut started = false;
            let mut missed = 0;
            // Units are sample frames. Producer: 512 frames/15.625 ms.
            // Consumer: 256/7.8125 ms. Delay production for 2048 frames/62.5 ms,
            // then deliver overdue packets as the wall-clock frame loop catches up.
            for now in (0..32768).step_by(256) {
                if !(16384..18432).contains(&now) {
                    while next_packet <= now {
                        let packet = vec![[100, -100]; 512];
                        let ready = if prefill {
                            reservoir.push(packet)
                        } else {
                            Some(packet)
                        };
                        if let Some(samples) = ready {
                            queued += samples.len();
                            started = true;
                        }
                        next_packet += 512;
                    }
                }
                if started {
                    if queued < 256 {
                        missed += 1;
                        queued = 0;
                    } else {
                        queued -= 256;
                    }
                }
            }
            missed
        }
        assert!(
            missed_deadlines(false) > 0,
            "unprimed delivery misses consumer deadlines"
        );
        assert_eq!(
            missed_deadlines(true),
            0,
            "real buffered PCM bridges the short stall"
        );
    }

    #[test]
    fn reset_discards_previous_epoch_and_requires_a_new_reserve() {
        let mut reservoir = AudioPrefill::default();
        assert!(reservoir.push(vec![[1, 1]; 2048]).is_none());
        assert!(reservoir.push(vec![[1, 1]; 2048]).is_some());
        reservoir.reset();
        assert!(reservoir.push(vec![[2, 2]; 2048]).is_none());
        assert_eq!(reservoir.push(vec![[2, 2]; 2048]), Some(vec![[2, 2]; 4096]));
    }

    #[test]
    fn bounded_packet_sizes_bound_reservoir_overshoot() {
        let mut reservoir = AudioPrefill::default();
        for _ in 0..3 {
            assert!(reservoir.push(vec![[0, 0]; 1365]).is_none());
        }
        let ready = reservoir.push(vec![[0, 0]; 2048]).expect("reserve");
        assert_eq!(ready.len(), PREFILL_FRAMES - 1 + 2048);
        assert!(reservoir.pending.is_empty());
    }
}
