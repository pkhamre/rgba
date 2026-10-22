use std::path::PathBuf;

pub(crate) const USAGE: &str = "\
gba-desktop --bios PATH --rom PATH [--save PATH] [--scale 1..6] [--frames COUNT] [--audio auto|pipewire|pulse|alsa|off]
Linux X11/XWayland preview. Audio needs pw-cat, pacat, or aplay installed.
Controls: arrows/WASD = D-pad; Z = A; X = B; Enter = Start; Backspace = Select;
Q/E = L/R; Space = pause; Tab = fast forward; F5 = flush save;
F6/F7 = quick state save/load; F12 = screenshot; Escape = options menu.
Choose Exit or close the window to quit and flush cartridge data.
Save defaults to ROM basename.sav; previous backup is kept in .sav.bak.
--frames exits after a bounded number of frames (useful for Xvfb smoke tests).";

#[derive(Debug)]
pub(crate) struct Arguments {
    pub(crate) bios: PathBuf,
    pub(crate) rom: PathBuf,
    pub(crate) save: Option<PathBuf>,
    pub(crate) scale: u16,
    pub(crate) frames: Option<u64>,
    pub(crate) audio: String,
}

impl Arguments {
    pub(crate) fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut bios = None;
        let mut rom = None;
        let mut save = None;
        let mut scale = None;
        let mut frames = None;
        let mut audio = None;
        while let Some(option) = arguments.next() {
            if !matches!(
                option.as_str(),
                "--bios" | "--rom" | "--save" | "--scale" | "--frames" | "--audio"
            ) {
                return Err(format!("unknown option {option}"));
            }
            let value = arguments
                .next()
                .ok_or_else(|| format!("missing value for {option}"))?;
            match option.as_str() {
                "--audio" if audio.is_none() => {
                    if !matches!(
                        value.as_str(),
                        "auto" | "pipewire" | "pulse" | "alsa" | "off"
                    ) {
                        return Err("audio must be auto, pipewire, pulse, alsa, or off".to_owned());
                    }
                    audio = Some(value);
                }
                "--bios" if bios.is_none() => bios = Some(PathBuf::from(value)),
                "--rom" if rom.is_none() => rom = Some(PathBuf::from(value)),
                "--save" if save.is_none() => save = Some(PathBuf::from(value)),
                "--scale" if scale.is_none() => {
                    let number: u16 = value.parse().map_err(|_| "scale must be 1..6")?;
                    if !(1..=6).contains(&number) {
                        return Err("scale must be 1..6".to_owned());
                    }
                    scale = Some(number);
                }
                "--frames" if frames.is_none() => {
                    let number: u64 = value.parse().map_err(|_| "frames must be positive")?;
                    if number == 0 {
                        return Err("frames must be positive".to_owned());
                    }
                    frames = Some(number);
                }
                _ => return Err(format!("duplicate option {option}")),
            }
        }
        Ok(Self {
            bios: bios.ok_or("--bios is required")?,
            rom: rom.ok_or("--rom is required")?,
            save,
            scale: scale.unwrap_or(3),
            frames,
            audio: audio.unwrap_or_else(|| "auto".to_owned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Arguments;
    #[test]
    fn validates_options_without_assets() {
        let parse = |args: &[&str]| Arguments::parse(args.iter().map(|arg| (*arg).to_owned()));
        assert_eq!(
            parse(&["--bios", "b", "--rom", "r", "--scale", "2"])
                .expect("valid")
                .scale,
            2
        );
        for args in [
            vec![],
            vec!["--bios", "b", "--rom", "r", "--scale", "0"],
            vec!["--bios", "b", "--rom", "r", "--frames", "0"],
            vec!["--rom", "r", "--rom", "r"],
        ] {
            assert!(parse(&args).is_err());
        }
    }
}
