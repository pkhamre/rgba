//! X11 game-selection window shown when the library has multiple games.

use crate::arguments::Arguments;
use crate::game_library::Game;
use crate::gamepad::{GamepadBindings, GamepadInput};
use crate::keybindings::Action;
use crate::menu::draw_text;
use crate::presentation::Presentation;
use std::error::Error;
use std::io;
use std::process;
use std::thread;
use std::time::Duration;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt, CreateGCAux, CreateWindowAux, EventMask, ImageOrder, PropMode,
    VisualClass, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperConnectionExt;

pub(crate) fn choose(
    args: &Arguments,
    games: &[Game],
    gamepad: &mut GamepadInput,
) -> Result<Option<Game>, Box<dyn Error>> {
    let (connection, screen_number) = RustConnection::connect(None).map_err(|error| {
        io::Error::other(format!("cannot open X11 display for game library: {error}"))
    })?;
    let setup = connection.setup();
    let screen = &setup.roots[screen_number];
    let depth = screen.root_depth;
    let format = setup
        .pixmap_formats
        .iter()
        .find(|format| format.depth == depth)
        .ok_or_else(|| io::Error::other("X11 root image format is missing"))?;
    if !matches!(format.bits_per_pixel, 24 | 32) {
        return Err(io::Error::other("game library requires a 24/32-bit X11 display").into());
    }
    let visual = screen
        .allowed_depths
        .iter()
        .flat_map(|depth| &depth.visuals)
        .find(|visual| visual.visual_id == screen.root_visual)
        .ok_or_else(|| io::Error::other("X11 root visual is missing"))?;
    if visual.class != VisualClass::TRUE_COLOR {
        return Err(io::Error::other("game library requires an X11 TrueColor visual").into());
    }
    let window = connection.generate_id()?;
    let gc = connection.generate_id()?;
    let width = 240 * args.scale;
    let height = 160 * args.scale;
    connection
        .create_window(
            depth,
            window,
            screen.root,
            0,
            0,
            width,
            height,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &CreateWindowAux::new().background_pixel(0).event_mask(
                EventMask::EXPOSURE | EventMask::KEY_PRESS | EventMask::STRUCTURE_NOTIFY,
            ),
        )?
        .check()?;
    connection
        .create_gc(gc, window, &CreateGCAux::new())?
        .check()?;
    let mut presentation = Presentation::create(
        &connection,
        window,
        gc,
        depth,
        screen.root_visual,
        args.scale,
    )?;
    if !presentation.resizable() {
        let mut hints = [0; 18];
        hints[0] = (1 << 4) | (1 << 5);
        hints[5] = u32::from(width);
        hints[6] = u32::from(height);
        hints[7] = u32::from(width);
        hints[8] = u32::from(height);
        connection
            .change_property32(
                PropMode::REPLACE,
                window,
                AtomEnum::WM_NORMAL_HINTS,
                AtomEnum::WM_SIZE_HINTS,
                &hints,
            )?
            .check()?;
    }
    let title_atom = AtomEnum::WM_NAME;
    connection
        .change_property8(
            PropMode::REPLACE,
            window,
            title_atom,
            AtomEnum::STRING,
            b"Rust GBA - Game Library",
        )?
        .check()?;
    let pid_atom = connection.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom;
    connection
        .change_property32(
            PropMode::REPLACE,
            window,
            pid_atom,
            AtomEnum::CARDINAL,
            &[process::id()],
        )?
        .check()?;
    let protocols = connection
        .intern_atom(false, b"WM_PROTOCOLS")?
        .reply()?
        .atom;
    let delete = connection
        .intern_atom(false, b"WM_DELETE_WINDOW")?
        .reply()?
        .atom;
    connection
        .change_property32(
            PropMode::REPLACE,
            window,
            protocols,
            AtomEnum::ATOM,
            &[delete],
        )?
        .check()?;
    let mapping = connection
        .get_keyboard_mapping(setup.min_keycode, setup.max_keycode - setup.min_keycode + 1)?
        .reply()?;
    let minimum_keycode = setup.min_keycode;
    let columns = usize::from(mapping.keysyms_per_keycode);
    let byte_order = setup.image_byte_order == ImageOrder::LSB_FIRST;
    connection.map_window(window)?.check()?;
    connection.flush()?;

    let mut selected = 0usize;
    let mut result = None;
    let mut running = true;
    let gamepad_bindings = GamepadBindings::default();
    gamepad.suppress_held(&gamepad_bindings);
    while running {
        let gamepad_poll = gamepad.poll(&gamepad_bindings);
        for action in gamepad_poll.pressed_actions {
            match selection_action(action, selected, games.len()) {
                Some(SelectionAction::Move(index)) => selected = index,
                Some(SelectionAction::Select(index)) => {
                    result = games.get(index).cloned();
                    running = false;
                }
                Some(SelectionAction::Cancel) => running = false,
                None => {}
            }
        }
        if !running {
            break;
        }
        draw_library(
            &connection,
            gc,
            &presentation,
            depth,
            format.bits_per_pixel,
            format.scanline_pad,
            byte_order,
            [visual.red_mask, visual.green_mask, visual.blue_mask],
            games,
            selected,
        )?;
        connection.flush()?;
        let mut received = false;
        while let Some(event) = connection.poll_for_event()? {
            received = true;
            match event {
                Event::ClientMessage(message)
                    if message.type_ == protocols && message.data.as_data32()[0] == delete =>
                {
                    running = false;
                }
                Event::DestroyNotify(_) => running = false,
                Event::ConfigureNotify(event) if event.window == window => {
                    presentation.resize(&connection, event.width, event.height)?;
                }
                Event::KeyPress(event) => {
                    let symbol = key_symbol(
                        usize::from(event.detail),
                        minimum_keycode,
                        columns,
                        &mapping.keysyms,
                    );
                    match symbol {
                        0xff52 if !games.is_empty() => {
                            selected = (selected + games.len() - 1) % games.len();
                        }
                        0xff54 if !games.is_empty() => selected = (selected + 1) % games.len(),
                        0xff0d if !games.is_empty() => {
                            result = games.get(selected).cloned();
                            running = false;
                        }
                        0xff1b | 0xff08 => running = false,
                        _ => {}
                    }
                }
                Event::Error(error) => {
                    return Err(io::Error::other(format!("X11 protocol error: {error:?}")).into());
                }
                _ => {}
            }
        }
        if !received && running {
            thread::sleep(Duration::from_millis(16));
        }
    }
    presentation.close(&connection)?;
    connection.free_gc(gc)?;
    connection.destroy_window(window)?;
    connection.flush()?;
    if result.is_some() {
        gamepad.suppress_held(&gamepad_bindings);
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn draw_library(
    connection: &RustConnection,
    gc: u32,
    presentation: &Presentation,
    depth: u8,
    bits_per_pixel: u8,
    scanline_pad: u8,
    little_endian: bool,
    masks: [u32; 3],
    games: &[Game],
    selected: usize,
) -> Result<(), Box<dyn Error>> {
    let pixels = library_pixels(games, selected);
    let bytes = usize::from(bits_per_pixel) / 8;
    let pad = usize::from(scanline_pad) / 8;
    let stride = (240 * usize::from(presentation.upload_scale) * bytes).div_ceil(pad) * pad;
    let image = super::desktop::pack_pixels(
        &pixels,
        presentation.upload_scale,
        bytes,
        stride,
        little_endian,
        masks,
    );
    let rows = 160 * usize::from(presentation.upload_scale);
    let width = 240 * presentation.upload_scale;
    let connection_bytes = connection.maximum_request_bytes();
    let rows_per_request = ((connection_bytes - 64) / stride).max(1);
    for row in (0..rows).step_by(rows_per_request) {
        let count = rows_per_request.min(rows - row);
        connection.put_image(
            x11rb::protocol::xproto::ImageFormat::Z_PIXMAP,
            presentation.pixmap,
            gc,
            width,
            count as u16,
            0,
            row as i16,
            0,
            depth,
            &image[row * stride..(row + count) * stride],
        )?;
    }
    presentation.present(connection)?;
    Ok(())
}

fn library_pixels(games: &[Game], selected: usize) -> Vec<u32> {
    let mut pixels = vec![0x101820; 240 * 160];
    draw_text(&mut pixels, 72, 12, "GAME LIBRARY", 0x80d8ff);
    for x in 10..230 {
        pixels[30 * 240 + x] = 0xffffff;
    }
    if games.is_empty() {
        draw_text(&mut pixels, 58, 52, "NO GAMES FOUND", 0xffd060);
        draw_text(&mut pixels, 22, 76, "COPY ROMS TO GAMES FOLDER", 0xffffff);
        draw_text(&mut pixels, 42, 92, "RESTART TO RESCAN", 0x90a0b0);
        draw_text(&mut pixels, 66, 128, "B OR ESC TO EXIT", 0x90a0b0);
        return pixels;
    }
    const VISIBLE_GAMES: usize = 5;
    let start = selected.saturating_sub(VISIBLE_GAMES - 1);
    for (row, game) in games.iter().enumerate().skip(start).take(VISIBLE_GAMES) {
        let y = 34 + (row - start) * 21;
        let color = if row == selected { 0xffd060 } else { 0xffffff };
        draw_text(
            &mut pixels,
            12,
            y,
            if row == selected { ">" } else { " " },
            color,
        );
        let (first_line, second_line) = title_lines(&game.title);
        draw_text(&mut pixels, 25, y, &first_line, color);
        draw_text(&mut pixels, 25, y + 9, &second_line, color);
    }
    draw_text(&mut pixels, 28, 136, "UP DOWN MOVE  A LOAD", 0x90a0b0);
    draw_text(&mut pixels, 72, 148, "B OR ESC BACK", 0x90a0b0);
    pixels
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionAction {
    Move(usize),
    Select(usize),
    Cancel,
}

fn selection_action(action: Action, selected: usize, game_count: usize) -> Option<SelectionAction> {
    match action {
        Action::Up if game_count > 0 => Some(SelectionAction::Move(
            (selected + game_count - 1) % game_count,
        )),
        Action::Down if game_count > 0 => Some(SelectionAction::Move((selected + 1) % game_count)),
        Action::A | Action::Start if game_count > 0 => Some(SelectionAction::Select(selected)),
        Action::B => Some(SelectionAction::Cancel),
        _ => None,
    }
}

fn title_lines(title: &str) -> (String, String) {
    const LINE_CHARACTERS: usize = 32;
    const MAX_CHARACTERS: usize = LINE_CHARACTERS * 2;
    let mut characters: Vec<char> = title.to_ascii_uppercase().chars().collect();
    if characters.len() > MAX_CHARACTERS {
        characters.truncate(MAX_CHARACTERS - 3);
        characters.extend(['.', '.', '.']);
    }
    if characters.len() <= LINE_CHARACTERS {
        return (characters.into_iter().collect(), String::new());
    }
    let preferred_split = LINE_CHARACTERS * 7 / 8;
    let split = characters[..preferred_split]
        .iter()
        .rposition(|character| *character == ' ')
        .or_else(|| {
            characters[..LINE_CHARACTERS]
                .iter()
                .rposition(|character| *character == ' ')
        })
        .unwrap_or(LINE_CHARACTERS);
    let next = if characters.get(split) == Some(&' ') {
        split + 1
    } else {
        split
    };
    let second = characters.split_off(next);
    let first = characters[..split].iter().collect();
    (first, second.into_iter().collect())
}

fn key_symbol(code: usize, minimum: u8, columns: usize, symbols: &[u32]) -> u32 {
    code.checked_sub(usize::from(minimum))
        .and_then(|index| symbols.get(index * columns))
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{SelectionAction, selection_action};
    use crate::game_library::Game;
    use crate::keybindings::Action;

    #[test]
    fn empty_library_screen_explains_how_to_populate_it() {
        let pixels = super::library_pixels(&[], 0);
        assert_eq!(pixels.len(), 240 * 160);
        assert_ne!(pixels, vec![0x101820; 240 * 160]);
    }

    #[test]
    fn long_filenames_wrap_and_extreme_names_get_an_ellipsis() {
        assert_eq!(
            super::title_lines("Pokemon - Emerald Version (USA, Europe)"),
            (
                "POKEMON - EMERALD VERSION".to_owned(),
                "(USA, EUROPE)".to_owned()
            )
        );
        let (first, second) = super::title_lines(&"A".repeat(80));
        assert_eq!(first.len(), 32);
        assert_eq!(second.len(), 32);
        assert!(second.ends_with("..."));
    }

    #[test]
    fn gamepad_actions_navigate_launch_and_cancel_the_library() {
        assert_eq!(
            selection_action(Action::Up, 0, 2),
            Some(SelectionAction::Move(1))
        );
        assert_eq!(
            selection_action(Action::Down, 1, 2),
            Some(SelectionAction::Move(0))
        );
        assert_eq!(
            selection_action(Action::A, 1, 2),
            Some(SelectionAction::Select(1))
        );
        assert_eq!(
            selection_action(Action::B, 0, 0),
            Some(SelectionAction::Cancel)
        );
        assert_eq!(selection_action(Action::A, 0, 0), None);
    }

    #[test]
    fn selected_game_row_is_rendered_differently() {
        let games = [
            Game {
                path: "one.gba".into(),
                identity: [1; 32],
                title: "ONE".to_owned(),
            },
            Game {
                path: "two.gba".into(),
                identity: [2; 32],
                title: "TWO".to_owned(),
            },
        ];
        assert_ne!(
            super::library_pixels(&games, 0),
            super::library_pixels(&games, 1)
        );
    }
}
