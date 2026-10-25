//! End-to-end CLI checks using synthetic assets, never proprietary images.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct RunDirectory {
    path: PathBuf,
}

impl RunDirectory {
    fn new() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("gba-cli-{}-{sequence}", std::process::id()));
        fs::create_dir(&path).expect("fresh test directory");
        let mut bios = vec![0; 0x4000];
        // Reset: B . (an infinite loop that must respect the instruction budget).
        bios[..4].copy_from_slice(&0xeaff_fffeu32.to_le_bytes());
        fs::write(path.join("bios.bin"), bios).expect("write synthetic BIOS");
        fs::write(path.join("game.gba"), [0; 192]).expect("write synthetic ROM");
        Self { path }
    }

    fn run(&self, options: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gba-headless"))
            .current_dir(&self.path)
            .args(["--bios", "bios.bin", "--rom", "game.gba"])
            .args(options)
            .output()
            .expect("headless executable launches")
    }
}

impl Drop for RunDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn instruction_budget_bounds_infinite_guest_loop() {
    let directory = RunDirectory::new();
    let output = directory.run(&["--steps", "7", "--trace", "run.trace"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary = String::from_utf8_lossy(&output.stderr);
    assert!(summary.contains("completed=7"));
    let trace = fs::read_to_string(directory.path.join("run.trace")).expect("trace artifact");
    assert_eq!(trace.lines().count(), 7);
    assert!(trace.lines().all(|line| line.starts_with("v1 ")));
}

#[test]
fn trace_creation_never_overwrites_existing_files_or_inputs() {
    let directory = RunDirectory::new();
    fs::write(directory.path.join("existing.trace"), b"preserve me").expect("test artifact");
    let output = directory.run(&["--steps", "1", "--trace", "existing.trace"]);
    assert!(!output.status.success());
    assert_eq!(
        fs::read(directory.path.join("existing.trace")).expect("preserved file"),
        b"preserve me"
    );
    let before = fs::read(directory.path.join("bios.bin")).expect("BIOS");
    let output = directory.run(&["--steps", "1", "--trace", "bios.bin"]);
    assert!(!output.status.success());
    assert_eq!(
        fs::read(directory.path.join("bios.bin")).expect("BIOS"),
        before
    );
}

#[test]
fn invalid_asset_sizes_and_missing_paths_report_context() {
    let directory = RunDirectory::new();
    fs::write(directory.path.join("bios.bin"), [0; 3]).expect("invalid BIOS fixture");
    let output = directory.run(&["--steps", "1"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("BIOS must be 16384 bytes, got 3"));
    fs::remove_file(directory.path.join("bios.bin")).expect("remove fixture");
    let output = directory.run(&["--steps", "1"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("BIOS bios.bin:"));
}

#[test]
fn unreachable_stop_address_returns_failure_not_success() {
    let directory = RunDirectory::new();
    let output = directory.run(&["--steps", "2", "--stop-at", "08000000"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("budget exhausted before target"));
    let output = directory.run(&["--steps", "2", "--stop-at", "0"]);
    assert!(
        output.status.success(),
        "initial address already matches target"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("completed=0"));
}

#[test]
fn unsupported_io_returns_a_terminal_error_with_instruction_location() {
    let directory = RunDirectory::new();
    let mut bios = vec![0; 0x4000];
    for (index, opcode) in [0xe3a0_0301u32, 0xe590_1400].into_iter().enumerate() {
        bios[index * 4..index * 4 + 4].copy_from_slice(&opcode.to_le_bytes());
    }
    fs::write(directory.path.join("bios.bin"), bios).expect("I/O program fixture");
    let output = directory.run(&["--steps", "10"]);
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("next=00000004"));
    assert!(message.contains("unimplemented I/O read at 04000400"));
}

#[test]
fn benchmark_repeats_without_touching_cartridge_saves() {
    let directory = RunDirectory::new();
    // Even an invalid adjacent save must not influence a benchmark.
    fs::write(directory.path.join("game.sav"), b"do not load or overwrite").expect("existing save");
    let output = directory.run(&[
        "--benchmark",
        "--frames",
        "3",
        "--warmup-frames",
        "1",
        "--repeats",
        "2",
        "--input",
        "0:a",
        "--input",
        "1:none",
        "--screenshot",
        "final.ppm",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(log.contains("repeat=1 "));
    assert!(log.contains("repeat=2 "));
    assert!(log.contains("deterministic=true"));
    assert!(log.contains("realtime_x="));
    assert_eq!(
        fs::read(directory.path.join("game.sav")).expect("save"),
        b"do not load or overwrite"
    );
    assert!(directory.path.join("final.ppm").exists());
    fs::remove_file(directory.path.join("game.sav")).expect("remove save");
    let output = directory.run(&[
        "--benchmark",
        "--frames",
        "1",
        "--warmup-frames",
        "0",
        "--repeats",
        "1",
    ]);
    assert!(output.status.success());
    assert!(!directory.path.join("game.sav").exists());
}

#[test]
fn benchmark_loads_state_and_rejects_invalid_state() {
    let directory = RunDirectory::new();
    let bios = fs::read(directory.path.join("bios.bin")).expect("BIOS");
    let rom = fs::read(directory.path.join("game.gba")).expect("ROM");
    let mut machine = gba_core::Machine::try_new(&bios, &rom).expect("machine");
    machine.run_frame().expect("advance snapshot");
    let state = machine.save_state().expect("state");
    fs::write(directory.path.join("start.state"), &state).expect("write state");
    let output = directory.run(&[
        "--benchmark",
        "--frames",
        "2",
        "--warmup-frames",
        "0",
        "--state",
        "start.state",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(directory.path.join("start.state")).expect("state"),
        state
    );
    fs::write(directory.path.join("start.state"), b"invalid state").expect("corrupt state");
    let output = directory.run(&["--benchmark", "--frames", "1", "--state", "start.state"]);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("benchmark_complete"));
}

#[test]
fn benchmark_fault_is_not_reported_as_success() {
    let directory = RunDirectory::new();
    let mut bios = vec![0; 0x4000];
    for (index, opcode) in [0xe3a0_0301u32, 0xe590_1400].into_iter().enumerate() {
        bios[index * 4..index * 4 + 4].copy_from_slice(&opcode.to_le_bytes());
    }
    fs::write(directory.path.join("bios.bin"), bios).expect("unsupported I/O fixture");
    let output = directory.run(&["--benchmark", "--frames", "1", "--warmup-frames", "0"]);
    assert!(!output.status.success());
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(log.contains("benchmark frame 0, PC 00000004"));
    assert!(!log.contains("benchmark_complete"));
}

#[test]
fn help_needs_no_assets() {
    let output = Command::new(env!("CARGO_BIN_EXE_gba-headless"))
        .arg("--help")
        .output()
        .expect("runner launches");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("sound is emulated, not played"));
}
