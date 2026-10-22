//! Linux evdev gamepad input for the desktop frontend.

use crate::keybindings::Action;
use evdev::{AbsoluteAxisCode, Device, EventSummary, KeyCode};
use std::io;
use std::time::{Duration, Instant};

const MENU_COMBO: u16 = button_bit(Action::Start) | button_bit(Action::Select);
const STICK_DEADZONE: f32 = 0.35;
const RECONNECT_INTERVAL: Duration = Duration::from_secs(2);

pub(crate) struct GamepadInput {
    device: Option<Device>,
    name: String,
    next_discovery: Instant,
    reported_missing: bool,
    key_buttons: u16,
    hat_x: i32,
    hat_y: i32,
    stick_x: i32,
    stick_y: i32,
    x_range: (i32, i32),
    y_range: (i32, i32),
    previous_mask: u16,
    suppressed_buttons: u16,
}

pub(crate) struct GamepadPoll {
    pub(crate) buttons: u16,
    pub(crate) menu_pressed: bool,
    pub(crate) pressed_actions: Vec<Action>,
}

impl GamepadInput {
    pub(crate) fn new() -> Self {
        Self {
            device: None,
            name: String::new(),
            next_discovery: Instant::now(),
            reported_missing: false,
            key_buttons: 0,
            hat_x: 0,
            hat_y: 0,
            stick_x: 0,
            stick_y: 0,
            x_range: (-32768, 32767),
            y_range: (-32768, 32767),
            previous_mask: 0,
            suppressed_buttons: 0,
        }
    }

    pub(crate) fn poll(&mut self) -> GamepadPoll {
        if self.device.is_none() && Instant::now() >= self.next_discovery {
            self.discover();
        }

        let mut event_pressed = 0;
        let mut disconnected = None;
        if let Some(device) = &mut self.device {
            match device.fetch_events() {
                Ok(events) => {
                    for event in events {
                        match event.destructure() {
                            EventSummary::Key(_, key, value) => {
                                if let Some(action) = action_for_key(key) {
                                    let bit = button_bit(action);
                                    if value == 1 {
                                        self.key_buttons |= bit;
                                        event_pressed |= bit;
                                    } else if value == 0 {
                                        self.key_buttons &= !bit;
                                    }
                                }
                            }
                            EventSummary::AbsoluteAxis(_, axis, value) => match axis {
                                AbsoluteAxisCode::ABS_HAT0X => self.hat_x = value,
                                AbsoluteAxisCode::ABS_HAT0Y => self.hat_y = value,
                                AbsoluteAxisCode::ABS_X => self.stick_x = value,
                                AbsoluteAxisCode::ABS_Y => self.stick_y = value,
                                _ => {}
                            },
                            _ => {}
                        }
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => disconnected = Some(error),
            }
        }
        if let Some(error) = disconnected {
            eprintln!("Gamepad {} disconnected: {error}", self.name);
            self.device = None;
            self.clear_state();
            self.next_discovery = Instant::now() + RECONNECT_INTERVAL;
        }

        let mut raw_buttons = self.key_buttons | hat_buttons(self.hat_x, self.hat_y);
        raw_buttons |= axis_buttons(self.stick_x, self.x_range, Action::Left, Action::Right);
        raw_buttons |= axis_buttons(self.stick_y, self.y_range, Action::Up, Action::Down);
        let menu_pressed =
            raw_buttons & MENU_COMBO == MENU_COMBO && self.previous_mask & MENU_COMBO != MENU_COMBO;
        let pressed = event_pressed | (raw_buttons & !self.previous_mask);
        self.previous_mask = raw_buttons;
        self.suppressed_buttons &= raw_buttons;
        let mut buttons = raw_buttons & !self.suppressed_buttons;
        if raw_buttons & MENU_COMBO == MENU_COMBO {
            buttons &= !MENU_COMBO;
        }
        GamepadPoll {
            buttons,
            menu_pressed,
            pressed_actions: Action::ALL
                .into_iter()
                .filter(|action| pressed & button_bit(*action) != 0)
                .collect(),
        }
    }

    fn discover(&mut self) {
        self.next_discovery = Instant::now() + RECONNECT_INTERVAL;
        for (_path, device) in evdev::enumerate() {
            let is_gamepad = device.supported_keys().is_some_and(|keys| {
                keys.contains(KeyCode::BTN_SOUTH)
                    && keys.contains(KeyCode::BTN_EAST)
                    && keys.contains(KeyCode::BTN_START)
                    && keys.contains(KeyCode::BTN_SELECT)
            });
            if !is_gamepad || device.set_nonblocking(true).is_err() {
                continue;
            }
            let name = device.name().unwrap_or("USB gamepad").to_owned();
            let axes = device
                .get_absinfo()
                .map(|axes| axes.collect::<Vec<_>>())
                .unwrap_or_default();
            self.clear_state();
            self.x_range = axis_range(&axes, AbsoluteAxisCode::ABS_X);
            self.y_range = axis_range(&axes, AbsoluteAxisCode::ABS_Y);
            for (axis, info) in &axes {
                match *axis {
                    AbsoluteAxisCode::ABS_HAT0X => self.hat_x = info.value(),
                    AbsoluteAxisCode::ABS_HAT0Y => self.hat_y = info.value(),
                    AbsoluteAxisCode::ABS_X => self.stick_x = info.value(),
                    AbsoluteAxisCode::ABS_Y => self.stick_y = info.value(),
                    _ => {}
                }
            }
            self.key_buttons = device
                .get_key_state()
                .map(|keys| {
                    keys.iter()
                        .filter_map(action_for_key)
                        .fold(0, |mask, action| mask | button_bit(action))
                })
                .unwrap_or(0);
            eprintln!("Gamepad connected: {name}");
            self.name = name;
            self.device = Some(device);
            self.reported_missing = false;
            return;
        }
        if !self.reported_missing {
            eprintln!(
                "No readable standard gamepad found; check USB connection and /dev/input permissions"
            );
            self.reported_missing = true;
        }
    }

    pub(crate) fn suppress_held(&mut self) {
        self.suppressed_buttons |= self.key_buttons
            | hat_buttons(self.hat_x, self.hat_y)
            | axis_buttons(self.stick_x, self.x_range, Action::Left, Action::Right)
            | axis_buttons(self.stick_y, self.y_range, Action::Up, Action::Down);
    }

    fn clear_state(&mut self) {
        self.key_buttons = 0;
        self.hat_x = 0;
        self.hat_y = 0;
        self.stick_x = 0;
        self.stick_y = 0;
        self.previous_mask = 0;
        self.suppressed_buttons = 0;
    }
}

fn action_for_key(key: KeyCode) -> Option<Action> {
    match key {
        KeyCode::BTN_SOUTH => Some(Action::A),
        KeyCode::BTN_EAST => Some(Action::B),
        KeyCode::BTN_SELECT => Some(Action::Select),
        KeyCode::BTN_START => Some(Action::Start),
        KeyCode::BTN_DPAD_RIGHT => Some(Action::Right),
        KeyCode::BTN_DPAD_LEFT => Some(Action::Left),
        KeyCode::BTN_DPAD_UP => Some(Action::Up),
        KeyCode::BTN_DPAD_DOWN => Some(Action::Down),
        KeyCode::BTN_TR => Some(Action::R),
        KeyCode::BTN_TL => Some(Action::L),
        _ => None,
    }
}

const fn button_bit(action: Action) -> u16 {
    match action.bit() {
        Some(bit) => bit,
        None => 0,
    }
}

fn axis_range(axes: &[(AbsoluteAxisCode, evdev::AbsInfo)], axis: AbsoluteAxisCode) -> (i32, i32) {
    axes.iter()
        .find(|(code, _)| *code == axis)
        .map(|(_, info)| (info.minimum(), info.maximum()))
        .filter(|(minimum, maximum)| minimum < maximum)
        .unwrap_or((-32768, 32767))
}

fn axis_buttons(value: i32, range: (i32, i32), negative: Action, positive: Action) -> u16 {
    let (minimum, maximum) = range;
    let midpoint = (minimum as f32 + maximum as f32) * 0.5;
    let half_range = (maximum as f32 - minimum as f32) * 0.5;
    if half_range <= 0.0 {
        return 0;
    }
    let position = (value as f32 - midpoint) / half_range;
    if position < -STICK_DEADZONE {
        button_bit(negative)
    } else if position > STICK_DEADZONE {
        button_bit(positive)
    } else {
        0
    }
}

fn hat_buttons(x: i32, y: i32) -> u16 {
    let mut buttons = 0;
    if x < 0 {
        buttons |= button_bit(Action::Left);
    } else if x > 0 {
        buttons |= button_bit(Action::Right);
    }
    if y < 0 {
        buttons |= button_bit(Action::Up);
    } else if y > 0 {
        buttons |= button_bit(Action::Down);
    }
    buttons
}

#[cfg(test)]
mod tests {
    use super::{action_for_key, axis_buttons, button_bit, hat_buttons};
    use crate::keybindings::Action;
    use evdev::KeyCode;

    #[test]
    fn standard_gamepad_face_start_select_and_shoulder_buttons_map_to_gba() {
        assert_eq!(action_for_key(KeyCode::BTN_SOUTH), Some(Action::A));
        assert_eq!(action_for_key(KeyCode::BTN_EAST), Some(Action::B));
        assert_eq!(action_for_key(KeyCode::BTN_SELECT), Some(Action::Select));
        assert_eq!(action_for_key(KeyCode::BTN_START), Some(Action::Start));
        assert_eq!(action_for_key(KeyCode::BTN_TL), Some(Action::L));
        assert_eq!(action_for_key(KeyCode::BTN_TR), Some(Action::R));
    }

    #[test]
    fn desktop_actions_do_not_produce_gamepad_button_bits() {
        assert_eq!(button_bit(Action::Pause), 0);
        assert_eq!(button_bit(Action::Exit), 0);
    }

    #[test]
    fn hat_axes_and_left_stick_map_with_a_deadzone() {
        assert_eq!(
            hat_buttons(-1, 1),
            button_bit(Action::Left) | button_bit(Action::Down)
        );
        assert_eq!(
            axis_buttons(-20_000, (-32_768, 32_767), Action::Left, Action::Right),
            button_bit(Action::Left)
        );
        assert_eq!(
            axis_buttons(20_000, (-32_768, 32_767), Action::Left, Action::Right),
            button_bit(Action::Right)
        );
        assert_eq!(
            axis_buttons(5_000, (-32_768, 32_767), Action::Left, Action::Right),
            0
        );
    }
}
