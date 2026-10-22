use crate::arguments;
use crate::arguments::Arguments;
use crate::frame_cache::FrameCache;
use crate::frame_pacing::FramePacing;
use crate::keybindings::{Action, KeyBindings};
use crate::menu::{Command, MenuState};
use crate::overlay;
use crate::presentation::Presentation;
use gba_host::Session;
use gba_host::audio_output::AudioOutput;
use gba_host::screenshot;
use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;
use std::process;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt, CreateGCAux, CreateWindowAux, EventMask, ImageFormat, ImageOrder,
    PropMode, VisualClass, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as WrapperConnectionExt;

pub(crate) fn run() -> Result<(), Box<dyn Error>> {
    let args = Arguments::parse(env::args().skip(1)).map_err(|message| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{message}\n{}", arguments::USAGE),
        )
    })?;
    let storage = crate::game_library::StoragePaths::from_env()?;
    storage.create()?;
    let mut games = crate::game_library::scan(&storage)?;
    let explicit_game = args
        .rom
        .as_deref()
        .map(crate::game_library::game_from_path)
        .transpose()?;
    let custom_save_identity = args
        .save
        .as_ref()
        .and(explicit_game.as_ref())
        .map(|game| game.identity);
    let mut selected = if let Some(game) = explicit_game.clone() {
        Some(game)
    } else if games.len() == 1 {
        games.first().cloned()
    } else {
        crate::library_window::choose(&args, &games)?
    };

    while let Some(game) = selected {
        if !games.iter().any(|known| known.identity == game.identity) {
            games.push(game.clone());
        }
        let migration = crate::game_library::migrate_legacy(&game.path, &game.identity, &storage)?;
        for conflict in migration.conflicts {
            eprintln!(
                "Keeping existing central data; legacy file was not copied: {}",
                conflict.display()
            );
        }
        let save_path = if custom_save_identity == Some(game.identity) {
            args.save
                .clone()
                .unwrap_or_else(|| storage.save_path(&game.identity))
        } else {
            storage.save_path(&game.identity)
        };
        let state_path = storage.state_path(&game.identity);
        let settings_path = storage.settings_path(&game.identity);
        let mut session = Session::try_new_with_paths(
            &args.bios,
            &game.path,
            &save_path,
            &state_path,
            &settings_path,
        )?;
        session
            .machine_mut()
            .set_rtc_timestamp(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs());
        let outcome = run_window(&args, &mut session);
        if let Err(error) = session.flush_save() {
            eprintln!("Save failed on exit; unsaved progress could not be persisted: {error}");
            return Err(error.into());
        }
        match outcome? {
            RunOutcome::Exit => return Ok(()),
            RunOutcome::Library => {
                games = crate::game_library::scan(&storage)?;
                if !games.iter().any(|known| known.identity == game.identity) {
                    games.push(game);
                }
                selected = crate::library_window::choose(&args, &games)?;
            }
        }
    }
    Ok(())
}

enum RunOutcome {
    Exit,
    Library,
}

fn run_window(args: &Arguments, session: &mut Session) -> Result<RunOutcome, Box<dyn Error>> {
    let (connection, screen_number) = RustConnection::connect(None).map_err(|error| {
        io::Error::other(format!("cannot open X11 display: {error}. Run in an X11/XWayland desktop, or use gba-headless --frames ... --screenshot ..."))
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
        return Err(
            io::Error::other("this preview requires an X11 24/32-bit TrueColor display").into(),
        );
    }
    let visual = screen
        .allowed_depths
        .iter()
        .flat_map(|depth| &depth.visuals)
        .find(|visual| visual.visual_id == screen.root_visual)
        .ok_or_else(|| io::Error::other("X11 root visual is missing"))?;
    if visual.class != VisualClass::TRUE_COLOR {
        return Err(io::Error::other("this preview requires an X11 TrueColor visual").into());
    }
    let masks = [visual.red_mask, visual.green_mask, visual.blue_mask];
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
                EventMask::EXPOSURE
                    | EventMask::KEY_PRESS
                    | EventMask::KEY_RELEASE
                    | EventMask::FOCUS_CHANGE,
            ),
        )?
        .check()?;
    connection
        .create_gc(gc, window, &CreateGCAux::new())?
        .check()?;
    let presentation = Presentation::create(
        &connection,
        window,
        gc,
        depth,
        screen.root_visual,
        args.scale,
    )?;
    let upload_scale = presentation.upload_scale;
    let upload_width = 240 * upload_scale;
    let upload_height = 160 * upload_scale;
    let title = format!("Rust GBA - {} [preview]", session.title());
    connection
        .change_property8(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            title.as_bytes(),
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
    let mapping = connection
        .get_keyboard_mapping(setup.min_keycode, setup.max_keycode - setup.min_keycode + 1)?
        .reply()?;
    let minimum_keycode = setup.min_keycode;
    let columns = usize::from(mapping.keysyms_per_keycode);
    let image_order = setup.image_byte_order;
    let bytes_per_pixel = usize::from(format.bits_per_pixel) / 8;
    let scanline_pad = usize::from(format.scanline_pad) / 8;
    let stride =
        (usize::from(upload_width) * bytes_per_pixel).div_ceil(scanline_pad) * scanline_pad;
    let rows_per_request = ((connection.maximum_request_bytes() - 64) / stride).max(1);
    connection.map_window(window)?.check()?;
    connection.flush()?;
    eprintln!(
        "{}\nSave: {}",
        arguments::USAGE,
        session.save_path().display()
    );
    let mut audio = if args.audio == "off" {
        None
    } else {
        match AudioOutput::open(&args.audio) {
            Ok(output) => Some(output),
            Err(error) => {
                eprintln!("{error}; continuing without audio");
                None
            }
        }
    };
    let mut bindings = crate::keybindings::KeyBindings::default();
    let mut settings_warning = None;
    match session.read_settings() {
        Ok(Some(bytes)) => match std::str::from_utf8(&bytes)
            .map_err(|_| crate::keybindings::BindingError::InvalidFormat)
            .and_then(crate::keybindings::KeyBindings::decode)
        {
            Ok(saved) => bindings = saved,
            Err(error) => {
                settings_warning = Some(format!("KEY SETTINGS INVALID: {error:?}"));
                eprintln!("Ignoring invalid key settings: {error:?}");
            }
        },
        Ok(None) => {}
        Err(error) => {
            settings_warning = Some(format!("KEY SETTINGS: {error}"));
            eprintln!("Ignoring unreadable key settings: {error}");
        }
    }
    let mut menu = MenuState::default();
    let mut menu_open = false;
    let mut gamepad = crate::gamepad::GamepadInput::new();
    let mut state_notice = None;
    let mut held = [false; 256];
    let mut paused = false;
    let mut fast = false;
    let mut running = true;
    let mut return_to_library = false;
    let mut frame_count = 0u64;
    let mut emulation_work = Duration::ZERO;
    let mut max_emulation_work = Duration::ZERO;
    let mut presentation_work = Duration::ZERO;
    let mut presentation_count = 0u64;
    let mut frame_cache = FrameCache::default();
    let frame_time = Duration::from_secs_f64(280896.0 / 16777216.0);
    let mut next_frame = Instant::now();
    let mut pacing = FramePacing::default();
    let mut last_write = Instant::now();
    let mut revision = session.machine().save_revision();
    while running {
        let gamepad_poll = gamepad.poll();
        if gamepad_poll.menu_pressed && !menu_open {
            menu = MenuState::default();
            if let Some(warning) = settings_warning.take() {
                menu.set_status(warning);
            }
            menu_open = true;
            fast = false;
            set_window_title(&connection, window, protocols, delete, &title, true, paused)?;
        }
        let menu_was_open = menu_open;
        let mut events = Vec::new();
        while let Some(event) = connection.poll_for_event()? {
            events.push(event);
        }
        let mut events = events.into_iter().peekable();
        while let Some(event) = events.next() {
            match event {
                Event::ClientMessage(message)
                    if message.type_ == protocols && message.data.as_data32()[0] == delete =>
                {
                    running = false
                }
                Event::DestroyNotify(_) => running = false,
                Event::FocusOut(_) => {
                    held.fill(false);
                    fast = false;
                }
                Event::KeyPress(event) => {
                    let code = usize::from(event.detail);
                    let symbol = key_symbol(code, minimum_keycode, columns, &mapping.keysyms);
                    let first = !held[code];
                    held[code] = true;
                    if !first {
                        continue;
                    }
                    if menu_open {
                        let command = menu.handle_key(symbol, &mut bindings);
                        handle_menu_command(
                            MenuRuntime {
                                connection: &connection,
                                window,
                                protocols,
                                delete,
                                title: &title,
                                session,
                                audio: &mut audio,
                                gamepad: &mut gamepad,
                                menu: &mut menu,
                                bindings: &mut bindings,
                                menu_open: &mut menu_open,
                                held: &mut held,
                                state_notice: &mut state_notice,
                                settings_warning: &mut settings_warning,
                                paused,
                                revision: &mut revision,
                                running: &mut running,
                                return_to_library: &mut return_to_library,
                            },
                            command,
                        )?;
                        continue;
                    }
                    match bindings.action_for(symbol) {
                        Some(crate::keybindings::Action::Menu) => {
                            menu = crate::menu::MenuState::default();
                            if let Some(warning) = settings_warning.take() {
                                menu.set_status(warning);
                            }
                            menu_open = true;
                            fast = false;
                            set_window_title(
                                &connection,
                                window,
                                protocols,
                                delete,
                                &title,
                                true,
                                paused,
                            )?;
                        }
                        Some(crate::keybindings::Action::Pause) => {
                            paused = !paused;
                            set_window_title(
                                &connection,
                                window,
                                protocols,
                                delete,
                                &title,
                                false,
                                paused,
                            )?;
                        }
                        Some(crate::keybindings::Action::FastForward) => fast = true,
                        Some(crate::keybindings::Action::FlushSave) => {
                            let _ = flush_save(session);
                        }
                        Some(crate::keybindings::Action::Screenshot) => {
                            if let Err(error) = capture(session) {
                                eprintln!("Screenshot failed: {error}");
                            }
                        }
                        Some(crate::keybindings::Action::SaveState) => match session.save_state() {
                            Ok(()) => {
                                set_state_notice(&mut state_notice, "STATE SAVED", true);
                                eprintln!("Save state: {}", session.state_path().display());
                            }
                            Err(error) => {
                                set_state_notice(&mut state_notice, "SAVE FAILED", false);
                                eprintln!("Save-state write failed: {error}");
                            }
                        },
                        Some(crate::keybindings::Action::Exit) => {
                            if flush_save(session) {
                                running = false;
                            }
                        }
                        Some(crate::keybindings::Action::LoadState) => match session.load_state() {
                            Ok(()) => {
                                set_state_notice(&mut state_notice, "STATE LOADED", true);
                                if let Some(output) = &mut audio {
                                    output.reset_after_state_load();
                                }
                                revision = session.machine().save_revision();
                                eprintln!("Loaded state: {}", session.state_path().display());
                            }
                            Err(error) => {
                                set_state_notice(&mut state_notice, "LOAD FAILED", false);
                                eprintln!("Save-state load failed: {error}");
                            }
                        },
                        _ => {}
                    }
                }
                Event::KeyRelease(event) => {
                    // Classic X11 autorepeat sends an adjacent release/press pair.
                    // Keep held state so one-shot controls do not repeat.
                    if matches!(events.peek(), Some(Event::KeyPress(next)) if next.detail == event.detail && next.time == event.time)
                    {
                        continue;
                    }
                    let code = usize::from(event.detail);
                    held[code] = false;
                    let symbol = key_symbol(code, minimum_keycode, columns, &mapping.keysyms);
                    if bindings.action_for(symbol) == Some(crate::keybindings::Action::FastForward)
                    {
                        fast = false;
                    }
                }
                Event::Error(error) => {
                    return Err(io::Error::other(format!("X11 protocol error: {error:?}")).into());
                }
                _ => {}
            }
        }
        if menu_open && running {
            for action in gamepad_poll.pressed_actions.iter().copied() {
                if gamepad_poll.menu_pressed && matches!(action, Action::Start | Action::Select) {
                    continue;
                }
                let command = menu.handle_gamepad_action(action, &mut bindings);
                handle_menu_command(
                    MenuRuntime {
                        connection: &connection,
                        window,
                        protocols,
                        delete,
                        title: &title,
                        session,
                        audio: &mut audio,
                        gamepad: &mut gamepad,
                        menu: &mut menu,
                        bindings: &mut bindings,
                        menu_open: &mut menu_open,
                        held: &mut held,
                        state_notice: &mut state_notice,
                        settings_warning: &mut settings_warning,
                        paused,
                        revision: &mut revision,
                        running: &mut running,
                        return_to_library: &mut return_to_library,
                    },
                    command,
                )?;
                if !menu_open || !running {
                    break;
                }
            }
        }
        if !running {
            break;
        }
        let buttons = if menu_open || menu_was_open {
            0
        } else {
            bindings.button_mask(&held, minimum_keycode, columns, &mapping.keysyms)
                | gamepad_poll.buttons
        };
        session.machine_mut().set_keys(buttons);
        if !paused && !menu_open {
            let started = Instant::now();
            session.machine_mut().run_frame()?;
            let elapsed = started.elapsed();
            emulation_work += elapsed;
            max_emulation_work = max_emulation_work.max(elapsed);
            frame_count += 1;
        }
        let samples = session.machine_mut().take_audio();
        if let Some(output) = &mut audio {
            output.set_muted(paused || menu_open || fast);
            output.submit(samples);
        }
        let presentation_started = Instant::now();
        let overlay = if menu_open {
            Some(menu.render(session.machine().screen(), &bindings))
        } else if paused {
            Some(overlay::paused(session.machine().screen()))
        } else {
            None
        };
        let base_pixels = overlay
            .as_deref()
            .unwrap_or_else(|| session.machine().screen());
        let notice = state_notice
            .as_ref()
            .filter(|notice| !menu_open && Instant::now() < notice.expires_at)
            .map(|notice| overlay::notice(base_pixels, notice.message, notice.success));
        let pixels = notice.as_deref().unwrap_or(base_pixels);
        if frame_cache.update(pixels) {
            let image = pack_pixels(
                pixels,
                upload_scale,
                bytes_per_pixel,
                stride,
                image_order == ImageOrder::LSB_FIRST,
                masks,
            );
            for row in (0..usize::from(upload_height)).step_by(rows_per_request) {
                let rows = rows_per_request.min(usize::from(upload_height) - row);
                connection.put_image(
                    ImageFormat::Z_PIXMAP,
                    presentation.pixmap,
                    gc,
                    upload_width,
                    rows as u16,
                    0,
                    row as i16,
                    0,
                    depth,
                    &image[row * stride..(row + rows) * stride],
                )?;
            }
        }
        // Always copy the cached pixmap: Expose must repaint even if game pixels
        // did not change. CPU/PPU/audio still advance once per emulated frame.
        presentation.present(&connection)?;
        connection.flush()?;
        presentation_work += presentation_started.elapsed();
        presentation_count += 1;
        let current_revision = session.machine().save_revision();
        if current_revision != revision {
            revision = current_revision;
            last_write = Instant::now();
        }
        if session.machine().save_dirty() && last_write.elapsed() >= Duration::from_secs(2) {
            let _ = flush_save(session);
            last_write = Instant::now();
        }
        if args.frames.is_some_and(|limit| frame_count >= limit) {
            break;
        }
        if fast && !paused && !menu_open {
            next_frame = Instant::now();
        } else {
            next_frame += frame_time;
            let now = Instant::now();
            let hint = if paused || menu_open {
                None
            } else {
                audio.as_ref().and_then(AudioOutput::needs_samples)
            };
            let wait = pacing.wait(next_frame.saturating_duration_since(now), hint);
            if !wait.is_zero() {
                std::thread::sleep(wait);
            } else if now > next_frame
                && now.duration_since(next_frame) > Duration::from_millis(250)
            {
                next_frame = now;
            }
        }
    }
    if let Some(output) = &audio {
        eprintln!("Audio host dropped frames: {}", output.dropped());
        let timing = output.timing();
        eprintln!(
            "Audio delivery: player={:?} submitted={} max_submit_gap_ms={:.1} gaps_over_40ms={} producer_hz={:.1} target_hz=32768",
            output.player_name(),
            timing.submitted_frames,
            timing.max_submission_gap.as_secs_f64() * 1000.0,
            timing.gaps_over_40_ms,
            timing.submission_rate_hz().unwrap_or(0.0)
        );
    }
    eprintln!(
        "Work timing: emulation_avg_ms={:.3} max_emulation_ms={:.3} presentation_avg_ms={:.3}",
        emulation_work.as_secs_f64() * 1000.0 / frame_count.max(1) as f64,
        max_emulation_work.as_secs_f64() * 1000.0,
        presentation_work.as_secs_f64() * 1000.0 / presentation_count.max(1) as f64
    );
    eprintln!(
        "Completed frames={frame_count} clocks={} PCM={} core_dropped={} video_uploads={}",
        session.machine().clocks(),
        session.machine().audio_frames(),
        session.machine().audio_dropped(),
        frame_cache.uploads()
    );
    presentation.close(&connection)?;
    connection.free_gc(gc)?;
    connection.flush()?;
    Ok(if return_to_library {
        RunOutcome::Library
    } else {
        RunOutcome::Exit
    })
}

fn set_window_title(
    connection: &impl Connection,
    window: u32,
    protocols: u32,
    delete: u32,
    title: &str,
    menu_open: bool,
    paused: bool,
) -> Result<(), Box<dyn Error>> {
    let suffix = if menu_open {
        " - OPTIONS"
    } else if paused {
        " - PAUSED"
    } else {
        ""
    };
    let label = format!("{title}{suffix}");
    connection
        .change_property8(
            PropMode::REPLACE,
            window,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            label.as_bytes(),
        )?
        .check()?;
    connection
        .change_property32(
            PropMode::REPLACE,
            window,
            protocols,
            AtomEnum::ATOM,
            &[delete],
        )?
        .check()?;
    Ok(())
}

struct MenuRuntime<'a> {
    connection: &'a RustConnection,
    window: u32,
    protocols: u32,
    delete: u32,
    title: &'a str,
    session: &'a mut Session,
    audio: &'a mut Option<AudioOutput>,
    gamepad: &'a mut crate::gamepad::GamepadInput,
    menu: &'a mut MenuState,
    bindings: &'a mut KeyBindings,
    menu_open: &'a mut bool,
    held: &'a mut [bool; 256],
    state_notice: &'a mut Option<StateNotice>,
    settings_warning: &'a mut Option<String>,
    paused: bool,
    revision: &'a mut u64,
    running: &'a mut bool,
    return_to_library: &'a mut bool,
}

fn handle_menu_command(runtime: MenuRuntime<'_>, command: Command) -> Result<(), Box<dyn Error>> {
    match command {
        Command::None => {}
        Command::Resume => {
            *runtime.menu_open = false;
            runtime.held.fill(false);
            runtime.gamepad.suppress_held();
            set_window_title(
                runtime.connection,
                runtime.window,
                runtime.protocols,
                runtime.delete,
                runtime.title,
                false,
                runtime.paused,
            )?;
        }
        Command::BackToLibrary => {
            if flush_save(runtime.session) {
                *runtime.return_to_library = true;
                *runtime.running = false;
            } else {
                runtime.menu.set_status("SAVE FAILED - SWITCH CANCELLED");
            }
        }
        Command::Exit => {
            if flush_save(runtime.session) {
                *runtime.running = false;
            } else {
                runtime.menu.set_status("SAVE FAILED - EXIT CANCELLED");
            }
        }
        Command::SaveState => match runtime.session.save_state() {
            Ok(()) => {
                runtime.menu.set_status("STATE SAVED");
                set_state_notice(runtime.state_notice, "STATE SAVED", true);
                eprintln!("Save state: {}", runtime.session.state_path().display());
            }
            Err(error) => {
                runtime.menu.set_status("STATE SAVE FAILED");
                set_state_notice(runtime.state_notice, "SAVE FAILED", false);
                eprintln!("Save-state write failed: {error}");
            }
        },
        Command::LoadState => match runtime.session.load_state() {
            Ok(()) => {
                runtime.menu.set_status("STATE LOADED");
                set_state_notice(runtime.state_notice, "STATE LOADED", true);
                if let Some(output) = runtime.audio {
                    output.reset_after_state_load();
                }
                eprintln!("Loaded state: {}", runtime.session.state_path().display());
                *runtime.revision = runtime.session.machine().save_revision();
            }
            Err(error) => {
                runtime.menu.set_status("STATE LOAD FAILED");
                set_state_notice(runtime.state_notice, "LOAD FAILED", false);
                eprintln!("Save-state load failed: {error}");
            }
        },
        Command::SettingsChanged => match runtime
            .session
            .write_settings(runtime.bindings.encode().as_bytes())
        {
            Ok(()) => {
                runtime.menu.set_status("SETTINGS SAVED");
                *runtime.settings_warning = None;
            }
            Err(error) => {
                runtime.menu.set_status("SETTINGS WRITE FAILED");
                eprintln!("Key settings write failed: {error}");
            }
        },
    }
    Ok(())
}

struct StateNotice {
    message: &'static str,
    success: bool,
    expires_at: Instant,
}

fn set_state_notice(notice: &mut Option<StateNotice>, message: &'static str, success: bool) {
    *notice = Some(StateNotice {
        message,
        success,
        expires_at: Instant::now() + Duration::from_secs(2),
    });
}

fn flush_save(session: &mut Session) -> bool {
    match session.flush_save() {
        Ok(true) => {
            eprintln!("Saved {}", session.save_path().display());
            true
        }
        Ok(false) => true,
        Err(error) => {
            eprintln!(
                "Save failed: {error}; progress remains in memory. Fix the path or free space, then press F5 before quitting."
            );
            false
        }
    }
}

fn capture(session: &Session) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all("artifacts")?;
    let name = format!(
        "artifacts/screenshot-{}-{}.ppm",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
        process::id()
    );
    screenshot::write_ppm(Path::new(&name), session.machine().screen())?;
    eprintln!("Screenshot: {name}");
    Ok(())
}

fn key_symbol(code: usize, minimum: u8, columns: usize, symbols: &[u32]) -> u32 {
    code.checked_sub(usize::from(minimum))
        .and_then(|index| symbols.get(index * columns))
        .copied()
        .unwrap_or(0)
}

pub(crate) fn pack_pixels(
    pixels: &[u32],
    scale: u16,
    bytes: usize,
    stride: usize,
    little: bool,
    masks: [u32; 3],
) -> Vec<u8> {
    let scale = usize::from(scale);
    let mut image = vec![0; stride * 160 * scale];
    let standard_rgb = masks == [0xff0000, 0xff00, 0xff];
    let layout = masks.map(|mask| {
        if mask == 0 {
            (0, 0, 0)
        } else {
            let shift = mask.trailing_zeros();
            (mask, shift, mask >> shift)
        }
    });
    // Convert each of the 38,400 source pixels once, not every enlarged pixel.
    // Duplicate horizontally, then copy the completed scanline vertically.
    for y in 0..160 {
        let row = y * scale * stride;
        for x in 0..240 {
            let color = pixels[y * 240 + x];
            let packed = if standard_rgb {
                color & 0x00ff_ffff
            } else {
                let channels = [(color >> 16) & 255, (color >> 8) & 255, color & 255];
                let mut packed = 0;
                for (channel, (mask, shift, maximum)) in channels.into_iter().zip(layout) {
                    if mask != 0 {
                        packed |= ((channel * maximum / 255) << shift) & mask;
                    }
                }
                packed
            };
            let encoded = if little {
                packed.to_le_bytes()
            } else {
                packed.to_be_bytes()
            };
            let pixel = if little {
                &encoded[..bytes]
            } else {
                &encoded[4 - bytes..]
            };
            let offset = row + x * scale * bytes;
            for repeat in 0..scale {
                let start = offset + repeat * bytes;
                image[start..start + bytes].copy_from_slice(pixel);
            }
        }
        for repeat in 1..scale {
            image.copy_within(row..row + stride, row + repeat * stride);
        }
    }
    image
}

#[cfg(test)]
mod tests {
    use crate::keybindings::KeyBindings;

    fn reference_pixels(
        pixels: &[u32],
        scale: u16,
        bytes: usize,
        stride: usize,
        little: bool,
        masks: [u32; 3],
    ) -> Vec<u8> {
        let scale = usize::from(scale);
        let mut image = vec![0; stride * 160 * scale];
        for y in 0..160 * scale {
            for x in 0..240 * scale {
                let color = pixels[(y / scale) * 240 + x / scale];
                let channels = [(color >> 16) & 255, (color >> 8) & 255, color & 255];
                let mut packed = 0;
                for (channel, mask) in channels.into_iter().zip(masks) {
                    if mask != 0 {
                        packed |= ((channel * (mask >> mask.trailing_zeros()) / 255)
                            << mask.trailing_zeros())
                            & mask;
                    }
                }
                let encoded = if little {
                    packed.to_le_bytes()
                } else {
                    packed.to_be_bytes()
                };
                let offset = y * stride + x * bytes;
                if little {
                    image[offset..offset + bytes].copy_from_slice(&encoded[..bytes]);
                } else {
                    image[offset..offset + bytes].copy_from_slice(&encoded[4 - bytes..]);
                }
            }
        }
        image
    }

    #[test]
    fn optimized_packing_matches_reference_for_scales_formats_masks_and_padding() {
        let mut seed = 0x1234_5678u32;
        let pixels: Vec<_> = (0..240 * 160)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                seed
            })
            .collect();
        for scale in [1, 3, 6] {
            for bytes in [3, 4] {
                for little in [false, true] {
                    for masks in [
                        [0xff0000, 0xff00, 0xff],
                        [0xff, 0xff00, 0xff0000],
                        [0x3ff00000, 0xffc00, 0x3ff],
                        [0, 0xff00, 0xff],
                    ] {
                        if bytes == 3 && masks[0] == 0x3ff00000 {
                            continue;
                        }
                        let stride = 240 * usize::from(scale) * bytes + 8;
                        let expected =
                            reference_pixels(&pixels, scale, bytes, stride, little, masks);
                        let actual =
                            super::pack_pixels(&pixels, scale, bytes, stride, little, masks);
                        assert_eq!(
                            actual, expected,
                            "scale={scale}, bytes={bytes}, little={little}, masks={masks:?}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn pixels_are_scaled_and_packed_without_display() {
        let mut pixels = vec![0; 240 * 160];
        pixels[0] = 0xff8040;
        let packed = super::pack_pixels(&pixels, 2, 4, 480 * 4, true, [0xff0000, 0xff00, 0xff]);
        assert_eq!(&packed[..8], &[0x40, 0x80, 0xff, 0, 0x40, 0x80, 0xff, 0]);
        assert_eq!(&packed[480 * 4..480 * 4 + 4], &[0x40, 0x80, 0xff, 0]);
        let big = super::pack_pixels(&pixels, 1, 3, 720, false, [0xff0000, 0xff00, 0xff]);
        assert_eq!(&big[..3], &[0xff, 0x80, 0x40]);
        let mut held = [false; 256];
        held[10] = true;
        held[11] = true;
        let mut symbols = vec![0; 248];
        symbols[2] = 0xff0d;
        symbols[3] = 0x7a;
        assert_eq!(KeyBindings::default().button_mask(&held, 8, 1, &symbols), 9);
    }
}
