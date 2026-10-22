use std::path::PathBuf;

/// Bounded execution configuration. Asset paths are always explicit.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Arguments {
    pub(crate) bios: PathBuf,
    pub(crate) rom: PathBuf,
    pub(crate) steps: u64,
    pub(crate) trace: Option<PathBuf>,
    pub(crate) stop_at: Option<u32>,
    pub(crate) frames: Option<u64>,
    pub(crate) screenshot: Option<PathBuf>,
    pub(crate) input: Vec<(u64, u16)>,
}

pub(crate) const USAGE: &str = "\
gba-headless --bios PATH --rom PATH [--steps COUNT] [--trace PATH] [--stop-at HEX]
Runs at most COUNT CPU activities (default 100000); stops on unsupported hardware.
--stop-at stops before executing the address (hexadecimal, optional 0x prefix).
Trace files contain v1 post-activity state; exec/skip, halt, and irq identify the kind.
--frames COUNT runs display frames instead of counting CPU activities.
--screenshot PATH writes the final 240x160 framebuffer as a binary PPM image.
--input FRAME:BUTTONS changes held keys (e.g. 400:start, 410:none).
This is a development runner; audio output is not yet implemented.";

impl Arguments {
    pub(crate) fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut bios = None;
        let mut rom = None;
        let mut steps = None;
        let mut trace = None;
        let mut stop_at = None;
        let mut frames = None;
        let mut screenshot = None;
        let mut input = Vec::new();
        let mut args = args;
        while let Some(option) = args.next() {
            let value = match option.as_str() {
                "--bios" | "--rom" | "--steps" | "--trace" | "--stop-at" | "--frames"
                | "--screenshot" | "--input" => args
                    .next()
                    .ok_or_else(|| format!("missing value for {option}"))?,
                _ => return Err(format!("unknown option {option}")),
            };
            match option.as_str() {
                "--bios" if bios.is_none() => bios = Some(PathBuf::from(value)),
                "--rom" if rom.is_none() => rom = Some(PathBuf::from(value)),
                "--steps" if steps.is_none() => {
                    let count: u64 = value
                        .parse()
                        .map_err(|_| "steps must be an integer".to_owned())?;
                    if count == 0 {
                        return Err("steps must be greater than zero".to_owned());
                    }
                    steps = Some(count);
                }
                "--trace" if trace.is_none() => trace = Some(PathBuf::from(value)),
                "--stop-at" if stop_at.is_none() => {
                    let address = value.strip_prefix("0x").unwrap_or(&value);
                    stop_at =
                        Some(u32::from_str_radix(address, 16).map_err(|_| {
                            "stop-at must be a 32-bit hexadecimal address".to_owned()
                        })?);
                }
                "--frames" if frames.is_none() => {
                    let count: u64 = value
                        .parse()
                        .map_err(|_| "frames must be a positive integer")?;
                    if count == 0 {
                        return Err("frames must be positive".to_owned());
                    }
                    frames = Some(count);
                }
                "--screenshot" if screenshot.is_none() => screenshot = Some(PathBuf::from(value)),
                "--input" => {
                    let (frame, buttons) =
                        value.split_once(':').ok_or("input must be FRAME:BUTTONS")?;
                    let frame: u64 = frame
                        .parse()
                        .map_err(|_| "input frame must be an integer")?;
                    let mut keys = 0;
                    for button in buttons.to_ascii_lowercase().split('+') {
                        keys |= match button {
                            "none" => 0,
                            "a" => 1,
                            "b" => 2,
                            "select" => 4,
                            "start" => 8,
                            "right" => 16,
                            "left" => 32,
                            "up" => 64,
                            "down" => 128,
                            "r" => 256,
                            "l" => 512,
                            _ => return Err(format!("unknown button {button}")),
                        };
                    }
                    if input.last().is_some_and(|(last, _)| frame <= *last) {
                        return Err("input events must have increasing frame numbers".to_owned());
                    }
                    input.push((frame, keys));
                }
                _ => return Err(format!("duplicate option {option}")),
            }
        }
        if frames.is_some() && (steps.is_some() || stop_at.is_some() || trace.is_some()) {
            return Err(
                "--frames cannot be combined with --steps, --stop-at or --trace".to_owned(),
            );
        }
        if !input.is_empty() && frames.is_none() {
            return Err("--input requires --frames".to_owned());
        }
        Ok(Self {
            bios: bios.ok_or_else(|| "--bios is required".to_owned())?,
            rom: rom.ok_or_else(|| "--rom is required".to_owned())?,
            steps: steps.unwrap_or(100000),
            trace,
            stop_at,
            frames,
            screenshot,
            input,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Arguments;
    use std::path::PathBuf;

    fn parse(args: &[&str]) -> Result<Arguments, String> {
        Arguments::parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn accepts_paths_with_spaces_and_bounded_count() {
        let args = parse(&[
            "--bios",
            "bios.bin",
            "--rom",
            "game with spaces.gba",
            "--steps",
            "42",
            "--stop-at",
            "08000000",
        ])
        .expect("valid arguments");
        assert_eq!(args.bios, PathBuf::from("bios.bin"));
        assert_eq!(args.rom, PathBuf::from("game with spaces.gba"));
        assert_eq!(args.steps, 42);
        assert_eq!(args.stop_at, Some(0x0800_0000));
    }

    #[test]
    fn rejects_bad_or_ambiguous_arguments() {
        for args in [
            vec![],
            vec!["--bios"],
            vec!["--wat", "x"],
            vec!["--bios", "b", "--rom", "r", "--steps", "0"],
            vec!["--bios", "b", "--rom", "r", "--steps", "-1"],
            vec![
                "--bios",
                "b",
                "--rom",
                "r",
                "--steps",
                "18446744073709551616",
            ],
            vec!["--bios", "b", "--rom", "r", "--rom", "r2"],
            vec!["--bios", "b", "--rom", "r", "--stop-at", "xyz"],
        ] {
            assert!(parse(&args).is_err(), "must reject {args:?}");
        }
    }
}
