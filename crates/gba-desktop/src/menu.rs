//! Host-only options and keybinding menus; guest pixels and state are untouched.

use crate::keybindings::{Action, BindingError, KeyBindings, key_name};

const OPTIONS: usize = 5;
const KEY_ROWS: usize = 7;
const RESET_INDEX: usize = Action::ALL.len();
const BACK_INDEX: usize = RESET_INDEX + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    None,
    Resume,
    Exit,
    BackToLibrary,
    SaveState,
    LoadState,
    SettingsChanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Options,
    Keybindings,
}

#[derive(Clone, Debug)]
pub(crate) struct MenuState {
    page: Page,
    selected: usize,
    capture: Option<Action>,
    status: String,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            page: Page::Options,
            selected: 0,
            capture: None,
            status: String::new(),
        }
    }
}

impl MenuState {
    pub(crate) fn handle_key(&mut self, symbol: u32, bindings: &mut KeyBindings) -> Command {
        if let Some(action) = self.capture {
            if symbol == 0xff1b {
                self.capture = None;
                self.status = "BIND CANCELLED".to_owned();
                return Command::None;
            }
            match bindings.rebind(action, symbol) {
                Ok(()) => {
                    self.capture = None;
                    self.status = "BIND CHANGED".to_owned();
                    return Command::SettingsChanged;
                }
                Err(BindingError::DuplicateKey) => self.status = "KEY ALREADY USED".to_owned(),
                Err(BindingError::InvalidKey) => self.status = "KEY NOT AVAILABLE".to_owned(),
                Err(BindingError::InvalidFormat) => self.status = "SETTINGS INVALID".to_owned(),
            }
            return Command::None;
        }

        let action = bindings.action_for(symbol);
        if action == Some(Action::Exit) {
            return Command::Exit;
        }
        let back = action == Some(Action::Menu) || action == Some(Action::B);
        if back {
            match self.page {
                Page::Options => return Command::Resume,
                Page::Keybindings => {
                    self.page = Page::Options;
                    self.selected = 2;
                    self.status.clear();
                    return Command::None;
                }
            }
        }
        if action == Some(Action::Up) {
            let length = self.page_length();
            self.selected = (self.selected + length - 1) % length;
            return Command::None;
        }
        if action == Some(Action::Down) {
            self.selected = (self.selected + 1) % self.page_length();
            return Command::None;
        }
        if !matches!(action, Some(Action::A | Action::Start)) {
            return Command::None;
        }
        self.status.clear();
        match self.page {
            Page::Options => match self.selected {
                0 => Command::SaveState,
                1 => Command::LoadState,
                2 => {
                    self.page = Page::Keybindings;
                    self.selected = 0;
                    Command::None
                }
                3 => Command::BackToLibrary,
                _ => Command::Exit,
            },
            Page::Keybindings if self.selected < Action::ALL.len() => {
                let action = Action::ALL[self.selected];
                self.capture = Some(action);
                self.status = "PRESS KEY - ESC CANCELS".to_owned();
                Command::None
            }
            Page::Keybindings if self.selected == RESET_INDEX => {
                *bindings = KeyBindings::default();
                self.status = "DEFAULTS RESTORED".to_owned();
                Command::SettingsChanged
            }
            Page::Keybindings => {
                self.page = Page::Options;
                self.selected = 3;
                Command::None
            }
        }
    }

    pub(crate) fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    pub(crate) fn render(&self, pixels: &[u32], bindings: &KeyBindings) -> Vec<u32> {
        let mut output = pixels.to_vec();
        let (left, top, right, bottom) = (12, 12, 228, 148);
        for y in top..bottom {
            for x in left..right {
                output[y * 240 + x] = if x == left || x == right - 1 || y == top || y == bottom - 1
                {
                    0xffffff
                } else {
                    0x101820
                };
            }
        }
        match self.page {
            Page::Options => self.draw_options(&mut output),
            Page::Keybindings => self.draw_bindings(&mut output, bindings),
        }
        if !self.status.is_empty() {
            draw_text(&mut output, 22, 132, &self.status, 0xffd060);
        }
        output
    }

    fn page_length(&self) -> usize {
        match self.page {
            Page::Options => OPTIONS,
            Page::Keybindings => BACK_INDEX + 1,
        }
    }

    fn draw_options(&self, output: &mut [u32]) {
        draw_text(output, 82, 22, "OPTIONS", 0x80d8ff);
        let entries = [
            "SAVE STATE",
            "LOAD STATE",
            "KEYBINDS",
            "BACK TO LIBRARY",
            "EXIT",
        ];
        for (index, entry) in entries.iter().enumerate() {
            let y = 43 + index * 15;
            draw_text(
                output,
                34,
                y,
                if index == self.selected { ">" } else { " " },
                0xffd060,
            );
            draw_text(
                output,
                48,
                y,
                entry,
                if index == self.selected {
                    0xffd060
                } else {
                    0xffffff
                },
            );
        }
        draw_text(output, 26, 119, "UP DOWN MOVE  A SELECT", 0x90a0b0);
    }

    fn draw_bindings(&self, output: &mut [u32], bindings: &KeyBindings) {
        draw_text(output, 65, 17, "KEYBINDINGS", 0x80d8ff);
        let start = self
            .selected
            .saturating_sub(KEY_ROWS - 1)
            .min(Action::ALL.len());
        for row in 0..KEY_ROWS {
            let index = start + row;
            if index > BACK_INDEX {
                break;
            }
            let (label, value) = if index < Action::ALL.len() {
                let action = Action::ALL[index];
                let keys = bindings.keys(action);
                let first = key_name(keys[0]);
                let second = if keys[1] == 0 {
                    String::new()
                } else {
                    format!("/{}", key_name(keys[1]))
                };
                (action.label().to_owned(), format!("{first}{second}"))
            } else if index == RESET_INDEX {
                ("RESET DEFAULTS".to_owned(), String::new())
            } else {
                ("BACK".to_owned(), String::new())
            };
            let y = 31 + row * 13;
            draw_text(
                output,
                21,
                y,
                if index == self.selected { ">" } else { " " },
                0xffd060,
            );
            draw_text(
                output,
                33,
                y,
                &label,
                if index == self.selected {
                    0xffd060
                } else {
                    0xffffff
                },
            );
            draw_text(output, 147, y, &value, 0x80d8ff);
        }
        if self.capture.is_some() {
            draw_text(output, 22, 124, "PRESS KEY - ESC CANCELS", 0xffd060);
        } else if self.status.is_empty() {
            draw_text(output, 22, 124, "A REBIND  START RESET", 0x90a0b0);
        }
    }
}

pub(crate) fn draw_text(output: &mut [u32], x: usize, y: usize, text: &str, color: u32) {
    for (character, letter) in text.chars().enumerate() {
        let glyph = glyph(letter);
        for (row, bits) in glyph.iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    let px = x + character * 6 + column;
                    let py = y + row;
                    if px < 240 && py < 160 {
                        output[py * 240 + px] = color;
                    }
                }
            }
        }
    }
}

fn glyph(character: char) -> [u8; 7] {
    match character {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        ':' => [0, 4, 4, 0, 4, 4, 0],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '/' => [1, 2, 2, 4, 8, 8, 16],
        '>' => [16, 8, 4, 2, 4, 8, 16],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, MenuState};
    use crate::keybindings::{Action, KeyBindings};

    #[test]
    fn menu_selects_save_load_and_keybinding_page_then_resumes() {
        let mut menu = MenuState::default();
        let mut bindings = KeyBindings::default();
        assert_eq!(menu.handle_key(0xff0d, &mut bindings), Command::SaveState);
        menu.handle_key(0xff54, &mut bindings);
        assert_eq!(menu.handle_key(0xff0d, &mut bindings), Command::LoadState);
        menu.handle_key(0xff54, &mut bindings);
        assert_eq!(menu.handle_key(0xff0d, &mut bindings), Command::None);
        assert_eq!(menu.handle_key(0xff1b, &mut bindings), Command::None);
        assert_eq!(menu.handle_key(0xff1b, &mut bindings), Command::Resume);
    }

    #[test]
    fn explicit_exit_is_a_menu_action() {
        let mut menu = MenuState::default();
        let mut bindings = KeyBindings::default();
        for _ in 0..4 {
            menu.handle_key(0xff54, &mut bindings);
        }
        assert_eq!(menu.handle_key(0xff0d, &mut bindings), Command::Exit);
    }

    #[test]
    fn menu_returns_to_the_game_library() {
        let mut menu = MenuState::default();
        let mut bindings = KeyBindings::default();
        for _ in 0..3 {
            menu.handle_key(0xff54, &mut bindings);
        }
        assert_eq!(
            menu.handle_key(0xff0d, &mut bindings),
            Command::BackToLibrary
        );
    }

    #[test]
    fn keybinding_capture_rejects_conflict_and_accepts_new_key() {
        let mut menu = MenuState::default();
        let mut bindings = KeyBindings::default();
        for _ in 0..2 {
            menu.handle_key(0xff54, &mut bindings);
        }
        menu.handle_key(0xff0d, &mut bindings);
        menu.handle_key(0xff0d, &mut bindings);
        assert_eq!(menu.handle_key(0x78, &mut bindings), Command::None);
        assert_eq!(menu.status, "KEY ALREADY USED");
        assert_eq!(
            menu.handle_key(0xffc5, &mut bindings),
            Command::SettingsChanged
        );
        assert_eq!(bindings.keys(Action::A), [0xffc5, 0]);
    }

    #[test]
    fn menu_overlay_is_host_only_and_has_the_guest_frame_size() {
        let pixels = vec![0x123456; 240 * 160];
        let visible = MenuState::default().render(&pixels, &KeyBindings::default());
        assert_eq!(visible.len(), pixels.len());
        assert_eq!(pixels[12 * 240 + 12], 0x123456);
        assert_eq!(visible[12 * 240 + 12], 0xffffff);
        assert_eq!(visible[0], pixels[0]);
    }
}
