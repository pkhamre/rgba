//! Asset-free desktop CLI and missing-display regressions.

use std::fs;
use std::path::Path;
use std::process::Command;

fn debug_command(state_home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gba-desktop"));
    command
        .env("XDG_STATE_HOME", state_home)
        .env_remove("_RGBA_DEBUG_LOG_CHILD")
        .env_remove("DISPLAY");
    command
}

fn read_logs(directory: &Path) -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;
    fs::read_dir(directory)
        .expect("log directory")
        .map(|entry| {
            let path = entry.expect("directory entry").path();
            assert_eq!(
                fs::metadata(&path)
                    .expect("log metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            fs::read_to_string(path).expect("UTF-8 log")
        })
        .collect()
}

#[test]
fn debug_captures_stdout_stderr_and_exit_status_in_separate_private_logs() {
    let root = std::env::temp_dir().join(format!("rgba-debug-output-{}", std::process::id()));
    fs::create_dir(&root).expect("fixture directory");
    let help = debug_command(&root)
        .args(["--debug", "--help"])
        .output()
        .expect("debug help");
    assert!(help.status.success());
    assert!(help.stdout.is_empty());
    assert!(String::from_utf8_lossy(&help.stderr).contains("Debug log:"));
    let invalid = debug_command(&root)
        .args(["--debug", "--bios", "absent", "--scale", "0"])
        .output()
        .expect("debug invalid options");
    assert_eq!(invalid.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&invalid.stderr).contains("scale must be"));
    let logs = read_logs(&root.join("rgba/logs"));
    assert_eq!(logs.len(), 2);
    assert!(
        logs.iter().any(|log| log.contains("Linux X11/XWayland")
            && log.contains("Process exited: exit status: 0"))
    );
    assert!(
        logs.iter()
            .any(|log| log.contains("error: scale must be 1..6")
                && log.contains("Process exited: exit status: 1"))
    );
    assert!(
        logs.iter()
            .all(|log| log.contains(env!("CARGO_PKG_VERSION")) && log.contains("Arguments:"))
    );
    let normal = debug_command(&root)
        .arg("--help")
        .output()
        .expect("normal help");
    assert!(normal.status.success());
    assert!(String::from_utf8_lossy(&normal.stdout).contains("Linux X11/XWayland"));
    assert!(normal.stderr.is_empty());
    assert_eq!(read_logs(&root.join("rgba/logs")).len(), 2);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn debug_uses_home_fallback_and_reports_unusable_log_locations() {
    let root = std::env::temp_dir().join(format!("rgba-debug-paths-{}", std::process::id()));
    fs::create_dir(&root).expect("fixture directory");
    for state in ["", "relative"] {
        let output = debug_command(Path::new(state))
            .env("HOME", &root)
            .args(["--debug", "--help"])
            .output()
            .expect("fallback debug help");
        assert!(output.status.success());
    }
    assert_eq!(read_logs(&root.join(".local/state/rgba/logs")).len(), 2);
    let blocker = root.join("not-a-directory");
    fs::write(&blocker, b"unchanged").expect("blocker");
    let output = debug_command(&blocker)
        .args(["--debug", "--help"])
        .output()
        .expect("unusable debug path");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot enable debug logging"));
    assert_eq!(fs::read(&blocker).expect("unchanged blocker"), b"unchanged");
    let output = debug_command(Path::new("relative"))
        .env_remove("HOME")
        .args(["--debug", "--help"])
        .output()
        .expect("missing home");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("absolute XDG_STATE_HOME or HOME"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn debug_captures_runtime_errors_after_library_startup() {
    let root = std::env::temp_dir().join(format!("rgba-debug-runtime-{}", std::process::id()));
    fs::create_dir(&root).expect("fixture directory");
    let output = debug_command(&root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .args(["--debug", "--bios", "absent"])
        .output()
        .expect("debug library startup");
    assert!(!output.status.success());
    let logs = read_logs(&root.join("state/rgba/logs"));
    assert_eq!(logs.len(), 1);
    assert!(logs[0].contains("game library"));
    assert!(logs[0].contains("error:"));
    assert!(root.join("data/rgba/games").is_dir());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn help_and_invalid_options_need_no_display_or_assets() {
    let help = Command::new(env!("CARGO_BIN_EXE_gba-desktop"))
        .arg("--help")
        .env_remove("DISPLAY")
        .output()
        .expect("run help");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("X11/XWayland"));
    let invalid = Command::new(env!("CARGO_BIN_EXE_gba-desktop"))
        .args(["--bios", "absent", "--rom", "absent", "--scale", "0"])
        .env_remove("DISPLAY")
        .output()
        .expect("run invalid CLI");
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("scale must be 1..6"));
}

#[test]
fn library_mode_needs_a_display_but_not_a_rom_argument() {
    let directory = std::env::temp_dir().join(format!("gba-library-cli-{}", std::process::id()));
    fs::create_dir(&directory).expect("new fixture directory");
    let bios = directory.join("bios.bin");
    fs::write(&bios, [0; 16384]).expect("synthetic BIOS");
    let output = Command::new(env!("CARGO_BIN_EXE_gba-desktop"))
        .arg("--bios")
        .arg(&bios)
        .env("XDG_DATA_HOME", directory.join("xdg-data"))
        .env("XDG_CONFIG_HOME", directory.join("xdg-config"))
        .env_remove("DISPLAY")
        .output()
        .expect("run library mode without display");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("game library"));
    assert!(directory.join("xdg-data/rgba/games").is_dir());
    fs::remove_dir_all(directory).expect("remove fixtures");
}

#[test]
fn missing_display_reports_headless_alternative_without_executing_guest() {
    let directory = std::env::temp_dir().join(format!("gba-desktop-cli-{}", std::process::id()));
    fs::create_dir(&directory).expect("new fixture directory");
    let bios = directory.join("bios.bin");
    let rom = directory.join("game.gba");
    fs::write(&bios, [0; 16384]).expect("synthetic BIOS");
    fs::write(&rom, [0; 192]).expect("synthetic ROM");
    let output = Command::new(env!("CARGO_BIN_EXE_gba-desktop"))
        .arg("--bios")
        .arg(&bios)
        .arg("--rom")
        .arg(&rom)
        .env("XDG_DATA_HOME", directory.join("xdg-data"))
        .env("XDG_CONFIG_HOME", directory.join("xdg-config"))
        .env_remove("DISPLAY")
        .output()
        .expect("run without display");
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("cannot open X11 display"));
    assert!(error.contains("gba-headless --frames"));
    assert!(!directory.join("game.sav").exists());
    fs::remove_dir_all(directory).expect("remove fixtures");
}
