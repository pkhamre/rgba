//! Linux evdev gamepad input and per-game controller bindings.

use crate::keybindings::{Action, BindingError, KeyBindings};
use crate::speed::Speed;
use evdev::{AbsoluteAxisCode, Device, EventSummary, KeyCode};
use std::collections::HashSet;
use std::io;
use std::time::{Duration, Instant};

const ACTIONS: [Action; 10] = [
    Action::A,
    Action::B,
    Action::Select,
    Action::Start,
    Action::Right,
    Action::Left,
    Action::Up,
    Action::Down,
    Action::R,
    Action::L,
];
const CONTROL_COUNT: usize = 3;
const AXIS_COUNT: usize = 64;
const MENU_COMBO: u16 = bit(Action::Start) | bit(Action::Select);
const STICK_DEADZONE: f32 = 0.35;
const RECONNECT_INTERVAL: Duration = Duration::from_secs(2);
const SETTINGS_MARKER: &str = "gba-gamepad-bindings-v1\n";
const SPEED_MARKER: &str = "gba-speed-v1\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum GamepadControl {
    Key(u16),
    AxisNegative(u16),
    AxisPositive(u16),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GamepadBindings {
    controls: [[Option<GamepadControl>; CONTROL_COUNT]; ACTIONS.len()],
}

impl GamepadBindings {
    pub(crate) const ACTIONS: [Action; 10] = ACTIONS;

    pub(crate) fn controls(&self, action: Action) -> &[Option<GamepadControl>; CONTROL_COUNT] {
        &self.controls[action_index(action).expect("only GBA actions have controller bindings")]
    }

    pub(crate) fn rebind(
        &mut self,
        action: Action,
        control: GamepadControl,
    ) -> Result<(), BindingError> {
        let index = action_index(action).ok_or(BindingError::InvalidKey)?;
        let invalid_control = match control {
            GamepadControl::Key(code) => code == 0 || code > 0x02ff,
            GamepadControl::AxisNegative(code) | GamepadControl::AxisPositive(code) => {
                code >= AXIS_COUNT as u16
            }
        };
        if invalid_control {
            return Err(BindingError::InvalidKey);
        }
        if ACTIONS
            .into_iter()
            .filter(|other| *other != action)
            .any(|other| self.controls(other).contains(&Some(control)))
        {
            return Err(BindingError::DuplicateKey);
        }
        self.controls[index] = [Some(control), None, None];
        Ok(())
    }

    fn action_for(&self, control: GamepadControl) -> Option<Action> {
        ACTIONS
            .into_iter()
            .find(|action| self.controls(*action).contains(&Some(control)))
    }

    fn mask(
        &self,
        keys: &HashSet<u16>,
        axes: &[i32; AXIS_COUNT],
        ranges: &[Option<(i32, i32)>; AXIS_COUNT],
    ) -> u16 {
        ACTIONS.into_iter().fold(0, |mask, action| {
            let active = self
                .controls(action)
                .iter()
                .flatten()
                .any(|control| match *control {
                    GamepadControl::Key(code) => keys.contains(&code),
                    GamepadControl::AxisNegative(code) => {
                        axis_position(code, axes, ranges) < -STICK_DEADZONE
                    }
                    GamepadControl::AxisPositive(code) => {
                        axis_position(code, axes, ranges) > STICK_DEADZONE
                    }
                });
            if active { mask | bit(action) } else { mask }
        })
    }

    pub(crate) fn display(&self, action: Action) -> String {
        self.controls(action)
            .iter()
            .flatten()
            .map(|control| control_name(*control))
            .collect::<Vec<_>>()
            .join("/")
    }

    pub(crate) fn encode(&self) -> String {
        let mut output = String::from(SETTINGS_MARKER);
        for action in ACTIONS {
            output.push_str(action.label());
            output.push('=');
            let mut first = true;
            for control in self.controls(action).iter().flatten() {
                if !first {
                    output.push(',');
                }
                first = false;
                output.push_str(&encode_control(*control));
            }
            output.push('\n');
        }
        output
    }

    pub(crate) fn decode(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        if lines.next() != Some(SETTINGS_MARKER.trim_end()) {
            return Err("invalid controller settings header".to_owned());
        }
        let mut bindings = Self::default();
        let mut seen = [false; ACTIONS.len()];
        let mut assigned = Vec::new();
        for line in lines {
            let (name, value) = line
                .split_once('=')
                .ok_or_else(|| "invalid controller binding".to_owned())?;
            let index = ACTIONS
                .iter()
                .position(|action| action.label() == name)
                .ok_or_else(|| "unknown controller action".to_owned())?;
            if seen[index] {
                return Err("duplicate controller action".to_owned());
            }
            let mut controls = [None; CONTROL_COUNT];
            let mut count = 0;
            for token in value.split(',') {
                if count == CONTROL_COUNT {
                    return Err("too many controller inputs for an action".to_owned());
                }
                let control = decode_control(token)?;
                if assigned.contains(&control) {
                    return Err("duplicate controller input".to_owned());
                }
                assigned.push(control);
                controls[count] = Some(control);
                count += 1;
            }
            if count == 0 {
                return Err("controller action has no input".to_owned());
            }
            bindings.controls[index] = controls;
            seen[index] = true;
        }
        if seen.contains(&false) {
            return Err("missing controller action".to_owned());
        }
        Ok(bindings)
    }
}

impl Default for GamepadBindings {
    fn default() -> Self {
        let key = |code: KeyCode| Some(GamepadControl::Key(code.code()));
        let negative = |axis: AbsoluteAxisCode| Some(GamepadControl::AxisNegative(axis.0));
        let positive = |axis: AbsoluteAxisCode| Some(GamepadControl::AxisPositive(axis.0));
        Self {
            controls: [
                [key(KeyCode::BTN_SOUTH), None, None],
                [key(KeyCode::BTN_EAST), None, None],
                [key(KeyCode::BTN_SELECT), None, None],
                [key(KeyCode::BTN_START), None, None],
                [
                    key(KeyCode::BTN_DPAD_RIGHT),
                    positive(AbsoluteAxisCode::ABS_X),
                    positive(AbsoluteAxisCode::ABS_HAT0X),
                ],
                [
                    key(KeyCode::BTN_DPAD_LEFT),
                    negative(AbsoluteAxisCode::ABS_X),
                    negative(AbsoluteAxisCode::ABS_HAT0X),
                ],
                [
                    key(KeyCode::BTN_DPAD_UP),
                    negative(AbsoluteAxisCode::ABS_Y),
                    negative(AbsoluteAxisCode::ABS_HAT0Y),
                ],
                [
                    key(KeyCode::BTN_DPAD_DOWN),
                    positive(AbsoluteAxisCode::ABS_Y),
                    positive(AbsoluteAxisCode::ABS_HAT0Y),
                ],
                [key(KeyCode::BTN_TR), None, None],
                [key(KeyCode::BTN_TL), None, None],
            ],
        }
    }
}

pub(crate) fn encode_settings(
    keyboard: &KeyBindings,
    gamepad: &GamepadBindings,
    speed: Speed,
) -> String {
    format!(
        "{}{}{}{}\n",
        keyboard.encode(),
        gamepad.encode(),
        SPEED_MARKER,
        speed.multiplier()
    )
}

pub(crate) fn decode_settings(text: &str) -> Result<(KeyBindings, GamepadBindings, Speed), String> {
    let (bindings_text, speed) = match text.split_once(SPEED_MARKER) {
        Some((bindings, encoded_speed)) => {
            if bindings.contains(SPEED_MARKER) {
                return Err("duplicate speed setting".to_owned());
            }
            let value = encoded_speed.strip_suffix('\n').unwrap_or(encoded_speed);
            if value.contains('\n') {
                return Err("invalid speed setting".to_owned());
            }
            (bindings, Speed::parse(value)?)
        }
        None => (text, Speed::default()),
    };
    let (keyboard, gamepad) = if let Some((keyboard, gamepad)) =
        bindings_text.split_once(SETTINGS_MARKER)
    {
        let keyboard = KeyBindings::decode(keyboard).map_err(|error| format!("{error:?}"))?;
        let gamepad = GamepadBindings::decode(&format!("{SETTINGS_MARKER}{gamepad}"))?;
        (keyboard, gamepad)
    } else {
        let keyboard = KeyBindings::decode(bindings_text).map_err(|error| format!("{error:?}"))?;
        (keyboard, GamepadBindings::default())
    };
    Ok((keyboard, gamepad, speed))
}

pub(crate) struct GamepadInput {
    device: Option<Device>,
    name: String,
    next_discovery: Instant,
    reported_missing: bool,
    pressed_keys: HashSet<u16>,
    axes: [i32; AXIS_COUNT],
    axis_ranges: [Option<(i32, i32)>; AXIS_COUNT],
    previous_mask: u16,
    suppressed_buttons: u16,
}

pub(crate) struct GamepadPoll {
    pub(crate) buttons: u16,
    pub(crate) menu_pressed: bool,
    pub(crate) pressed_actions: Vec<Action>,
    pub(crate) pressed_controls: Vec<GamepadControl>,
}

impl GamepadInput {
    pub(crate) fn new() -> Self {
        Self {
            device: None,
            name: String::new(),
            next_discovery: Instant::now(),
            reported_missing: false,
            pressed_keys: HashSet::new(),
            axes: [0; AXIS_COUNT],
            axis_ranges: [None; AXIS_COUNT],
            previous_mask: 0,
            suppressed_buttons: 0,
        }
    }

    pub(crate) fn poll(&mut self, bindings: &GamepadBindings) -> GamepadPoll {
        if self.device.is_none() && Instant::now() >= self.next_discovery {
            self.discover();
        }

        let mut event_pressed = 0;
        let mut pressed_controls = Vec::new();
        let mut disconnected = None;
        if let Some(device) = &mut self.device {
            match device.fetch_events() {
                Ok(events) => {
                    for event in events {
                        match event.destructure() {
                            EventSummary::Key(_, key, value) => {
                                let code = key.code();
                                let control = GamepadControl::Key(code);
                                if value == 1 {
                                    self.pressed_keys.insert(code);
                                    push_unique(&mut pressed_controls, control);
                                    if let Some(action) = bindings.action_for(control) {
                                        event_pressed |= bit(action);
                                    }
                                } else if value == 0 {
                                    self.pressed_keys.remove(&code);
                                }
                            }
                            EventSummary::AbsoluteAxis(_, axis, value) => {
                                let code = axis.0;
                                let index = usize::from(code);
                                if index >= AXIS_COUNT {
                                    continue;
                                }
                                self.axes[index] = value;
                                for control in [
                                    GamepadControl::AxisNegative(code),
                                    GamepadControl::AxisPositive(code),
                                ] {
                                    if control_active(control, &self.axes, &self.axis_ranges) {
                                        push_unique(&mut pressed_controls, control);
                                        if let Some(action) = bindings.action_for(control) {
                                            event_pressed |= bit(action);
                                        }
                                    }
                                }
                            }
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

        let raw_buttons = bindings.mask(&self.pressed_keys, &self.axes, &self.axis_ranges);
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
            pressed_actions: ACTIONS
                .into_iter()
                .filter(|action| pressed & bit(*action) != 0)
                .collect(),
            pressed_controls,
        }
    }

    pub(crate) fn suppress_held(&mut self, bindings: &GamepadBindings) {
        let current = bindings.mask(&self.pressed_keys, &self.axes, &self.axis_ranges);
        self.suppressed_buttons |= current;
        self.previous_mask = current;
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
            self.clear_state();
            if let Ok(axes) = device.get_absinfo() {
                for (axis, info) in axes {
                    let index = usize::from(axis.0);
                    if index < AXIS_COUNT {
                        self.axes[index] = info.value();
                        self.axis_ranges[index] = Some((info.minimum(), info.maximum()));
                    }
                }
            }
            if let Ok(keys) = device.get_key_state() {
                self.pressed_keys = keys.iter().map(KeyCode::code).collect();
            }
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

    fn clear_state(&mut self) {
        self.pressed_keys.clear();
        self.axes = [0; AXIS_COUNT];
        self.axis_ranges = [None; AXIS_COUNT];
        self.previous_mask = 0;
        self.suppressed_buttons = 0;
    }
}

fn action_index(action: Action) -> Option<usize> {
    ACTIONS.iter().position(|candidate| *candidate == action)
}

const fn bit(action: Action) -> u16 {
    match action.bit() {
        Some(bit) => bit,
        None => 0,
    }
}

fn axis_position(
    code: u16,
    axes: &[i32; AXIS_COUNT],
    ranges: &[Option<(i32, i32)>; AXIS_COUNT],
) -> f32 {
    let index = usize::from(code);
    if index >= AXIS_COUNT {
        return 0.0;
    }
    let (minimum, maximum) = ranges[index].unwrap_or_else(|| {
        if (0x10..=0x17).contains(&code) {
            (-1, 1)
        } else {
            (-32768, 32767)
        }
    });
    let half_range = (maximum as f32 - minimum as f32) * 0.5;
    if half_range <= 0.0 {
        return 0.0;
    }
    (axes[index] as f32 - (minimum as f32 + maximum as f32) * 0.5) / half_range
}

fn control_active(
    control: GamepadControl,
    axes: &[i32; AXIS_COUNT],
    ranges: &[Option<(i32, i32)>; AXIS_COUNT],
) -> bool {
    match control {
        GamepadControl::Key(_) => false,
        GamepadControl::AxisNegative(code) => axis_position(code, axes, ranges) < -STICK_DEADZONE,
        GamepadControl::AxisPositive(code) => axis_position(code, axes, ranges) > STICK_DEADZONE,
    }
}

fn push_unique(controls: &mut Vec<GamepadControl>, control: GamepadControl) {
    if !controls.contains(&control) {
        controls.push(control);
    }
}

fn encode_control(control: GamepadControl) -> String {
    match control {
        GamepadControl::Key(code) => format!("K{code:04X}"),
        GamepadControl::AxisNegative(code) => format!("A{code:04X}-"),
        GamepadControl::AxisPositive(code) => format!("A{code:04X}+"),
    }
}

fn decode_control(text: &str) -> Result<GamepadControl, String> {
    let bytes = text.as_bytes();
    match bytes {
        [b'K', digits @ ..] if digits.len() == 4 => {
            let code = u16::from_str_radix(std::str::from_utf8(digits).unwrap_or(""), 16)
                .map_err(|_| "invalid controller key code".to_owned())?;
            if code == 0 || code > 0x02ff {
                return Err("controller key code is out of range".to_owned());
            }
            Ok(GamepadControl::Key(code))
        }
        [b'A', digits @ .., direction] if digits.len() == 4 => {
            let code = u16::from_str_radix(std::str::from_utf8(digits).unwrap_or(""), 16)
                .map_err(|_| "invalid controller axis code".to_owned())?;
            if code >= AXIS_COUNT as u16 {
                return Err("controller axis code is out of range".to_owned());
            }
            match direction {
                b'-' => Ok(GamepadControl::AxisNegative(code)),
                b'+' => Ok(GamepadControl::AxisPositive(code)),
                _ => Err("invalid controller axis direction".to_owned()),
            }
        }
        _ => Err("invalid controller input".to_owned()),
    }
}

fn control_name(control: GamepadControl) -> String {
    match control {
        GamepadControl::Key(code) => match code {
            0x130 => "SOUTH".to_owned(),
            0x131 => "EAST".to_owned(),
            0x132 => "C".to_owned(),
            0x133 => "NORTH".to_owned(),
            0x134 => "WEST".to_owned(),
            0x136 => "TL".to_owned(),
            0x137 => "TR".to_owned(),
            0x13a => "SELECT".to_owned(),
            0x13b => "START".to_owned(),
            0x220 => "DUP".to_owned(),
            0x221 => "DDOWN".to_owned(),
            0x222 => "DLEFT".to_owned(),
            0x223 => "DRIGHT".to_owned(),
            _ => format!("K{code:03X}"),
        },
        GamepadControl::AxisNegative(code) | GamepadControl::AxisPositive(code) => {
            let direction = if matches!(control, GamepadControl::AxisNegative(_)) {
                '-'
            } else {
                '+'
            };
            let axis = match code {
                0x00 => "LX",
                0x01 => "LY",
                0x10 => "HX",
                0x11 => "HY",
                _ => return format!("A{code:02X}{direction}"),
            };
            format!("{axis}{direction}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GamepadBindings, GamepadControl, GamepadInput, action_index, bit, control_active,
        decode_settings, encode_settings,
    };
    use crate::keybindings::{Action, KeyBindings};
    use crate::speed::Speed;
    use evdev::{AbsoluteAxisCode, KeyCode};

    #[test]
    fn defaults_map_standard_controls_and_support_stick_and_hat_movement() {
        let bindings = GamepadBindings::default();
        assert!(
            bindings
                .controls(Action::A)
                .contains(&Some(GamepadControl::Key(KeyCode::BTN_SOUTH.code())))
        );
        assert!(
            bindings
                .controls(Action::Right)
                .contains(&Some(GamepadControl::AxisPositive(
                    AbsoluteAxisCode::ABS_X.0
                )))
        );
        assert!(
            bindings
                .controls(Action::Up)
                .contains(&Some(GamepadControl::AxisNegative(
                    AbsoluteAxisCode::ABS_HAT0Y.0
                )))
        );
    }

    #[test]
    fn controller_settings_round_trip_and_legacy_keyboard_settings_load_defaults() {
        let keyboard = KeyBindings::default();
        let gamepad = GamepadBindings::default();
        let speed = Speed::Four;
        let encoded = encode_settings(&keyboard, &gamepad, speed);
        assert_eq!(
            decode_settings(&encoded).expect("encoded settings are valid"),
            (keyboard.clone(), gamepad, speed)
        );
        assert_eq!(
            decode_settings(&keyboard.encode()).expect("legacy keyboard settings are valid"),
            (keyboard.clone(), GamepadBindings::default(), Speed::One)
        );
        let previous_format = format!(
            "{}{}",
            keyboard.encode(),
            GamepadBindings::default().encode()
        );
        assert_eq!(
            decode_settings(&previous_format).expect("legacy controller settings are valid"),
            (keyboard.clone(), GamepadBindings::default(), Speed::One)
        );
        assert_eq!(
            decode_settings(&encode_settings(
                &keyboard,
                &GamepadBindings::default(),
                Speed::Sixteen
            ))
            .expect("speed settings are valid")
            .2,
            Speed::Sixteen
        );
        let invalid = format!("{}gba-speed-v1\n3\n", keyboard.encode());
        assert!(decode_settings(&invalid).is_err());
    }

    #[test]
    fn held_library_input_is_suppressed_using_the_game_bindings() {
        let mut input = GamepadInput::new();
        input.pressed_keys.insert(KeyCode::BTN_SOUTH.code());
        let mut bindings = GamepadBindings::default();
        bindings
            .rebind(Action::A, GamepadControl::Key(KeyCode::BTN_NORTH.code()))
            .expect("north button is free");
        bindings
            .rebind(Action::B, GamepadControl::Key(KeyCode::BTN_SOUTH.code()))
            .expect("south button is now free");
        input.suppress_held(&bindings);
        assert_eq!(input.suppressed_buttons, bit(Action::B));
    }

    #[test]
    fn rebinding_rejects_duplicate_controls_and_axis_deadzone_is_applied() {
        let mut bindings = GamepadBindings::default();
        assert!(
            bindings
                .rebind(Action::B, GamepadControl::Key(KeyCode::BTN_SOUTH.code()))
                .is_err()
        );
        assert!(bindings.rebind(Action::A, GamepadControl::Key(0)).is_err());
        let mut axes = [0; 64];
        let mut ranges = [None; 64];
        axes[usize::from(AbsoluteAxisCode::ABS_X.0)] = 20_000;
        ranges[usize::from(AbsoluteAxisCode::ABS_X.0)] = Some((-32768, 32767));
        assert!(control_active(
            GamepadControl::AxisPositive(AbsoluteAxisCode::ABS_X.0),
            &axes,
            &ranges
        ));
        assert_eq!(action_index(Action::Pause), None);
        assert_ne!(bit(Action::A), 0);
    }
}
