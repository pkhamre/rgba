//! Linux X11/XWayland emulator preview frontend.

mod arguments;
mod desktop;

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
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
