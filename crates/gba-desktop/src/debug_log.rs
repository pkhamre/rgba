//! Opt-in stdout/stderr capture, including failures before desktop initialization.

use std::env;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const CHILD_MARKER: &str = "_RGBA_DEBUG_LOG_CHILD";

/// Runs the application once with both output streams redirected to a private
/// log. Using a child process avoids unsafe descriptor manipulation and also
/// captures panics and output inherited by subprocesses. The parent waits so an
/// AppImage mount remains available until the child exits.
pub(crate) fn launch() -> io::Result<Option<ExitCode>> {
    let arguments: Vec<OsString> = env::args_os().skip(1).collect();
    if !arguments.iter().any(|arg| arg == "--debug") || env::var_os(CHILD_MARKER).is_some() {
        return Ok(None);
    }
    let directory = log_directory(env::var_os("XDG_STATE_HOME"), env::var_os("HOME"))?;
    std::fs::create_dir_all(&directory)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let path = directory.join(format!("rgba-{timestamp}-{}.log", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    writeln!(file, "rgba {} debug log", env!("CARGO_PKG_VERSION"))?;
    writeln!(file, "Arguments: {arguments:?}")?;
    writeln!(file, "Working directory: {}", env::current_dir()?.display())?;
    eprintln!("Debug log: {}", path.display());
    let status = Command::new(env::current_exe()?)
        .args(arguments)
        .env(CHILD_MARKER, &path)
        .stdout(Stdio::from(file.try_clone()?))
        .stderr(Stdio::from(file.try_clone()?))
        .status();
    match status {
        Ok(status) => {
            writeln!(file, "Process exited: {status}")?;
            let code = status
                .code()
                .unwrap_or_else(|| 128 + status.signal().unwrap_or(1));
            Ok(Some(ExitCode::from(u8::try_from(code).unwrap_or(1))))
        }
        Err(error) => {
            writeln!(file, "Cannot start debug process: {error}")?;
            Err(error)
        }
    }
}

fn log_directory(state_home: Option<OsString>, home: Option<OsString>) -> io::Result<PathBuf> {
    let base = state_home.filter(|value| Path::new(value).is_absolute());
    let base = match base {
        Some(path) => PathBuf::from(path),
        None => {
            let home = home
                .filter(|value| Path::new(value).is_absolute())
                .ok_or_else(|| {
                    io::Error::other("--debug requires absolute XDG_STATE_HOME or HOME")
                })?;
            PathBuf::from(home).join(".local/state")
        }
    };
    Ok(base.join("rgba/logs"))
}

#[cfg(test)]
mod tests {
    use super::log_directory;
    use std::ffi::OsString;
    use std::path::Path;

    #[test]
    fn state_directory_uses_absolute_xdg_or_home_fallback() {
        assert_eq!(
            log_directory(Some(OsString::from("/state")), None).expect("absolute XDG"),
            Path::new("/state/rgba/logs")
        );
        for state in [
            None,
            Some(OsString::new()),
            Some(OsString::from("relative")),
        ] {
            assert_eq!(
                log_directory(state, Some(OsString::from("/home/test"))).expect("HOME fallback"),
                Path::new("/home/test/.local/state/rgba/logs")
            );
        }
        assert!(log_directory(None, None).is_err());
        assert!(log_directory(None, Some(OsString::from("relative"))).is_err());
    }
}
