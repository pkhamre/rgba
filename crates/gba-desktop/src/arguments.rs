use std::path::PathBuf;

pub(crate) const USAGE: &str = "\
gba-desktop --bios PATH [--rom PATH] [--save PATH] [--scale 1..6] [--frames COUNT] [--audio auto|pipewire|pulse|alsa|off]
Linux X11/XWayland preview. Audio needs pw-cat, pacat, or aplay installed.
Controls: arrows/WASD = D-pad; Z = A; X = B; Enter = Start; Backspace = Select;
Q/E = L/R; Space = pause; Tab = fast forward; F5 = flush save;
F6/F7 = quick state save/load; F12 = screenshot; Escape = options menu.
Options includes Back to Library; Exit or close the window to quit.
Without --rom, games are scanned from $XDG_DATA_HOME/rgba/games (default
~/.local/share/rgba/games). Saves and states use content-hash names there;
settings use $XDG_CONFIG_HOME/rgba/settings (default ~/.config/rgba/settings).
--save overrides the cartridge-save path for an explicit --rom.
--frames exits after a bounded number of frames (useful for Xvfb smoke tests).";

#[derive(Debug)]
pub(crate) struct Arguments {
    pub(crate) bios: PathBuf,
    pub(crate) rom: Option<PathBuf>,
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
        if save.is_some() && rom.is_none() {
            return Err("--save requires --rom".to_owned());
        }
        Ok(Self {
            bios: bios.ok_or("--bios is required")?,
            rom,
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
        assert_eq!(parse(&["--bios", "b"]).expect("library mode").rom, None);
        for args in [
            vec![],
            vec!["--bios", "b", "--rom", "r", "--scale", "0"],
            vec!["--bios", "b", "--rom", "r", "--frames", "0"],
            vec!["--rom", "r", "--rom", "r"],
            vec!["--bios", "b", "--save", "s"],
        ] {
            assert!(parse(&args).is_err());
        }
    }
}
