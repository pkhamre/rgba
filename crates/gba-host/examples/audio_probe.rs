//! Asset-free audio transport probe (not emulation or a perceptual quality test).
//! Usage: audio_probe [auto|pipewire|pulse|alsa] [SECONDS, max 60].

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
    if arguments.len() > 2 || !(1..=60).contains(&seconds) {
        return Err("expected BACKEND SECONDS (1..60)".into());
    }
    let mut output = AudioOutput::open(backend)?;
    let start = Instant::now();
    // A quiet 256 Hz square tone, independent channels, 512-frame packets.
    for packet in 0..seconds * 64 {
        let samples = (0..512)
            .map(|index| {
                let value = if (packet * 512 + index) % 128 < 64 {
                    1000
                } else {
                    -1000
                };
                [value, -value]
            })
            .collect();
        output.submit(samples);
        std::thread::sleep(
            (start + Duration::from_micros(u64::from(packet + 1) * 15625))
                .saturating_duration_since(Instant::now()),
        );
    }
    // Let the bounded host queue reach the player before stopping it.
    std::thread::sleep(Duration::from_millis(250));
    eprintln!(
        "Probe: seconds={seconds}, host_dropped={}",
        output.dropped()
    );
    if output.dropped() != 0 {
        return Err("audio probe lost output frames".into());
    }
    Ok(())
}
