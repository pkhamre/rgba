//! Linux X11/XWayland emulator preview frontend.

mod arguments;
mod debug_log;
mod desktop;
mod frame_cache;
mod frame_pacing;
mod game_library;
mod gamepad;
mod keybindings;
mod library_window;
mod menu;
mod overlay;
mod presentation;
mod speed;

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    match debug_log::launch() {
        Ok(Some(status)) => return status,
        Ok(None) => {}
        Err(error) => {
            eprintln!("error: cannot enable debug logging: {error}");
            return ExitCode::FAILURE;
        }
    }
    if env::args()
        .skip(1)
        .any(|value| value == "--help" || value == "-h")
    {
        println!("{}", arguments::USAGE);
        return ExitCode::SUCCESS;
    }
    match desktop::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
