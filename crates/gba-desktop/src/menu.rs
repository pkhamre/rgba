//! Host-only options and keybinding menus; guest pixels and state are untouched.

use crate::gamepad::{GamepadBindings, GamepadControl};
use crate::keybindings::{Action, BindingError, KeyBindings, key_name};

const OPTIONS: usize = 6;
const KEY_ROWS: usize = 7;
const KEY_RESET_INDEX: usize = Action::ALL.len();
const KEY_BACK_INDEX: usize = KEY_RESET_INDEX + 1;
const PAD_RESET_INDEX: usize = GamepadBindings::ACTIONS.len();
const PAD_BACK_INDEX: usize = PAD_RESET_INDEX + 1;

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
    ControllerBindings,
}

#[derive(Clone, Debug)]
pub(crate) struct MenuState {
    page: Page,
    selected: usize,
    capture: Option<Action>,
    controller_capture: Option<Action>,
    status: String,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            page: Page::Options,
            selected: 0,
            capture: None,
            controller_capture: None,
            status: String::new(),
        }
    }
}

impl MenuState {
    pub(crate) fn handle_key(
        &mut self,
        symbol: u32,
        bindings: &mut KeyBindings,
        gamepad_bindings: &mut GamepadBindings,
    ) -> Command {
        if self.controller_capture.is_some() {
            if symbol == 0xff1b {
                self.controller_capture = None;
                self.status = "PAD BIND CANCELLED".to_owned();
            }
            return Command::None;
        }
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

        self.handle_action(bindings.action_for(symbol), bindings, gamepad_bindings)
    }

    pub(crate) fn handle_gamepad_action(
        &mut self,
        action: Action,
        bindings: &mut KeyBindings,
        gamepad_bindings: &mut GamepadBindings,
    ) -> Command {
        if self.capture.is_some() || self.controller_capture.is_some() {
            return Command::None;
        }
        self.handle_action(Some(action), bindings, gamepad_bindings)
    }

    pub(crate) fn handle_gamepad_control(
        &mut self,
        control: GamepadControl,
        gamepad_bindings: &mut GamepadBindings,
    ) -> Command {
        let Some(action) = self.controller_capture else {
            return Command::None;
        };
        match gamepad_bindings.rebind(action, control) {
            Ok(()) => {
                self.controller_capture = None;
                self.status = "PAD BIND CHANGED".to_owned();
                Command::SettingsChanged
            }
            Err(BindingError::DuplicateKey) => {
                self.status = "PAD INPUT IN USE".to_owned();
                Command::None
            }
            Err(BindingError::InvalidKey) => {
                self.status = "PAD INPUT INVALID".to_owned();
                Command::None
            }
            Err(BindingError::InvalidFormat) => {
                self.status = "PAD SETTINGS INVALID".to_owned();
                Command::None
            }
        }
    }

    pub(crate) fn is_capturing_controller(&self) -> bool {
        self.controller_capture.is_some()
    }

    fn handle_action(
        &mut self,
        action: Option<Action>,
        bindings: &mut KeyBindings,
        gamepad_bindings: &mut GamepadBindings,
    ) -> Command {
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
                Page::ControllerBindings => {
                    self.page = Page::Options;
                    self.selected = 3;
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
                3 => {
                    self.page = Page::ControllerBindings;
                    self.selected = 0;
                    Command::None
                }
                4 => Command::BackToLibrary,
                _ => Command::Exit,
            },
            Page::Keybindings if self.selected < Action::ALL.len() => {
                let action = Action::ALL[self.selected];
                self.capture = Some(action);
                self.status = "PRESS KEY - ESC CANCELS".to_owned();
                Command::None
            }
            Page::Keybindings if self.selected == KEY_RESET_INDEX => {
                *bindings = KeyBindings::default();
                self.status = "DEFAULTS RESTORED".to_owned();
                Command::SettingsChanged
            }
            Page::Keybindings => {
                self.page = Page::Options;
                self.selected = 2;
                Command::None
            }
            Page::ControllerBindings if self.selected < GamepadBindings::ACTIONS.len() => {
                let action = GamepadBindings::ACTIONS[self.selected];
                self.controller_capture = Some(action);
                self.status = "PRESS PAD INPUT - ESC CANCELS".to_owned();
                Command::None
            }
            Page::ControllerBindings if self.selected == PAD_RESET_INDEX => {
                *gamepad_bindings = GamepadBindings::default();
                self.status = "PAD DEFAULTS RESTORED".to_owned();
                Command::SettingsChanged
            }
            Page::ControllerBindings => {
                self.page = Page::Options;
                self.selected = 3;
                Command::None
            }
        }
    }

    pub(crate) fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    pub(crate) fn render(
        &self,
        pixels: &[u32],
        bindings: &KeyBindings,
        gamepad_bindings: &GamepadBindings,
    ) -> Vec<u32> {
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
            Page::ControllerBindings => {
                self.draw_controller_bindings(&mut output, gamepad_bindings)
            }
        }
        if !self.status.is_empty() {
            draw_text(&mut output, 22, 132, &self.status, 0xffd060);
        }
        output
    }

    fn page_length(&self) -> usize {
        match self.page {
            Page::Options => OPTIONS,
            Page::Keybindings => KEY_BACK_INDEX + 1,
            Page::ControllerBindings => PAD_BACK_INDEX + 1,
        }
    }

    fn draw_options(&self, output: &mut [u32]) {
        draw_text(output, 82, 22, "OPTIONS", 0x80d8ff);
        let entries = [
            "SAVE STATE",
            "LOAD STATE",
            "KEYBINDS",
            "PAD KEYBINDS",
            "BACK TO LIBRARY",
            "EXIT",
        ];
        for (index, entry) in entries.iter().enumerate() {
            let y = 39 + index * 12;
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
            if index > KEY_BACK_INDEX {
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
            } else if index == KEY_RESET_INDEX {
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

    fn draw_controller_bindings(&self, output: &mut [u32], bindings: &GamepadBindings) {
        draw_text(output, 55, 17, "PAD KEYBINDS", 0x80d8ff);
        let start = self
            .selected
            .saturating_sub(KEY_ROWS - 1)
            .min(GamepadBindings::ACTIONS.len());
        for row in 0..KEY_ROWS {
            let index = start + row;
            if index > PAD_BACK_INDEX {
                break;
            }
            let (label, value) = if index < GamepadBindings::ACTIONS.len() {
                let action = GamepadBindings::ACTIONS[index];
                (action.label().to_owned(), bindings.display(action))
            } else if index == PAD_RESET_INDEX {
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
            draw_text(output, 120, y, &value, 0x80d8ff);
        }
        if self.controller_capture.is_some() {
            draw_text(output, 22, 124, "PRESS PAD INPUT - ESC CANCELS", 0xffd060);
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
        ',' => [0, 0, 0, 0, 0, 4, 8],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '(' => [2, 4, 8, 8, 8, 4, 2],
        ')' => [8, 4, 2, 2, 2, 4, 8],
        '/' => [1, 2, 2, 4, 8, 8, 16],
        '>' => [16, 8, 4, 2, 4, 8, 16],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, MenuState};
    use crate::gamepad::{GamepadBindings, GamepadControl};
    use crate::keybindings::{Action, KeyBindings};
    use evdev::KeyCode;

    fn key(
        menu: &mut MenuState,
        symbol: u32,
        keyboard: &mut KeyBindings,
        gamepad: &mut GamepadBindings,
    ) -> Command {
        menu.handle_key(symbol, keyboard, gamepad)
    }

    fn pad_action(
        menu: &mut MenuState,
        action: Action,
        keyboard: &mut KeyBindings,
        gamepad: &mut GamepadBindings,
    ) -> Command {
        menu.handle_gamepad_action(action, keyboard, gamepad)
    }

    #[test]
    fn menu_selects_save_load_and_binding_pages_then_resumes() {
        let mut menu = MenuState::default();
        let mut keyboard = KeyBindings::default();
        let mut gamepad = GamepadBindings::default();
        assert_eq!(
            key(&mut menu, 0xff0d, &mut keyboard, &mut gamepad),
            Command::SaveState
        );
        key(&mut menu, 0xff54, &mut keyboard, &mut gamepad);
        assert_eq!(
            key(&mut menu, 0xff0d, &mut keyboard, &mut gamepad),
            Command::LoadState
        );
        key(&mut menu, 0xff54, &mut keyboard, &mut gamepad);
        assert_eq!(
            key(&mut menu, 0xff0d, &mut keyboard, &mut gamepad),
            Command::None
        );
        key(&mut menu, 0xff1b, &mut keyboard, &mut gamepad);
        assert_eq!(
            key(&mut menu, 0xff1b, &mut keyboard, &mut gamepad),
            Command::Resume
        );
    }

    #[test]
    fn gamepad_actions_navigate_and_activate_the_options_menu() {
        let mut menu = MenuState::default();
        let mut keyboard = KeyBindings::default();
        let mut gamepad = GamepadBindings::default();
        assert_eq!(
            pad_action(&mut menu, Action::Down, &mut keyboard, &mut gamepad),
            Command::None
        );
        assert_eq!(
            pad_action(&mut menu, Action::A, &mut keyboard, &mut gamepad),
            Command::LoadState
        );
    }

    #[test]
    fn controller_binding_page_captures_a_pad_input_and_persists_it() {
        let mut menu = MenuState::default();
        let mut keyboard = KeyBindings::default();
        let mut gamepad = GamepadBindings::default();
        for _ in 0..3 {
            pad_action(&mut menu, Action::Down, &mut keyboard, &mut gamepad);
        }
        pad_action(&mut menu, Action::A, &mut keyboard, &mut gamepad);
        pad_action(&mut menu, Action::A, &mut keyboard, &mut gamepad);
        assert!(menu.is_capturing_controller());
        assert_eq!(
            menu.handle_gamepad_control(
                GamepadControl::Key(KeyCode::BTN_NORTH.code()),
                &mut gamepad,
            ),
            Command::SettingsChanged
        );
        assert_eq!(
            gamepad.controls(Action::A)[0],
            Some(GamepadControl::Key(KeyCode::BTN_NORTH.code()))
        );
    }

    #[test]
    fn explicit_exit_and_back_to_library_are_menu_actions() {
        let mut menu = MenuState::default();
        let mut keyboard = KeyBindings::default();
        let mut gamepad = GamepadBindings::default();
        for _ in 0..5 {
            key(&mut menu, 0xff54, &mut keyboard, &mut gamepad);
        }
        assert_eq!(
            key(&mut menu, 0xff0d, &mut keyboard, &mut gamepad),
            Command::Exit
        );
        let mut menu = MenuState::default();
        for _ in 0..4 {
            key(&mut menu, 0xff54, &mut keyboard, &mut gamepad);
        }
        assert_eq!(
            key(&mut menu, 0xff0d, &mut keyboard, &mut gamepad),
            Command::BackToLibrary
        );
    }

    #[test]
    fn keyboard_binding_capture_rejects_conflict_and_accepts_new_key() {
        let mut menu = MenuState::default();
        let mut keyboard = KeyBindings::default();
        let mut gamepad = GamepadBindings::default();
        for _ in 0..2 {
            key(&mut menu, 0xff54, &mut keyboard, &mut gamepad);
        }
        key(&mut menu, 0xff0d, &mut keyboard, &mut gamepad);
        key(&mut menu, 0xff0d, &mut keyboard, &mut gamepad);
        assert_eq!(
            key(&mut menu, 0x78, &mut keyboard, &mut gamepad),
            Command::None
        );
        assert_eq!(menu.status, "KEY ALREADY USED");
        assert_eq!(
            key(&mut menu, 0xffc5, &mut keyboard, &mut gamepad),
            Command::SettingsChanged
        );
        assert_eq!(keyboard.keys(Action::A), [0xffc5, 0]);
    }

    #[test]
    fn menu_overlay_is_host_only_and_has_the_guest_frame_size() {
        let pixels = vec![0x123456; 240 * 160];
        let visible = MenuState::default().render(
            &pixels,
            &KeyBindings::default(),
            &GamepadBindings::default(),
        );
        assert_eq!(visible.len(), pixels.len());
        assert_eq!(pixels[12 * 240 + 12], 0x123456);
        assert_eq!(visible[12 * 240 + 12], 0xffffff);
        assert_eq!(visible[0], pixels[0]);
    }
}
