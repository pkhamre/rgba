//! ROM-specific keyboard bindings for GBA buttons and desktop actions.

use std::fmt::Write as _;

const ACTION_COUNT: usize = 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum Action {
    A = 0,
    B,
    Select,
    Start,
    Right,
    Left,
    Up,
    Down,
    R,
    L,
    Pause,
    FastForward,
    Menu,
    SaveState,
    LoadState,
    FlushSave,
    Screenshot,
    Exit,
}

impl Action {
    pub(crate) const ALL: [Self; ACTION_COUNT] = [
        Self::A,
        Self::B,
        Self::Select,
        Self::Start,
        Self::Right,
        Self::Left,
        Self::Up,
        Self::Down,
        Self::R,
        Self::L,
        Self::Pause,
        Self::FastForward,
        Self::Menu,
        Self::SaveState,
        Self::LoadState,
        Self::FlushSave,
        Self::Screenshot,
        Self::Exit,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::A => "BUTTON A",
            Self::B => "BUTTON B",
            Self::Select => "SELECT",
            Self::Start => "START",
            Self::Right => "RIGHT",
            Self::Left => "LEFT",
            Self::Up => "UP",
            Self::Down => "DOWN",
            Self::R => "BUTTON R",
            Self::L => "BUTTON L",
            Self::Pause => "PAUSE",
            Self::FastForward => "FAST FORWARD",
            Self::Menu => "OPTIONS MENU",
            Self::SaveState => "SAVE STATE",
            Self::LoadState => "LOAD STATE",
            Self::FlushSave => "FLUSH GAME SAVE",
            Self::Screenshot => "SCREENSHOT",
            Self::Exit => "EXIT EMULATOR",
        }
    }

    pub(crate) const fn bit(self) -> Option<u16> {
        match self {
            Self::A => Some(1),
            Self::B => Some(2),
            Self::Select => Some(4),
            Self::Start => Some(8),
            Self::Right => Some(16),
            Self::Left => Some(32),
            Self::Up => Some(64),
            Self::Down => Some(128),
            Self::R => Some(256),
            Self::L => Some(512),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeyBindings {
    keys: [[u32; 2]; ACTION_COUNT],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BindingError {
    InvalidFormat,
    DuplicateKey,
    InvalidKey,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            keys: [
                [0x7a, 0],      // Z: A
                [0x78, 0],      // X: B
                [0xff08, 0],    // Backspace: Select
                [0xff0d, 0],    // Enter: Start
                [0xff53, 0x64], // Right / D
                [0xff51, 0x61], // Left / A
                [0xff52, 0x77], // Up / W
                [0xff54, 0x73], // Down / S
                [0x65, 0],      // E: R
                [0x71, 0],      // Q: L
                [0x20, 0],      // Space: pause
                [0xff09, 0],    // Tab: hold fast-forward
                [0xff1b, 0],    // Escape: options menu
                [0xffc3, 0],    // F6: quick state save
                [0xffc4, 0],    // F7: quick state load
                [0xffc2, 0],    // F5: flush cartridge save
                [0xffc9, 0],    // F12: screenshot
                [0, 0],         // no default exit shortcut
            ],
        }
    }
}

impl KeyBindings {
    pub(crate) fn keys(&self, action: Action) -> [u32; 2] {
        self.keys[action as usize]
    }

    pub(crate) fn rebind(&mut self, action: Action, key: u32) -> Result<(), BindingError> {
        if key == 0 || key > 0x0110_ffff {
            return Err(BindingError::InvalidKey);
        }
        if Action::ALL
            .into_iter()
            .any(|other| other != action && self.keys(other).contains(&key))
        {
            return Err(BindingError::DuplicateKey);
        }
        self.keys[action as usize] = [key, 0];
        Ok(())
    }

    pub(crate) fn action_for(&self, key: u32) -> Option<Action> {
        Action::ALL
            .into_iter()
            .find(|action| self.keys(*action).contains(&key) && key != 0)
    }

    pub(crate) fn button_mask(
        &self,
        held: &[bool],
        minimum: u8,
        columns: usize,
        symbols: &[u32],
    ) -> u16 {
        let mut pressed = 0;
        for (code, down) in held.iter().enumerate() {
            if !down {
                continue;
            }
            let symbol = code
                .checked_sub(usize::from(minimum))
                .and_then(|index| symbols.get(index * columns))
                .copied()
                .unwrap_or(0);
            if let Some(action) = self.action_for(symbol) {
                pressed |= action.bit().unwrap_or(0);
            }
        }
        pressed
    }

    pub(crate) fn encode(&self) -> String {
        let mut output = String::from("gba-keybindings-v1\n");
        for action in Action::ALL {
            let [first, second] = self.keys(action);
            let _ = writeln!(output, "{}={first:08x},{second:08x}", action.label());
        }
        output
    }

    pub(crate) fn decode(text: &str) -> Result<Self, BindingError> {
        let mut lines = text.lines();
        if lines.next() != Some("gba-keybindings-v1") {
            return Err(BindingError::InvalidFormat);
        }
        let mut keys = [[0; 2]; ACTION_COUNT];
        let mut seen = [false; ACTION_COUNT];
        for line in lines {
            let (name, values) = line.split_once('=').ok_or(BindingError::InvalidFormat)?;
            let index = Action::ALL
                .iter()
                .position(|action| action.label() == name)
                .ok_or(BindingError::InvalidFormat)?;
            if seen[index] {
                return Err(BindingError::InvalidFormat);
            }
            let (first, second) = values.split_once(',').ok_or(BindingError::InvalidFormat)?;
            keys[index] = [
                u32::from_str_radix(first, 16).map_err(|_| BindingError::InvalidFormat)?,
                u32::from_str_radix(second, 16).map_err(|_| BindingError::InvalidFormat)?,
            ];
            if keys[index].iter().any(|key| *key > 0x0110_ffff)
                || keys[index][0] == 0 && (index != Action::Exit as usize || keys[index][1] != 0)
            {
                return Err(BindingError::InvalidKey);
            }
            seen[index] = true;
        }
        if seen.contains(&false) {
            return Err(BindingError::InvalidFormat);
        }
        let bindings = Self { keys };
        let mut assigned = Vec::new();
        for action in Action::ALL {
            for key in bindings.keys(action) {
                if key == 0 {
                    continue;
                }
                if assigned.contains(&key) {
                    return Err(BindingError::DuplicateKey);
                }
                assigned.push(key);
            }
        }
        Ok(bindings)
    }
}

pub(crate) fn key_name(key: u32) -> String {
    if key == 0 {
        return "UNBOUND".to_owned();
    }
    let named = match key {
        0xff08 => Some("BACK"),
        0xff09 => Some("TAB"),
        0xff0d => Some("ENTER"),
        0xff1b => Some("ESC"),
        0xff51 => Some("LEFT"),
        0xff52 => Some("UP"),
        0xff53 => Some("RIGHT"),
        0xff54 => Some("DOWN"),
        0xffc2 => Some("F5"),
        0xffc3 => Some("F6"),
        0xffc4 => Some("F7"),
        0xffc9 => Some("F12"),
        0x20 => Some("SPACE"),
        _ => None,
    };
    named.map(str::to_owned).unwrap_or_else(|| {
        char::from_u32(key)
            .filter(|character| character.is_ascii_graphic())
            .map(|character| character.to_ascii_uppercase().to_string())
            .unwrap_or_else(|| format!("{key:04X}"))
    })
}

#[cfg(test)]
mod tests {
    use super::{Action, BindingError, KeyBindings};

    #[test]
    fn defaults_keep_existing_controls_and_add_distinct_state_shortcuts() {
        let bindings = KeyBindings::default();
        assert_eq!(bindings.action_for(0xff52), Some(Action::Up));
        assert_eq!(bindings.action_for(u32::from(b'w')), Some(Action::Up));
        assert_eq!(bindings.action_for(0xffc3), Some(Action::SaveState));
        assert_eq!(bindings.action_for(0xffc4), Some(Action::LoadState));
        assert_eq!(bindings.action_for(0xff1b), Some(Action::Menu));
    }

    #[test]
    fn settings_round_trip_and_duplicate_assignments_are_rejected() {
        let mut bindings = KeyBindings::default();
        bindings.rebind(Action::Pause, 0xffc5).expect("new key");
        let text = bindings.encode();
        assert_eq!(KeyBindings::decode(&text), Ok(bindings.clone()));
        assert_eq!(
            bindings.rebind(Action::Pause, 0xffc3),
            Err(BindingError::DuplicateKey)
        );
        assert_eq!(
            bindings.rebind(Action::Pause, 0),
            Err(BindingError::InvalidKey)
        );
        let duplicate = text.replace(
            "SAVE STATE=0000ffc3,00000000",
            "SAVE STATE=0000ffc5,00000000",
        );
        assert_eq!(
            KeyBindings::decode(&duplicate),
            Err(BindingError::DuplicateKey)
        );
    }

    #[test]
    fn rebind_removes_default_aliases_and_key_names_cover_defaults() {
        let mut bindings = KeyBindings::default();
        bindings
            .rebind(Action::Up, u32::from(b'i'))
            .expect("new key");
        assert_eq!(bindings.action_for(0xff52), None);
        assert_eq!(bindings.action_for(u32::from(b'w')), None);
        assert_eq!(bindings.action_for(u32::from(b'i')), Some(Action::Up));
        assert_eq!(super::key_name(0xffc3), "F6");
        assert_eq!(super::key_name(0x7a), "Z");
    }
}
