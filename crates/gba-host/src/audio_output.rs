//! Bounded Linux audio transport through installed PipeWire/PulseAudio/ALSA tools.
//! No host callbacks or device timing can advance the emulated clock.

use crate::audio_prefill::{AudioPrefill, PREFILL_FRAMES};
use crate::audio_timing::{AudioTiming, SubmissionTiming};
use crate::stream_format::StreamFormat;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread;
use std::thread::JoinHandle;

struct Candidate {
    name: &'static str,
    command: Command,
    format: StreamFormat,
}

/// Bounded asynchronous transport with ordered, one-attempt-per-player fallback.
/// A live process is not proof of an audible/routed stream or freedom from XRUNs.
pub struct AudioOutput {
    player: Option<Player>,
    remaining: VecDeque<Candidate>,
    muted: bool,
    speed_multiplier: u32,
    reducer: SpeedReducer,
    dropped: u64,
    timing: SubmissionTiming,
}

impl AudioOutput {
    /// Opens auto, pipewire, pulse, or alsa without invoking a shell.
    /// Auto advances to the next player on both early and later process/I/O failure.
    /// Explicit selections do not silently change backend.
    pub fn open(backend: &str) -> io::Result<Self> {
        let names: &[&str] = match backend {
            "auto" => &["pipewire", "pulse", "alsa"],
            "pipewire" => &["pipewire"],
            "pulse" => &["pulse"],
            "alsa" => &["alsa"],
            _ => {
                return Err(io::Error::other(
                    "backend must be auto, pipewire, pulse, or alsa; skip opening output for off",
                ));
            }
        };
        let candidates = names
            .iter()
            .map(|name| {
                let (program, arguments, format): (&str, &[&str], StreamFormat) = match *name {
                    "pipewire" => (
                        "pw-cat",
                        &["--playback", "--latency=80ms", "-"],
                        StreamFormat::AuBigEndian,
                    ),
                    "pulse" => (
                        "pacat",
                        &[
                            "--playback",
                            "--raw",
                            "--rate=32768",
                            "--channels=2",
                            "--format=s16le",
                            "--latency-msec=80",
                            "--process-time-msec=20",
                        ],
                        StreamFormat::RawLittleEndian,
                    ),
                    _ => (
                        "aplay",
                        &[
                            "--quiet",
                            "--file-type=raw",
                            "--format=S16_LE",
                            "--channels=2",
                            "--rate=32768",
                            "--buffer-time=80000",
                        ],
                        StreamFormat::RawLittleEndian,
                    ),
                };
                let mut command = Command::new(program);
                command.args(arguments);
                Candidate {
                    name: program,
                    command,
                    format,
                }
            })
            .collect();
        Self::from_candidates(candidates)
    }

    fn from_candidates(remaining: VecDeque<Candidate>) -> io::Result<Self> {
        let mut output = Self {
            player: None,
            remaining,
            muted: false,
            speed_multiplier: 1,
            reducer: SpeedReducer::default(),
            dropped: 0,
            timing: SubmissionTiming::default(),
        };
        output.start_next()?;
        Ok(output)
    }

    fn start_next(&mut self) -> io::Result<()> {
        let mut last = io::Error::other("no remaining audio players");
        while let Some(candidate) = self.remaining.pop_front() {
            let name = candidate.name;
            match Player::spawn(candidate) {
                Ok(mut player) => {
                    // This only catches immediate failures, not playback readiness.
                    thread::sleep(std::time::Duration::from_millis(40));
                    if let Some(reason) = player.failure() {
                        eprintln!("Audio: {name} failed during startup: {reason}");
                        last = io::Error::other(reason);
                        continue;
                    }
                    player.set_muted(self.muted);
                    eprintln!(
                        "Audio: {name} started, 32768 Hz stereo (80 ms requested latency; not playback confirmation)"
                    );
                    eprintln!(
                        "Audio prefill: {} frames (125 ms of real PCM)",
                        PREFILL_FRAMES
                    );
                    self.timing.reset_gap();
                    self.player = Some(player);
                    return Ok(());
                }
                Err(error) => {
                    eprintln!("Audio: could not start {name}: {error}");
                    last = error;
                }
            }
        }
        Err(io::Error::other(format!(
            "audio unavailable: {last}; install pw-cat/pacat/aplay or use --audio off"
        )))
    }

    /// Checks child exit and writer errors, including while paused/no PCM is sent.
    /// A backend restart may cause a short gap and bounded process-start overhead.
    pub fn poll(&mut self) {
        let reason = self.player.as_mut().and_then(Player::failure);
        if let Some(reason) = reason {
            if let Some(player) = self.player.take() {
                eprintln!(
                    "Audio: {} stopped: {reason}; checking remaining players",
                    player.name
                );
                self.dropped += player.dropped;
                drop(player);
            }
            if let Err(error) = self.start_next() {
                eprintln!("{error}; continuing silently");
            }
        }
    }

    /// Most recently checked live player process, not a device-readiness signal.
    #[must_use]
    pub fn player_name(&self) -> Option<&str> {
        self.player.as_ref().map(|player| player.name)
    }

    /// Mutes new packets during pause or unlimited Tab fast-forward.
    /// Device buffers still drain.
    pub fn set_muted(&mut self, muted: bool) {
        if self.muted != muted {
            self.timing.reset_gap();
            self.reducer.reset();
        }
        self.muted = muted;
        if let Some(player) = &self.player {
            player.set_muted(muted);
        }
    }

    /// Compresses submitted PCM time by this factor, raising pitch with speed.
    /// Supported factors are 1, 2, 4, 8, and 16. Guest audio synthesis is unchanged.
    pub fn set_speed(&mut self, multiplier: u32) -> io::Result<()> {
        if !matches!(multiplier, 1 | 2 | 4 | 8 | 16) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "audio speed must be 1, 2, 4, 8, or 16",
            ));
        }
        if multiplier == self.speed_multiplier {
            return Ok(());
        }
        self.speed_multiplier = multiplier;
        self.reducer.set_factor(multiplier as usize);
        self.timing.reset_gap();
        if let Some(player) = &self.player {
            player.epoch.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Drops stale pre-load PCM from the bounded writer queue and starts a fresh
    /// prefill epoch after a machine state restore. OS/device buffers may still drain.
    pub fn reset_after_state_load(&mut self) {
        if let Some(player) = &self.player {
            player.epoch.fetch_add(1, Ordering::Relaxed);
        }
        self.reducer.reset();
        self.timing.reset_gap();
    }

    /// Sends PCM without waiting for playback; even an empty packet polls health.
    /// At most eight packets of up to 2,048 frames are queued.
    pub fn submit(&mut self, samples: Vec<[i16; 2]>) {
        self.poll();
        let samples = self.reducer.reduce(samples);
        if self.player.is_some() && !self.muted && !samples.is_empty() && samples.len() <= 2048 {
            self.timing.record(std::time::Instant::now(), samples.len());
        }
        if let Some(player) = &mut self.player {
            player.submit(samples);
        } else if !self.muted {
            self.dropped += samples.len() as u64;
        }
    }

    /// Some(true) asks the host pacer for bounded catch-up when little accepted
    /// PCM remains in the writer. Some(false) means enough is pending; None means
    /// muted/unavailable output. Does not include device/OS buffers or prove sound.
    #[must_use]
    pub fn needs_samples(&self) -> Option<bool> {
        self.player
            .as_ref()
            .filter(|player| !self.muted && !player.failed.load(Ordering::Relaxed))
            .map(|player| player.pending.load(Ordering::Relaxed) < PREFILL_FRAMES as u64)
    }

    /// Submission-gap measurements; unrelated to device XRUN reporting.
    #[must_use]
    pub fn timing(&self) -> AudioTiming {
        self.timing.snapshot()
    }

    /// Frames rejected by host queues/unavailable output. Not a device-underrun
    /// counter or a delivery receipt: a dying player can lose in-flight samples.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped + self.player.as_ref().map_or(0, |player| player.dropped)
    }
}

#[derive(Default)]
struct SpeedReducer {
    factor: usize,
    consumed: usize,
    sums: [i64; 2],
}

impl SpeedReducer {
    fn set_factor(&mut self, factor: usize) {
        self.factor = factor;
        self.reset();
    }

    fn reset(&mut self) {
        self.consumed = 0;
        self.sums = [0; 2];
    }

    fn reduce(&mut self, samples: Vec<[i16; 2]>) -> Vec<[i16; 2]> {
        let factor = self.factor.max(1);
        if factor == 1 {
            return samples;
        }
        let mut reduced = Vec::with_capacity((samples.len() + self.consumed) / factor);
        for frame in samples {
            self.sums[0] += i64::from(frame[0]);
            self.sums[1] += i64::from(frame[1]);
            self.consumed += 1;
            if self.consumed == factor {
                reduced.push([
                    (self.sums[0] / factor as i64) as i16,
                    (self.sums[1] / factor as i64) as i16,
                ]);
                self.reset();
            }
        }
        reduced
    }
}

struct Packet {
    epoch: u64,
    samples: Vec<[i16; 2]>,
}

struct Player {
    name: &'static str,
    child: Child,
    sender: Option<SyncSender<Packet>>,
    worker: Option<JoinHandle<()>>,
    muted: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    pending: Arc<AtomicU64>,
    dropped: u64,
}

impl Player {
    fn spawn(candidate: Candidate) -> io::Result<Self> {
        let Candidate {
            name,
            mut command,
            format,
        } = candidate;
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut pipe = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("audio player stdin unavailable"))?;
        let (sender, receiver) = mpsc::sync_channel::<Packet>(8);
        let muted = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let pending = Arc::new(AtomicU64::new(0));
        let thread_pending = Arc::clone(&pending);
        let thread_epoch = Arc::clone(&epoch);
        let thread_muted = Arc::clone(&muted);
        let thread_failed = Arc::clone(&failed);
        let worker = thread::spawn(move || {
            let mut filter = HighPass::default();
            let mut prefill = AudioPrefill::default();
            let mut observed_epoch = 0;
            if let Err(error) = pipe.write_all(format.header()) {
                if !thread_failed.swap(true, Ordering::Relaxed) {
                    eprintln!("Audio {name} header failed: {error}");
                }
                return;
            }
            while let Ok(packet) = receiver.recv() {
                let epoch = thread_epoch.load(Ordering::Relaxed);
                if epoch != observed_epoch {
                    let discarded = prefill.reset();
                    thread_pending.fetch_sub(discarded as u64, Ordering::Relaxed);
                    filter = HighPass::default();
                    observed_epoch = epoch;
                }
                if packet.epoch != epoch || thread_muted.load(Ordering::Relaxed) {
                    thread_pending.fetch_sub(packet.samples.len() as u64, Ordering::Relaxed);
                    continue;
                }
                let Some(samples) = prefill.push(packet.samples) else {
                    continue;
                };
                let bytes = filter.encode(&samples, format);
                let result = pipe.write_all(&bytes);
                thread_pending.fetch_sub(samples.len() as u64, Ordering::Relaxed);
                if let Err(error) = result {
                    if !thread_failed.swap(true, Ordering::Relaxed) {
                        eprintln!("Audio {name} write failed: {error}");
                    }
                    break;
                }
            }
        });
        Ok(Self {
            name,
            child,
            sender: Some(sender),
            worker: Some(worker),
            muted,
            failed,
            epoch,
            pending,
            dropped: 0,
        })
    }

    fn failure(&mut self) -> Option<String> {
        // Check process status even without a new write: stdin may still look open
        // after a short-lived descendant inherited it, or the worker may be idle.
        match self.child.try_wait() {
            Ok(Some(status)) => Some(format!("exited with {status}")),
            Err(error) => Some(format!("cannot inspect player: {error}")),
            Ok(None) if self.failed.load(Ordering::Relaxed) => {
                Some("stream writer failed".to_owned())
            }
            Ok(None) => None,
        }
    }

    fn set_muted(&self, muted: bool) {
        if self.muted.swap(muted, Ordering::Relaxed) != muted {
            self.epoch.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn submit(&mut self, samples: Vec<[i16; 2]>) {
        if samples.is_empty() || self.muted.load(Ordering::Relaxed) {
            return;
        }
        let count = samples.len() as u64;
        if samples.len() > 2048 || self.failed.load(Ordering::Relaxed) {
            self.dropped += count;
            return;
        }
        if let Some(sender) = &self.sender {
            self.pending.fetch_add(count, Ordering::Relaxed);
            match sender.try_send(Packet {
                epoch: self.epoch.load(Ordering::Relaxed),
                samples,
            }) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                    self.pending.fetch_sub(count, Ordering::Relaxed);
                    self.dropped += count
                }
            }
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.failed.store(true, Ordering::Relaxed);
        self.sender.take();
        // Stop first to unblock pipe writes, then reap. Never wait indefinitely
        // for a worker whose pipe was inherited by an unexpected descendant.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(worker) = self.worker.take() {
            for _ in 0..20 {
                if worker.is_finished() {
                    let _ = worker.join();
                    return;
                }
                thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }
}

#[derive(Default)]
struct HighPass {
    previous: [i32; 2],
    state: [i32; 2],
}
impl HighPass {
    fn encode(&mut self, samples: &[[i16; 2]], format: StreamFormat) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        for frame in samples {
            for (side, sample) in frame.iter().enumerate() {
                let value = i32::from(*sample);
                self.state[side] = value - self.previous[side]
                    + ((i64::from(self.state[side]) * 65500) / 65536) as i32;
                self.previous[side] = value;
                let filtered = self.state[side].clamp(-32768, 32767) as i16;
                bytes.extend_from_slice(&format.sample(filtered));
            }
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::{AudioOutput, Candidate, SpeedReducer};
    use crate::stream_format::StreamFormat;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    fn candidate(name: &'static str, command: Command) -> Candidate {
        Candidate {
            name,
            command,
            format: StreamFormat::RawLittleEndian,
        }
    }
    fn shell(name: &'static str, script: &str) -> Candidate {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        candidate(name, command)
    }
    fn single(command: Command) -> std::io::Result<AudioOutput> {
        AudioOutput::from_candidates(vec![candidate("test", command)].into())
    }
    struct Trigger {
        path: PathBuf,
    }
    impl Trigger {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "gba-audio-trigger-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("new fixture directory");
            Self { path }
        }
        fn player(&self) -> Candidate {
            let mut command = Command::new("sh");
            command.args([
                "-c",
                "while [ ! -e \"$1\" ]; do sleep 0.01; done; exit 9",
                "_",
            ]);
            command.arg(self.path.join("exit"));
            candidate("exits-late", command)
        }
        fn fire(&self) {
            fs::write(self.path.join("exit"), b"exit").expect("release child");
        }
    }
    impl Drop for Trigger {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
    fn wait_for(output: &mut AudioOutput, name: Option<&str>) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            output.submit(Vec::new());
            if output.player_name() == name {
                break;
            }
            assert!(Instant::now() < deadline, "player did not reach {name:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn speed_reducer_compresses_time_and_carries_partial_groups_between_packets() {
        let mut reducer = SpeedReducer::default();
        reducer.set_factor(2);
        assert_eq!(
            reducer.reduce(vec![[100, -100], [300, -300], [500, -500]]),
            [[200, -200]]
        );
        assert_eq!(reducer.reduce(vec![[700, -700]]), [[600, -600]]);
        for factor in [2, 4, 8, 16] {
            reducer.set_factor(factor);
            let output = reducer.reduce(vec![[1600, -1600]; factor * 3]);
            assert_eq!(output, vec![[1600, -1600]; 3], "factor={factor}");
        }
    }

    #[test]
    fn audio_speed_rejects_unsupported_factors_and_keeps_tab_mute_independent() {
        let mut output = single(Command::new("cat")).expect("audio sink");
        assert!(output.set_speed(3).is_err());
        output.set_speed(16).expect("supported accelerated audio");
        output.submit(vec![[1600, -1600]; 1600]);
        assert_eq!(output.timing().submitted_frames, 100);
        output.set_muted(true);
        assert!(output.muted);
        output.set_muted(false);
        assert!(!output.muted);
    }

    #[test]
    fn filter_is_stereo_little_endian_and_removes_dc() {
        let mut f = super::HighPass::default();
        let format = crate::stream_format::StreamFormat::RawLittleEndian;
        assert_eq!(f.encode(&[[0x1234, -2]], format), [0x34, 0x12, 0xfe, 0xff]);
        let output = f.encode(&vec![[0x1234, -2]; 32768], format);
        assert_eq!(&output[output.len() - 4..], &[0; 4]);
    }
    #[test]
    fn au_payload_is_big_endian_after_filtering() {
        let input = [[1000, -1000], [0x1234, -2], [0, 0], [-300, 300]];
        let raw = super::HighPass::default()
            .encode(&input, crate::stream_format::StreamFormat::RawLittleEndian);
        let au = super::HighPass::default()
            .encode(&input, crate::stream_format::StreamFormat::AuBigEndian);
        assert_eq!(&au[..4], &[3, 232, 252, 24]);
        let decode = |bytes: &[u8], big: bool| {
            bytes
                .chunks_exact(2)
                .map(|pair| {
                    let pair = pair.try_into().expect("sample");
                    if big {
                        i16::from_be_bytes(pair)
                    } else {
                        i16::from_le_bytes(pair)
                    }
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(decode(&au, true), decode(&raw, false));
    }

    #[test]
    fn missing_player_and_invalid_backend_are_recoverable() {
        let command = std::process::Command::new("gba-audio-player-does-not-exist");
        assert!(single(command).is_err());
        assert!(super::AudioOutput::open("invalid").is_err());
    }
    #[test]
    fn early_exit_advances_to_next_candidate() {
        let mut output = AudioOutput::from_candidates(
            vec![
                shell("exits-now", "exit 7"),
                shell("working", "exec cat >/dev/null"),
            ]
            .into(),
        )
        .expect("fallback");
        assert_eq!(output.player_name(), Some("working"));
        output.submit(vec![[10, -10]; 512]);
        assert_eq!(output.dropped(), 0);
    }

    #[test]
    fn late_exit_falls_back_even_while_paused_and_no_audio_is_written() {
        let trigger = Trigger::new();
        let mut output = AudioOutput::from_candidates(
            vec![trigger.player(), shell("working", "exec cat >/dev/null")].into(),
        )
        .expect("initial player");
        assert_eq!(output.player_name(), Some("exits-late"));
        output.set_muted(true);
        trigger.fire();
        wait_for(&mut output, Some("working"));
        assert!(output.muted, "fallback preserves pause/fast-forward mute");
        output.set_muted(false);
        output.submit(vec![[10, -10]; 512]);
        assert_eq!(output.dropped(), 0);
    }

    #[test]
    fn broken_stdin_with_live_process_triggers_fallback() {
        let mut output = AudioOutput::from_candidates(
            vec![
                shell("closes-stdin", "exec 0<&-; while :; do :; done"),
                shell("working", "exec cat >/dev/null"),
            ]
            .into(),
        )
        .expect("initial player");
        assert_eq!(output.player_name(), Some("closes-stdin"));
        for _ in 0..4 {
            output.submit(vec![[10, -10]; 1024]);
        }
        wait_for(&mut output, Some("working"));
    }

    #[test]
    fn explicit_candidate_exhaustion_is_silent_without_restarting_a_failed_player() {
        let trigger = Trigger::new();
        let mut output =
            AudioOutput::from_candidates(vec![trigger.player()].into()).expect("player");
        trigger.fire();
        wait_for(&mut output, None);
        for _ in 0..3 {
            output.submit(vec![[0, 0]; 4]);
        }
        assert!(output.remaining.is_empty());
        assert_eq!(output.dropped(), 12);
    }

    #[test]
    fn worker_prefills_and_discards_old_epoch_packets_after_resume() {
        let fixture = Trigger::new();
        let path = fixture.path.join("pcm");
        let mut command = Command::new("sh");
        command.args(["-c", "exec cat > \"$1\"", "_"]).arg(&path);
        let mut output = single(command).expect("capture player");
        assert_eq!(output.needs_samples(), Some(true));
        output.submit(vec![[10, -10]; 2048]);
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(
            output
                .player
                .as_ref()
                .expect("player")
                .pending
                .load(Ordering::Relaxed),
            2048
        );
        assert_eq!(
            fs::metadata(&path).expect("capture").len(),
            0,
            "partial reserve not sent"
        );
        output.submit(vec![[10, -10]; 2048]);
        let wait_bytes = |expected| {
            let deadline = Instant::now() + Duration::from_secs(3);
            while fs::metadata(&path).expect("capture").len() < expected {
                assert!(Instant::now() < deadline, "worker did not deliver PCM");
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(fs::metadata(&path).expect("capture").len(), expected);
        };
        wait_bytes(4096 * 4);
        let old_epoch = output
            .player
            .as_ref()
            .expect("player")
            .epoch
            .load(Ordering::Relaxed);
        output.set_muted(true);
        assert_eq!(output.needs_samples(), None);
        output.set_muted(false);
        let before_load = output
            .player
            .as_ref()
            .expect("player")
            .epoch
            .load(Ordering::Relaxed);
        output.reset_after_state_load();
        assert_eq!(
            output
                .player
                .as_ref()
                .expect("player")
                .epoch
                .load(Ordering::Relaxed),
            before_load + 1
        );
        // Inject delayed pre-pause data after resume, deterministically modelling
        // the race where it was already queued when the user changed mode.
        output
            .player
            .as_ref()
            .expect("player")
            .pending
            .fetch_add(2048, Ordering::Relaxed);
        output
            .player
            .as_ref()
            .expect("player")
            .sender
            .as_ref()
            .expect("sender")
            .try_send(super::Packet {
                epoch: old_epoch,
                samples: vec![[999, 999]; 2048],
            })
            .expect("queued old epoch");
        output.submit(vec![[20, -20]; 512]);
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(fs::metadata(&path).expect("capture").len(), 4096 * 4);
        output.submit(vec![[20, -20]; 2048]);
        output.submit(vec![[20, -20]; 1536]);
        wait_bytes(8192 * 4);
        let deadline = Instant::now() + Duration::from_secs(3);
        while output
            .player
            .as_ref()
            .expect("player")
            .pending
            .load(Ordering::Relaxed)
            != 0
        {
            assert!(
                Instant::now() < deadline,
                "pending frame accounting did not drain"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn pending_hint_accounts_for_partial_reserve_discard_on_mode_change() {
        let mut output =
            AudioOutput::from_candidates(vec![shell("capture", "exec cat >/dev/null")].into())
                .expect("player");
        output.submit(vec![[1, -1]; 512]);
        output.set_muted(true);
        assert_eq!(output.needs_samples(), None);
        output.set_muted(false);
        output.submit(vec![[2, -2]; 256]);
        let deadline = Instant::now() + Duration::from_secs(3);
        while output
            .player
            .as_ref()
            .expect("player")
            .pending
            .load(Ordering::Relaxed)
            != 256
        {
            assert!(Instant::now() < deadline, "old reserve was not discarded");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(output.needs_samples(), Some(true));
    }

    #[test]
    fn stalled_player_does_not_block_emulation_or_shutdown() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "while :; do :; done"]);
        let mut output = single(command).expect("test child");
        let start = std::time::Instant::now();
        for _ in 0..100 {
            output.submit(vec![[0, 0]; 1024]);
        }
        assert!(output.dropped() > 0);
        assert_eq!(output.needs_samples(), Some(false));
        drop(output);
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
    }
}
