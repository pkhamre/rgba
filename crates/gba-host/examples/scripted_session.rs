//! Bounded, stdin-driven validation sessions using ordinary emulated keys.
//! Usage: scripted_session BIOS ROM SAVE (use a disposable SAVE path).
//! Commands: run FRAMES KEYMASK, shot PATH, wav PATH, stop-wav, flush, quit.

use gba_host::Session;
use gba_host::screenshot;
use gba_host::wav::WavWriter;
use std::env;
use std::error::Error;
use std::io::{self, BufRead, Write};
use std::path::Path;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "expected BIOS ROM SAVE; commands: run FRAMES KEYMASK, shot PATH, wav PATH, stop-wav, flush, quit".into(),
        );
    }
    let mut session = Session::try_new(
        Path::new(&args[0]),
        Path::new(&args[1]),
        Some(Path::new(&args[2])),
    )?;
    let mut wav: Option<WavWriter> = None;
    let mut nonzero = 0u64;
    let mut peak = 0i32;
    for line in io::stdin().lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields.as_slice() {
            ["run", frames, keys] | ["run", frames, keys, _] => {
                let frames: u64 = frames.parse()?;
                let keys: u16 = keys.parse()?;
                if frames > 200_000 || keys > 1023 {
                    return Err("run bound is 200000 frames, key mask 0..1023".into());
                }
                session.machine_mut().set_keys(keys);
                for _ in 0..frames {
                    session.machine_mut().run_frame()?;
                    let samples = session.machine_mut().take_audio();
                    for frame in &samples {
                        for value in frame {
                            nonzero += u64::from(*value != 0);
                            peak = peak.max(i32::from(*value).abs());
                        }
                    }
                    if let Some(writer) = &mut wav {
                        writer.write(&samples)?;
                    }
                }
                if fields.len() == 4 {
                    screenshot::write_ppm(Path::new(fields[3]), session.machine().screen())?;
                }
            }
            ["shot", _, ..] => {
                screenshot::write_ppm(Path::new(line[4..].trim()), session.machine().screen())?
            }
            ["wav", _, ..] if wav.is_none() => {
                wav = Some(WavWriter::create(
                    Path::new(line[3..].trim()),
                    gba_core::AUDIO_SAMPLE_RATE,
                )?);
            }
            ["stop-wav"] => {
                if let Some(writer) = wav.take() {
                    writer.finish()?;
                }
            }
            ["flush"] => {
                session.flush_save()?;
            }
            ["quit"] => break,
            _ => {
                return Err(
                    "commands: run FRAMES KEYMASK, shot PATH, wav PATH, stop-wav, flush, quit"
                        .into(),
                );
            }
        }
        println!(
            "OK frames={} clocks={} pc={:08x} dirty={} revision={}",
            session.machine().frames(),
            session.machine().clocks(),
            session.machine().cpu().instruction_address(),
            session.machine().save_dirty(),
            session.machine().save_revision()
        );
        println!(
            "PCM frames={} dropped={} nonzero={} peak={}",
            session.machine().audio_frames(),
            session.machine().audio_dropped(),
            nonzero,
            peak
        );
        io::stdout().flush()?;
    }
    if let Some(writer) = wav {
        writer.finish()?;
    }
    session.flush_save()?;
    Ok(())
}
