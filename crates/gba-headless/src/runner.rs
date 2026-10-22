use crate::arguments;
use crate::arguments::Arguments;
use gba_core::Machine;
use std::env;
use std::error::Error;
use std::fs;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter};

pub(crate) fn run() -> Result<(), Box<dyn Error>> {
    use std::io::Write;

    let args = Arguments::parse(env::args().skip(1)).map_err(|message| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{message}\n{}", arguments::USAGE),
        )
    })?;
    let bios = fs::read(&args.bios).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("BIOS {}: {error}", args.bios.display()),
        )
    })?;
    let rom = fs::read(&args.rom).map_err(|error| {
        io::Error::new(error.kind(), format!("ROM {}: {error}", args.rom.display()))
    })?;
    let mut machine = Machine::try_new(&bios, &rom)?;
    // Never truncate input images or existing artifacts by accident.
    let mut trace: Option<BufWriter<File>> = match &args.trace {
        Some(path) => Some(BufWriter::new(
            OpenOptions::new().write(true).create_new(true).open(path)?,
        )),
        None => None,
    };
    let title = String::from_utf8_lossy(&rom[0xa0..0xac]);
    let game_code = String::from_utf8_lossy(&rom[0xac..0xb0]);
    eprintln!(
        "ROM: {title} ({game_code}), {} bytes; BIOS: {} bytes",
        rom.len(),
        bios.len()
    );
    let mut completed = 0;
    let mut fault = None;
    while completed < args.steps {
        if args.stop_at == Some(machine.cpu().instruction_address()) {
            break;
        }
        match machine.step() {
            Ok(step) => {
                completed += 1;
                if let Some(writer) = &mut trace {
                    writeln!(writer, "{step}")?;
                }
            }
            Err(error) => {
                fault = Some(error);
                break;
            }
        }
    }
    if let Some(writer) = &mut trace {
        writer.flush()?;
    }
    let cpu = machine.cpu();
    eprintln!(
        "completed={completed} clocks={} next={:08x} cpsr={:08x}",
        machine.clocks(),
        cpu.instruction_address(),
        cpu.status().bits()
    );
    if let Some(error) = fault {
        return Err(error.into());
    }
    if let Some(target) = args.stop_at
        && cpu.instruction_address() != target
    {
        return Err(io::Error::other(format!(
            "instruction budget exhausted before target {target:08x}"
        ))
        .into());
    }
    Ok(())
}
