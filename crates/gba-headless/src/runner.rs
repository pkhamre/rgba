use crate::arguments;
use crate::arguments::Arguments;
use gba_host::Session;
use gba_host::screenshot;
use std::env;
use std::error::Error;
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
    let mut session = Session::try_new(&args.bios, &args.rom, None)?;
    eprintln!(
        "ROM: {}; save: {}",
        session.title(),
        session.save_path().display()
    );
    let machine = session.machine_mut();
    // Never truncate input images or existing artifacts by accident.
    let mut trace: Option<BufWriter<File>> = match &args.trace {
        Some(path) => Some(BufWriter::new(
            OpenOptions::new().write(true).create_new(true).open(path)?,
        )),
        None => None,
    };
    if let Some(frames) = args.frames {
        let mut event = 0;
        for frame in 0..frames {
            if event < args.input.len() && args.input[event].0 == frame {
                machine.set_keys(args.input[event].1);
                event += 1;
            }
            machine.run_frame().map_err(|error| {
                io::Error::other(format!(
                    "frame {frame}, PC {:08x}: {error}",
                    machine.cpu().instruction_address()
                ))
            })?;
        }
        if let Some(path) = &args.screenshot {
            screenshot::write_ppm(path, machine.screen())?;
        }
        eprintln!(
            "frames={} clocks={} PC={:08x}",
            machine.frames(),
            machine.clocks(),
            machine.cpu().instruction_address()
        );
        session.flush_save()?;
        return Ok(());
    }
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
    if let Some(path) = &args.screenshot {
        screenshot::write_ppm(path, machine.screen())?;
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
    session.flush_save()?;
    Ok(())
}
