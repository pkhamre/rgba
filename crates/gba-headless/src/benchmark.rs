//! Unpaced, deterministic core measurements. No persistent saves are read or written.

use crate::arguments::Arguments;
use gba_core::{MAX_SAVE_STATE_SIZE, Machine};
use gba_host::screenshot;
use std::error::Error;
use std::fs;
use std::io::{self, Read};
use std::time::{Duration, Instant};

const MASTER_HZ: f64 = 16_777_216.0;
const HASH_SEED: u64 = 0xcbf2_9ce4_8422_2325;

// Stable FNV-1a is a diagnostic fingerprint, not an integrity/security guarantee.
fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash = (*hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
    }
}

struct Measurement {
    elapsed: Duration,
    frame_times: Vec<Duration>,
    clocks: u64,
    pcm_frames: u64,
    pcm_hash: u64,
    state: Vec<u8>,
}

fn measure(
    machine: &mut Machine,
    frames: u64,
    input: &[(u64, u16)],
) -> Result<Measurement, Box<dyn Error>> {
    let mut frame_times = Vec::new();
    let mut pcm_hash = HASH_SEED;
    let mut pcm_frames = 0;
    let clocks = machine.clocks();
    let mut event = 0;
    let start = Instant::now();
    for frame in 0..frames {
        if event < input.len() && input[event].0 == frame {
            machine.set_keys(input[event].1);
            event += 1;
        }
        let frame_start = Instant::now();
        machine.run_frame().map_err(|error| {
            io::Error::other(format!(
                "benchmark frame {frame}, PC {:08x}: {error}",
                machine.cpu().instruction_address()
            ))
        })?;
        // Drain like the desktop, rather than timing an overflowing core queue.
        let samples = machine.take_audio();
        for sample in &samples {
            for channel in sample {
                hash_bytes(&mut pcm_hash, &channel.to_le_bytes());
            }
        }
        pcm_frames += samples.len() as u64;
        frame_times.push(frame_start.elapsed());
    }
    let elapsed = start.elapsed();
    Ok(Measurement {
        elapsed,
        frame_times,
        clocks: machine.clocks() - clocks,
        pcm_frames,
        pcm_hash,
        // Serialization and equality checks are deliberately outside the timer.
        state: machine.save_state()?,
    })
}

pub(crate) fn run(args: &Arguments) -> Result<(), Box<dyn Error>> {
    let mut baseline = Machine::try_new(&fs::read(&args.bios)?, &fs::read(&args.rom)?)?;
    if let Some(path) = &args.state {
        let mut bytes = Vec::new();
        fs::File::open(path)?
            .take(MAX_SAVE_STATE_SIZE as u64 + 1)
            .read_to_end(&mut bytes)?;
        baseline.load_state(&bytes)?;
    }
    for _ in 0..args.warmup_frames {
        baseline.run_frame()?;
        baseline.take_audio();
    }
    // A loaded state may contain queued PCM even with zero warm-up.
    baseline.take_audio();
    let frames = args.frames.expect("benchmark requires frames");
    eprintln!(
        "benchmark_v1 frames={frames} warmup_frames={} repeats={} build={} arch={} os={}",
        args.warmup_frames,
        args.repeats,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "optimized"
        },
        std::env::consts::ARCH,
        std::env::consts::OS
    );
    let mut reference: Option<Measurement> = None;
    let mut elapsed_runs = Vec::new();
    for repeat in 1..=args.repeats {
        // Clone outside the measured region; all repeats start identically.
        let mut machine = baseline.clone();
        let mut result = measure(&mut machine, frames, &args.input)?;
        if let Some(expected) = &reference
            && (result.state != expected.state
                || result.pcm_hash != expected.pcm_hash
                || result.pcm_frames != expected.pcm_frames
                || result.clocks != expected.clocks)
        {
            return Err(io::Error::other(format!(
                "benchmark repeat {repeat} differs from repeat 1"
            ))
            .into());
        }
        result.frame_times.sort_unstable();
        let seconds = result.elapsed.as_secs_f64();
        let emulated_seconds = result.clocks as f64 / MASTER_HZ;
        let p95_index = (result.frame_times.len() - 1) * 95 / 100;
        let mut state_hash = HASH_SEED;
        hash_bytes(&mut state_hash, &result.state);
        eprintln!(
            "repeat={repeat} wall_s={seconds:.6} frames_per_s={:.3} realtime_x={:.3} frame_avg_ms={:.3} frame_p95_ms={:.3} frame_max_ms={:.3} clocks={} pcm_frames={} pcm_hash={:016x} state_hash={state_hash:016x}",
            frames as f64 / seconds,
            emulated_seconds / seconds,
            seconds * 1000.0 / frames as f64,
            result.frame_times[p95_index].as_secs_f64() * 1000.0,
            result
                .frame_times
                .last()
                .expect("nonempty measurement")
                .as_secs_f64()
                * 1000.0,
            result.clocks,
            result.pcm_frames,
            result.pcm_hash
        );
        elapsed_runs.push(result.elapsed);
        if repeat == args.repeats
            && let Some(path) = &args.screenshot
        {
            screenshot::write_ppm(path, machine.screen())?;
        }
        if reference.is_none() {
            reference = Some(result);
        }
    }
    elapsed_runs.sort_unstable();
    eprintln!(
        "benchmark_complete deterministic=true median_wall_s={:.6} (upper median for even repeats)",
        elapsed_runs[elapsed_runs.len() / 2].as_secs_f64()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::measure;
    use gba_core::Machine;

    fn machine() -> Machine {
        let mut bios = vec![0; 16 * 1024];
        // ARM branch to self, allowing hardware to advance without game assets.
        bios[..4].copy_from_slice(&0xeaff_fffe_u32.to_le_bytes());
        Machine::try_new(&bios, &[0; 192]).expect("synthetic machine")
    }

    #[test]
    fn repeated_measurements_preserve_state_and_pcm() {
        let baseline = machine();
        let mut first = baseline.clone();
        let mut second = baseline.clone();
        let input = [(0, 1), (1, 0)];
        let a = measure(&mut first, 3, &input).expect("first run");
        let b = measure(&mut second, 3, &input).expect("second run");
        assert_eq!(a.state, b.state);
        assert_eq!(a.pcm_hash, b.pcm_hash);
        assert_eq!(a.clocks, b.clocks);
        assert_eq!(a.pcm_frames, b.pcm_frames);
        assert!(a.pcm_frames > 0);
        assert_eq!(a.frame_times.len(), 3);
        assert_eq!(baseline.frames(), 0, "baseline must not advance");
        assert_eq!(first.audio_dropped(), 0, "PCM must be drained each frame");
    }

    #[test]
    fn scripted_input_changes_final_state() {
        let mut first = machine();
        let mut second = first.clone();
        let a = measure(&mut first, 2, &[]).expect("no keys");
        let b = measure(&mut second, 2, &[(0, 1)]).expect("A held");
        assert_ne!(a.state, b.state);
    }
}
