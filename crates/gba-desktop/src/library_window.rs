//! X11 game library and built-in ROM file browser.

use crate::arguments::Arguments;
use crate::game_library::{self, Game, StoragePaths};
use crate::gamepad::{GamepadBindings, GamepadInput};
use crate::keybindings::Action;
use crate::menu::draw_text;
use crate::presentation::Presentation;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
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
    storage: &StoragePaths,
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
    let mut browser: Option<Browser> = None;
    let mut notice = String::new();
    let mut result = None;
    let mut running = true;
    let gamepad_bindings = GamepadBindings::default();
    gamepad.suppress_held(&gamepad_bindings);
    while running {
        let gamepad_poll = gamepad.poll(&gamepad_bindings);
        for action in gamepad_poll.pressed_actions {
            handle_action(
                action,
                games,
                storage,
                &mut browser,
                &mut selected,
                &mut notice,
                &mut result,
                &mut running,
            );
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
            browser.as_ref(),
            selected,
            &notice,
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
                    let action = match symbol {
                        0xff52 => Some(Action::Up),
                        0xff54 => Some(Action::Down),
                        0xff0d => Some(Action::A),
                        0xff1b | 0xff08 => Some(Action::B),
                        _ => None,
                    };
                    if let Some(action) = action {
                        handle_action(
                            action,
                            games,
                            storage,
                            &mut browser,
                            &mut selected,
                            &mut notice,
                            &mut result,
                            &mut running,
                        );
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

struct Browser {
    directory: PathBuf,
    entries: Vec<PathBuf>,
}

impl Browser {
    fn open(directory: &Path) -> io::Result<Self> {
        let mut entries = fs::read_dir(directory)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_dir()
                    || path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| {
                            ["gba", "agb", "bin"]
                                .iter()
                                .any(|candidate| ext.eq_ignore_ascii_case(candidate))
                        })
            })
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| {
            b.is_dir()
                .cmp(&a.is_dir())
                .then_with(|| a.file_name().cmp(&b.file_name()))
        });
        Ok(Self {
            directory: directory.to_owned(),
            entries,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_action(
    action: Action,
    games: &[Game],
    storage: &StoragePaths,
    browser: &mut Option<Browser>,
    selected: &mut usize,
    notice: &mut String,
    result: &mut Option<Game>,
    running: &mut bool,
) {
    let count = browser
        .as_ref()
        .map_or(games.len() + 1, |view| view.entries.len() + 1);
    match selection_action(action, *selected, count) {
        Some(SelectionAction::Move(index)) => {
            *selected = index;
            notice.clear();
        }
        Some(SelectionAction::Cancel) if browser.is_some() => {
            *browser = None;
            *selected = 0;
            notice.clear();
        }
        Some(SelectionAction::Cancel) => *running = false,
        Some(SelectionAction::Select(index)) => {
            notice.clear();
            if let Some(view) = browser.as_ref() {
                let path = if index == 0 {
                    view.directory
                        .parent()
                        .unwrap_or(&view.directory)
                        .to_owned()
                } else {
                    view.entries[index - 1].clone()
                };
                if path.is_dir() {
                    match Browser::open(&path) {
                        Ok(next) => {
                            *browser = Some(next);
                            *selected = 0;
                        }
                        Err(error) => {
                            eprintln!("Cannot open directory {}: {error}", path.display());
                            *notice = "CANNOT OPEN DIRECTORY".into();
                        }
                    }
                } else {
                    match game_library::import(&path, storage) {
                        Ok(game) => {
                            *result = Some(game);
                            *running = false;
                        }
                        Err(error) => {
                            eprintln!("Cannot import {}: {error}", path.display());
                            *notice = if error.kind() == io::ErrorKind::AlreadyExists {
                                "ROM OR NAME ALREADY EXISTS"
                            } else {
                                "IMPORT FAILED SEE LOG"
                            }
                            .into();
                        }
                    }
                }
            } else if index == 0 {
                let home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/"));
                match Browser::open(&home) {
                    Ok(view) => {
                        *browser = Some(view);
                        *selected = 0;
                    }
                    Err(error) => {
                        eprintln!("Cannot open {}: {error}", home.display());
                        *notice = "CANNOT OPEN HOME".into();
                    }
                }
            } else {
                *result = games.get(index - 1).cloned();
                *running = false;
            }
        }
        None => {}
    }
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
    browser: Option<&Browser>,
    selected: usize,
    notice: &str,
) -> Result<(), Box<dyn Error>> {
    let pixels = library_pixels(games, browser, selected, notice);
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

fn library_pixels(
    games: &[Game],
    browser: Option<&Browser>,
    selected: usize,
    notice: &str,
) -> Vec<u32> {
    let mut pixels = vec![0x101820; 240 * 160];
    let title = if browser.is_some() {
        "LOAD ROM"
    } else {
        "GAME LIBRARY"
    };
    draw_text(
        &mut pixels,
        (240 - title.len() * 6) / 2,
        12,
        title,
        0x80d8ff,
    );
    let version = format!("V{}", env!("CARGO_PKG_VERSION"));
    draw_text(&mut pixels, 236 - version.len() * 6, 12, &version, 0x90a0b0);
    for x in 10..230 {
        pixels[30 * 240 + x] = 0xffffff;
    }
    let rows: Vec<String> = if let Some(view) = browser {
        std::iter::once("(DIR) ..".to_owned())
            .chain(view.entries.iter().map(|path| {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if path.is_dir() {
                    format!("(DIR) {name}")
                } else {
                    name.into_owned()
                }
            }))
            .collect()
    } else {
        std::iter::once("LOAD ROM".to_owned())
            .chain(games.iter().map(|game| game.title.clone()))
            .collect()
    };
    const VISIBLE_GAMES: usize = 4;
    let start = selected.saturating_sub(VISIBLE_GAMES - 1);
    for (row, title) in rows.iter().enumerate().skip(start).take(VISIBLE_GAMES) {
        let y = 34 + (row - start) * 21;
        let color = if row == selected { 0xffd060 } else { 0xffffff };
        draw_text(
            &mut pixels,
            12,
            y,
            if row == selected { ">" } else { " " },
            color,
        );
        let (first_line, second_line) = title_lines(title);
        draw_text(&mut pixels, 25, y, &first_line, color);
        draw_text(&mut pixels, 25, y + 9, &second_line, color);
    }
    if !notice.is_empty() {
        draw_text(&mut pixels, 12, 120, notice, 0xffd060);
    } else if let Some(view) = browser {
        let path = view.directory.to_string_lossy().to_ascii_uppercase();
        let chars: Vec<char> = path.chars().collect();
        let shown: String = chars[chars.len().saturating_sub(36)..].iter().collect();
        draw_text(&mut pixels, 12, 120, &shown, 0x90a0b0);
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
        let pixels = super::library_pixels(&[], None, 0, "");
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
    fn browser_navigates_imports_and_reports_conflicts() {
        use crate::game_library::StoragePaths;
        use std::fs;
        let root = std::env::temp_dir().join(format!("rgba-browser-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let source = root.join("source");
        fs::create_dir_all(source.join("folder")).expect("source folder");
        fs::write(source.join("game.gba"), vec![1; 0xc0]).expect("ROM");
        fs::write(source.join("ignore.txt"), b"ignored").expect("text");
        let paths = StoragePaths {
            data: root.join("data"),
            config: root.join("config"),
            games: root.join("data/games"),
            saves: root.join("data/saves"),
            states: root.join("data/states"),
            settings: root.join("config/settings"),
        };
        paths.create().expect("storage");
        let mut browser = Some(super::Browser::open(&source).expect("browser"));
        assert_eq!(browser.as_ref().expect("view").entries.len(), 2);
        let mut selected = 1;
        let mut notice = String::new();
        let mut result = None;
        let mut running = true;
        super::handle_action(
            Action::A,
            &[],
            &paths,
            &mut browser,
            &mut selected,
            &mut notice,
            &mut result,
            &mut running,
        );
        assert_eq!(
            browser.as_ref().expect("view").directory,
            source.join("folder")
        );
        super::handle_action(
            Action::A,
            &[],
            &paths,
            &mut browser,
            &mut selected,
            &mut notice,
            &mut result,
            &mut running,
        );
        assert_eq!(browser.as_ref().expect("view").directory, source);
        selected = 2;
        super::handle_action(
            Action::A,
            &[],
            &paths,
            &mut browser,
            &mut selected,
            &mut notice,
            &mut result,
            &mut running,
        );
        assert!(!running);
        assert_eq!(
            result.as_ref().expect("game").path,
            paths.games.join("game.gba")
        );
        running = true;
        result = None;
        super::handle_action(
            Action::A,
            &[],
            &paths,
            &mut browser,
            &mut selected,
            &mut notice,
            &mut result,
            &mut running,
        );
        assert!(running);
        assert!(result.is_none());
        assert_eq!(notice, "ROM OR NAME ALREADY EXISTS");
        super::handle_action(
            Action::B,
            &[],
            &paths,
            &mut browser,
            &mut selected,
            &mut notice,
            &mut result,
            &mut running,
        );
        assert!(browser.is_none());
        assert!(running);
        fs::remove_dir_all(root).expect("cleanup");
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
            super::library_pixels(&games, None, 0, ""),
            super::library_pixels(&games, None, 1, "")
        );
    }
}
