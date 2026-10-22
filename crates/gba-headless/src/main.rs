//! Bounded development runner for the emulator core.

mod arguments;
mod runner;

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    if env::args()
        .skip(1)
        .any(|arg| arg == "--help" || arg == "-h")
    {
        println!("{}", arguments::USAGE);
        return ExitCode::SUCCESS;
    }
    match runner::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
