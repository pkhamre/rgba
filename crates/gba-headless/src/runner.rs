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
    let mut completion: Option<u32> = None;
    let mut previous_idle: Option<(u32, u32, u32)> = None;
    let mut idle_repeats = 0;
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
                if let Some(register) = args.test_result_register {
                    if let Some(value) = self_branch_result(
                        step.address,
                        step.opcode,
                        step.thumb,
                        step.registers[register],
                    ) {
                        let current = (step.address, step.opcode, value);
                        if previous_idle == Some(current) {
                            idle_repeats += 1;
                        } else {
                            idle_repeats = 0;
                        }
                        previous_idle = Some(current);
                        if idle_repeats >= 2 {
                            completion = Some(value);
                            break;
                        }
                    } else {
                        previous_idle = None;
                        idle_repeats = 0;
                    }
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
    if let Some(register) = args.test_result_register {
        let value = completion.ok_or_else(|| {
            io::Error::other(format!(
                "test did not reach a stable self-branch within {} CPU activities",
                args.steps
            ))
        })?;
        if value == 0 {
            eprintln!("TEST_RESULT=PASS register=r{register} value=00000000");
        } else {
            eprintln!("TEST_RESULT=FAIL register=r{register} value={value:08x}");
            return Err(io::Error::other("hardware test ROM reported a failure").into());
        }
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

fn self_branch_result(address: u32, opcode: u32, thumb: bool, result: u32) -> Option<u32> {
    let target = if thumb {
        if opcode & 0xf800 != 0xe000 {
            return None;
        }
        let immediate = (opcode & 0x07ff) as i32;
        let signed = if immediate & 0x0400 != 0 {
            immediate - 0x0800
        } else {
            immediate
        };
        address.wrapping_add(4).wrapping_add((signed * 2) as u32)
    } else {
        if opcode >> 28 != 0xe || opcode & 0x0e00_0000 != 0x0a00_0000 {
            return None;
        }
        let displacement = ((opcode << 8) as i32 >> 6) as u32;
        address.wrapping_add(8).wrapping_add(displacement)
    };
    (target == address).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::self_branch_result;
    #[test]
    fn recognizes_only_arm_and_thumb_self_branches() {
        assert_eq!(
            self_branch_result(0x0800_0100, 0xeaff_fffe, false, 12),
            Some(12)
        );
        assert_eq!(self_branch_result(0x0800_0100, 0xe7fe, true, 7), Some(7));
        assert_eq!(
            self_branch_result(0x0800_0100, 0xeaff_fffd, false, 12),
            None
        );
        assert_eq!(self_branch_result(0x0800_0100, 0xe7fd, true, 7), None);
    }
}
