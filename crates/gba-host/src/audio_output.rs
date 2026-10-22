//! Bounded Linux audio transport through installed PipeWire/PulseAudio/ALSA tools.
//! No host callbacks or device timing can advance the emulated clock.

use std::io::{self, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread;
use std::thread::JoinHandle;

/// Bounded asynchronous PCM transport. Drop stops/reaps the player process.
pub struct AudioOutput {
    child: Child,
    sender: Option<SyncSender<Vec<[i16; 2]>>>,
    worker: Option<JoinHandle<()>>,
    muted: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    dropped: u64,
}

impl AudioOutput {
    /// Opens `auto`, `pipewire`, `pulse`, or `alsa` without invoking a shell.
    /// Installed players resample the 32,768 Hz stream for the actual device.
    pub fn open(backend: &str) -> io::Result<Self> {
        let candidates: &[&str] = match backend {
            "auto" => &["pipewire", "pulse", "alsa"],
            "pipewire" => &["pipewire"],
            "pulse" => &["pulse"],
            "alsa" => &["alsa"],
            _ => {
                return Err(io::Error::other(
                    "audio backend must be auto, pipewire, pulse, alsa, or off",
                ));
            }
        };
        let mut last = io::Error::other(
            "no audio player found; install pw-cat, pacat, or aplay, or use --audio off",
        );
        for candidate in candidates {
            let (program, args): (&str, &[&str]) = match *candidate {
                "pipewire" => (
                    "pw-cat",
                    &[
                        "--playback",
                        "--rate=32768",
                        "--channels=2",
                        "--format=s16",
                        "--latency=80ms",
                        "-",
                    ],
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
                    ],
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
                ),
            };
            let mut command = Command::new(program);
            command.args(args);
            match Self::spawn(command) {
                Ok(mut output) => {
                    // Catch immediate connection/CLI failures before trying the next tool.
                    thread::sleep(std::time::Duration::from_millis(40));
                    if let Some(status) = output.child.try_wait()? {
                        last = io::Error::other(format!("{program} exited on startup: {status}"));
                        continue;
                    }
                    eprintln!("Audio: {program}, 32768 Hz stereo (80 ms requested latency)");
                    return Ok(output);
                }
                Err(error) => last = error,
            }
        }
        Err(io::Error::other(format!(
            "audio unavailable: {last}; install pw-cat/pacat/aplay, select --audio pulse/alsa, or use --audio off"
        )))
    }

    fn spawn(mut command: Command) -> io::Result<Self> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut pipe = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("audio player stdin unavailable"))?;
        let (sender, receiver) = mpsc::sync_channel::<Vec<[i16; 2]>>(2);
        let muted = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let thread_muted = Arc::clone(&muted);
        let thread_failed = Arc::clone(&failed);
        let worker = thread::spawn(move || {
            let mut filter = HighPass::default();
            while let Ok(samples) = receiver.recv() {
                if thread_muted.load(Ordering::Relaxed) {
                    filter = HighPass::default();
                    continue;
                }
                let bytes = filter.encode(&samples);
                if let Err(error) = pipe.write_all(&bytes) {
                    if !thread_failed.swap(true, Ordering::Relaxed) {
                        eprintln!(
                            "Audio output stopped: {error}; gameplay continues silently. Select another --audio backend on restart."
                        );
                    }
                    break;
                }
            }
        });
        Ok(Self {
            child,
            sender: Some(sender),
            worker: Some(worker),
            muted,
            failed,
            dropped: 0,
        })
    }

    /// Disables new packets during pause/fast-forward. Already buffered device
    /// samples drain; core sound generation/guest timing are unaffected.
    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    /// Sends PCM without blocking emulation; at most two packets are queued.
    pub fn submit(&mut self, samples: Vec<[i16; 2]>) {
        if samples.is_empty() || self.muted.load(Ordering::Relaxed) {
            return;
        }
        let count = samples.len() as u64;
        if samples.len() > 2048 {
            self.dropped += count;
            return;
        }
        if self.failed.load(Ordering::Relaxed) {
            self.dropped += count;
            return;
        }
        if let Some(sender) = &self.sender {
            match sender.try_send(samples) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                    self.dropped += count
                }
            }
        }
    }

    /// PCM frames dropped by the bounded host queue or a failed player.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        self.failed.store(true, Ordering::Relaxed);
        self.sender.take();
        // Kill first: the worker may be blocked writing into the player's pipe.
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
            // Do not freeze the window on an unexpectedly stuck external pipe.
        }
    }
}

#[derive(Default)]
struct HighPass {
    previous: [i32; 2],
    state: [i32; 2],
}
impl HighPass {
    fn encode(&mut self, samples: &[[i16; 2]]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        for frame in samples {
            for (side, sample) in frame.iter().enumerate() {
                let value = i32::from(*sample);
                self.state[side] = value - self.previous[side]
                    + ((i64::from(self.state[side]) * 65500) / 65536) as i32;
                self.previous[side] = value;
                let filtered = self.state[side].clamp(-32768, 32767) as i16;
                bytes.extend_from_slice(&filtered.to_le_bytes());
            }
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn filter_is_stereo_little_endian_and_removes_dc() {
        let mut f = super::HighPass::default();
        assert_eq!(f.encode(&[[0x1234, -2]]), [0x34, 0x12, 0xfe, 0xff]);
        let output = f.encode(&vec![[0x1234, -2]; 32768]);
        assert_eq!(&output[output.len() - 4..], &[0; 4]);
    }
    #[test]
    fn missing_player_and_invalid_backend_are_recoverable() {
        let command = std::process::Command::new("gba-audio-player-does-not-exist");
        assert!(super::AudioOutput::spawn(command).is_err());
        assert!(super::AudioOutput::open("invalid").is_err());
    }
    #[test]
    fn stalled_player_does_not_block_emulation_or_shutdown() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "while :; do :; done"]);
        let mut output = super::AudioOutput::spawn(command).expect("test child");
        let start = std::time::Instant::now();
        for _ in 0..100 {
            output.submit(vec![[0, 0]; 1024]);
        }
        assert!(output.dropped() > 0);
        drop(output);
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
    }
}
