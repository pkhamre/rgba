//! Asset-free desktop CLI and missing-display regressions.

use std::fs;
use std::process::Command;

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
