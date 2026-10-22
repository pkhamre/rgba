use crate::arguments;
use crate::arguments::Arguments;
use crate::overlay;
use gba_host::Session;
use gba_host::audio_output::AudioOutput;
use gba_host::screenshot;
use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;
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
    let mut session = Session::try_new(&args.bios, &args.rom, args.save.as_deref())?;
    session
        .machine_mut()
        .set_rtc_timestamp(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs());
    let outcome = run_window(&args, &mut session);
    if let Err(error) = session.flush_save() {
        eprintln!("Save failed on exit; unsaved progress could not be persisted: {error}");
        return Err(error.into());
    }
    outcome
}

fn run_window(args: &Arguments, session: &mut Session) -> Result<(), Box<dyn Error>> {
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
    let pixmap = connection.generate_id()?;
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
    connection
        .create_pixmap(depth, pixmap, window, width, height)?
        .check()?;
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
    let stride = (usize::from(width) * bytes_per_pixel).div_ceil(scanline_pad) * scanline_pad;
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
    let mut held = [false; 256];
    let mut paused = false;
    let mut fast = false;
    let mut running = true;
    let mut frame_count = 0u64;
    let frame_time = Duration::from_secs_f64(280896.0 / 16777216.0);
    let mut next_frame = Instant::now();
    let mut last_write = Instant::now();
    let mut revision = session.machine().save_revision();
    while running {
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
                    match symbol {
                        0xff1b => running = false,
                        0x20 if first => {
                            paused = !paused;
                            let label = format!("{title}{}", if paused { " - PAUSED" } else { "" });
                            connection.change_property8(
                                PropMode::REPLACE,
                                window,
                                AtomEnum::WM_NAME,
                                AtomEnum::STRING,
                                label.as_bytes(),
                            )?;
                        }
                        0xff09 => fast = true,
                        0xffc2 if first => flush_save(session),
                        0xffc9 if first => {
                            if let Err(error) = capture(session) {
                                eprintln!("Screenshot failed: {error}");
                            }
                        }
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
                    held[usize::from(event.detail)] = false;
                    if key_symbol(
                        usize::from(event.detail),
                        minimum_keycode,
                        columns,
                        &mapping.keysyms,
                    ) == 0xff09
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
        if !running {
            break;
        }
        let mut buttons = 0;
        for (code, pressed) in held.iter().enumerate() {
            if *pressed {
                buttons |= button(key_symbol(code, minimum_keycode, columns, &mapping.keysyms));
            }
        }
        session.machine_mut().set_keys(buttons);
        if !paused {
            session.machine_mut().run_frame()?;
            frame_count += 1;
        }
        let samples = session.machine_mut().take_audio();
        if let Some(output) = &mut audio {
            output.set_muted(paused || fast);
            output.submit(samples);
        }
        let overlay = paused.then(|| overlay::paused(session.machine().screen()));
        let image = pack_pixels(
            overlay
                .as_deref()
                .unwrap_or_else(|| session.machine().screen()),
            args.scale,
            bytes_per_pixel,
            stride,
            image_order == ImageOrder::LSB_FIRST,
            masks,
        );
        for row in (0..usize::from(height)).step_by(rows_per_request) {
            let rows = rows_per_request.min(usize::from(height) - row);
            connection.put_image(
                ImageFormat::Z_PIXMAP,
                pixmap,
                gc,
                width,
                rows as u16,
                0,
                row as i16,
                0,
                depth,
                &image[row * stride..(row + rows) * stride],
            )?;
        }
        connection.copy_area(pixmap, window, gc, 0, 0, 0, 0, width, height)?;
        connection.flush()?;
        let current_revision = session.machine().save_revision();
        if current_revision != revision {
            revision = current_revision;
            last_write = Instant::now();
        }
        if session.machine().save_dirty() && last_write.elapsed() >= Duration::from_secs(2) {
            flush_save(session);
            last_write = Instant::now();
        }
        if args.frames.is_some_and(|limit| frame_count >= limit) {
            break;
        }
        if fast && !paused {
            next_frame = Instant::now();
        } else {
            next_frame += frame_time;
            let now = Instant::now();
            if next_frame > now {
                std::thread::sleep(next_frame - now);
            } else if now.duration_since(next_frame) > Duration::from_millis(250) {
                next_frame = now;
            }
        }
    }
    if let Some(output) = &audio {
        eprintln!("Audio host dropped frames: {}", output.dropped());
    }
    eprintln!(
        "Completed frames={frame_count} clocks={} PCM={} core_dropped={}",
        session.machine().clocks(),
        session.machine().audio_frames(),
        session.machine().audio_dropped()
    );
    connection.free_pixmap(pixmap)?;
    connection.free_gc(gc)?;
    connection.flush()?;
    Ok(())
}

fn flush_save(session: &mut Session) {
    match session.flush_save() {
        Ok(true) => eprintln!("Saved {}", session.save_path().display()),
        Ok(false) => {}
        Err(error) => eprintln!(
            "Save failed: {error}; progress remains in memory. Fix the path or free space, then press F5 before quitting."
        ),
    }
}

fn capture(session: &Session) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all("artifacts")?;
    let name = format!(
        "artifacts/screenshot-{}.ppm",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
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

fn button(symbol: u32) -> u16 {
    match symbol {
        0x7a | 0x5a => 1,
        0x78 | 0x58 => 2,
        0xff08 => 4,
        0xff0d => 8,
        0xff53 | 0x64 => 16,
        0xff51 | 0x61 => 32,
        0xff52 | 0x77 => 64,
        0xff54 | 0x73 => 128,
        0x65 => 256,
        0x71 => 512,
        _ => 0,
    }
}

fn pack_pixels(
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

#[cfg(test)]
mod tests {
    #[test]
    fn pixels_are_scaled_and_packed_without_display() {
        let mut pixels = vec![0; 240 * 160];
        pixels[0] = 0xff8040;
        let packed = super::pack_pixels(&pixels, 2, 4, 480 * 4, true, [0xff0000, 0xff00, 0xff]);
        assert_eq!(&packed[..8], &[0x40, 0x80, 0xff, 0, 0x40, 0x80, 0xff, 0]);
        assert_eq!(&packed[480 * 4..480 * 4 + 4], &[0x40, 0x80, 0xff, 0]);
        let big = super::pack_pixels(&pixels, 1, 3, 720, false, [0xff0000, 0xff00, 0xff]);
        assert_eq!(&big[..3], &[0xff, 0x80, 0x40]);
        assert_eq!(super::button(0xff0d), 8);
        assert_eq!(super::button(0x7a), 1);
    }
}
