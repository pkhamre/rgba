//! Asset-free audio transport probe (not emulation or a perceptual quality test).
//! Usage: audio_probe [auto|pipewire|pulse|alsa] [SECONDS, max 60] [STALL_MS, max 500].

use gba_host::audio_output::AudioOutput;
use std::env;
use std::error::Error;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let backend = arguments.first().map(String::as_str).unwrap_or("auto");
    let seconds: u32 = arguments
        .get(1)
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(3);
    let stall_ms: u64 = arguments
        .get(2)
        .map(|value| value.parse())
        .transpose()?
        .unwrap_or(0);
    if arguments.len() > 3 || !(1..=60).contains(&seconds) || stall_ms > 500 {
        return Err("expected BACKEND SECONDS (1..60) STALL_MS (0..500)".into());
    }
    if stall_ms != 0 {
        eprintln!("Probe injects a {stall_ms} ms producer stall once per second");
    }
    let mut output = AudioOutput::open(backend)?;
    let start = Instant::now();
    // Quiet 256 Hz tone, right at half volume. Same phase avoids cancellation
    // if the device downmixes to mono. Each packet contains 512 stereo frames.
    for packet in 0..seconds * 64 {
        if packet != 0 && packet.is_multiple_of(64) {
            std::thread::sleep(Duration::from_millis(stall_ms));
        }
        let samples = (0..512)
            .map(|index| {
                let value = if (packet * 512 + index) % 128 < 64 {
                    1000
                } else {
                    -1000
                };
                [value, value / 2]
            })
            .collect();
        output.submit(samples);
        std::thread::sleep(
            (start + Duration::from_micros(u64::from(packet + 1) * 15625))
                .saturating_duration_since(Instant::now()),
        );
    }
    // Let the bounded host queue reach the player before stopping it.
    std::thread::sleep(Duration::from_millis(500));
    output.poll();
    eprintln!(
        "Probe: seconds={seconds}, player={:?}, host_dropped={}",
        output.player_name(),
        output.dropped()
    );
    let timing = output.timing();
    eprintln!(
        "Audio delivery: max_submit_gap_ms={:.1} gaps_over_40ms={} producer_hz={:.1} target_hz=32768",
        timing.max_submission_gap.as_secs_f64() * 1000.0,
        timing.gaps_over_40_ms,
        timing.submission_rate_hz().unwrap_or(0.0)
    );
    if output.player_name().is_none() {
        return Err("audio player stopped during the probe".into());
    }
    if output.dropped() != 0 {
        return Err("audio probe lost output frames".into());
    }
    Ok(())
}
