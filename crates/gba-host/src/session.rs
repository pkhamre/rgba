use gba_core::{MAX_SAVE_STATE_SIZE, Machine};
use std::error::Error;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};

/// A machine and its disk-backed cartridge save. No emulator save states.
#[derive(Debug)]
pub struct Session {
    machine: Machine,
    save_path: PathBuf,
    state_path: PathBuf,
    title: String,
}

impl Session {
    /// Loads BIOS/ROM and an optional existing backup, rejecting invalid sizes
    /// and save paths that would overwrite either input image.
    pub fn try_new(
        bios_path: &Path,
        rom_path: &Path,
        save_path: Option<&Path>,
    ) -> Result<Self, Box<dyn Error>> {
        let bios = fs::read(bios_path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("BIOS {}: {error}", bios_path.display()),
            )
        })?;
        let rom = fs::read(rom_path).map_err(|error| {
            io::Error::new(error.kind(), format!("ROM {}: {error}", rom_path.display()))
        })?;
        let mut machine = Machine::try_new(&bios, &rom)?;
        let save_path = save_path
            .map(Path::to_owned)
            .unwrap_or_else(|| rom_path.with_extension("sav"));
        let state_path = rom_path.with_extension("state");
        let bios_canonical = bios_path.canonicalize()?;
        let rom_canonical = rom_path.canonicalize()?;
        let save_candidates = [
            save_path.clone(),
            sidecar(&save_path, ".bak"),
            sidecar(&save_path, ".tmp"),
            sidecar(&save_path, ".bak.tmp"),
        ];
        let state_candidates = [
            state_path.clone(),
            sidecar(&state_path, ".tmp"),
            sidecar(&state_path, ".bak"),
            sidecar(&state_path, ".bak.tmp"),
        ];
        for candidate in save_candidates.iter().chain(state_candidates.iter()) {
            if let Ok(canonical) = candidate.canonicalize()
                && (canonical == bios_canonical || canonical == rom_canonical)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "save, state, and sidecar paths must not overwrite BIOS or ROM",
                )
                .into());
            }
        }
        for state_candidate in &state_candidates {
            let aliases_save = save_candidates
                .iter()
                .any(|candidate| paths_alias(state_candidate, candidate));
            if aliases_save {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "save-state path and sidecars must not alias cartridge-save paths",
                )
                .into());
            }
        }
        match fs::read(&save_path) {
            Ok(bytes) => machine.load_save(&bytes)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("save {}: {error}", save_path.display()),
                )
                .into());
            }
        }
        let title = String::from_utf8_lossy(&rom[0xa0..0xac])
            .trim_matches(char::from(0))
            .to_owned();
        Ok(Self {
            machine,
            save_path,
            state_path,
            title,
        })
    }

    /// Current machine state.
    #[must_use]
    pub fn machine(&self) -> &Machine {
        &self.machine
    }

    /// Mutable machine access for frame execution and input.
    #[must_use]
    pub fn machine_mut(&mut self) -> &mut Machine {
        &mut self.machine
    }

    /// Cartridge title from the validated ROM header.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Location of the in-game Flash/SRAM save.
    #[must_use]
    pub fn save_path(&self) -> &Path {
        &self.save_path
    }

    /// Location of the single emulator save-state slot.
    #[must_use]
    pub fn state_path(&self) -> &Path {
        &self.state_path
    }

    /// Atomically writes the current complete machine state to the quick slot.
    pub fn save_state(&self) -> Result<(), Box<dyn Error>> {
        let bytes = self.machine.save_state()?;
        atomic_write(&self.state_path, &bytes)?;
        Ok(())
    }

    /// Loads and validates the quick slot, synchronizing its cartridge backup
    /// to the configured `.sav` before replacing the running machine.
    pub fn load_state(&mut self) -> Result<(), Box<dyn Error>> {
        let length = fs::metadata(&self.state_path)?.len();
        if length > MAX_SAVE_STATE_SIZE as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "save state exceeds size limit",
            )
            .into());
        }
        let bytes = fs::read(&self.state_path)?;
        let mut candidate = self.machine.clone();
        candidate.load_state(&bytes)?;
        let cartridge = candidate.save_data();
        let previous = match fs::read(&self.save_path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        if previous.as_deref() != Some(cartridge) {
            if let Some(previous) = previous {
                atomic_write(&sidecar(&self.save_path, ".bak"), &previous)?;
            }
            atomic_write(&self.save_path, cartridge)?;
        }
        candidate.mark_save_clean();
        self.machine = candidate;
        Ok(())
    }

    /// Atomically saves changed backup data and preserves the prior file in
    /// .bak. Dirty state is cleared only after all writes succeed.
    pub fn flush_save(&mut self) -> Result<bool, io::Error> {
        if !self.machine.save_dirty() {
            return Ok(false);
        }
        match fs::read(&self.save_path) {
            Ok(previous) => atomic_write(&sidecar(&self.save_path, ".bak"), &previous)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        atomic_write(&self.save_path, self.machine.save_data())?;
        self.machine.mark_save_clean();
        Ok(true)
    }
}

fn paths_alias(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), io::Error> {
    use std::io::Write;
    let temporary = sidecar(path, ".tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{Session, sidecar};
    use gba_core::MAX_SAVE_STATE_SIZE;
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    struct Assets {
        directory: PathBuf,
    }
    impl Assets {
        fn new() -> Self {
            let directory = env::temp_dir().join(format!(
                "gba-save-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&directory).expect("unique test directory");
            let mut bios = vec![0; 0x4000];
            for (index, opcode) in [0xe3a0_040eu32, 0xe3a0_102a, 0xe5c0_1000, 0xeaff_fffe]
                .into_iter()
                .enumerate()
            {
                bios[index * 4..index * 4 + 4].copy_from_slice(&opcode.to_le_bytes());
            }
            fs::write(directory.join("bios.bin"), bios).expect("synthetic BIOS");
            fs::write(directory.join("game.gba"), [0; 192]).expect("synthetic ROM");
            Self { directory }
        }
        fn session(&self) -> Session {
            Session::try_new(
                &self.directory.join("bios.bin"),
                &self.directory.join("game.gba"),
                None,
            )
            .expect("valid assets")
        }
    }
    impl Drop for Assets {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    #[test]
    fn cartridge_save_round_trip_and_backup() {
        let assets = Assets::new();
        let mut session = assets.session();
        assert_eq!(session.machine().save_revision(), 0);
        for _ in 0..3 {
            session
                .machine_mut()
                .step()
                .expect("write SRAM through CPU");
        }
        assert_eq!(session.machine().save_data()[0], 42);
        assert_eq!(session.machine().save_revision(), 1);
        assert!(session.flush_save().expect("atomic first save"));
        assert_eq!(
            session.machine().save_revision(),
            1,
            "flushing is not a guest write"
        );
        assert!(!session.flush_save().expect("clean save does not rewrite"));
        let bytes = fs::read(session.save_path()).expect("on-disk backup");
        let restored = assets.session();
        assert_eq!(restored.machine().save_data(), bytes);
        let bios_path = assets.directory.join("bios.bin");
        let mut bios = fs::read(&bios_path).expect("synthetic BIOS");
        bios[4..8].copy_from_slice(&0xe3a0_1063u32.to_le_bytes());
        fs::write(bios_path, bios).expect("new synthetic write value");
        let mut newer = assets.session();
        for _ in 0..3 {
            newer.machine_mut().step().expect("SRAM write");
        }
        newer.flush_save().expect("replacement save");
        assert_eq!(
            fs::read(assets.directory.join("game.sav.bak")).expect("backup"),
            bytes
        );
    }

    #[test]
    fn quick_state_round_trip_syncs_cartridge_save_and_preserves_backup() {
        let assets = Assets::new();
        let mut session = assets.session();
        for _ in 0..3 {
            session.machine_mut().step().expect("SRAM write");
        }
        assert_eq!(session.machine().save_data()[0], 42);
        let saved_clocks = session.machine().clocks();
        let expected_save = session.machine().save_data().to_vec();
        session.save_state().expect("atomic state write");
        assert!(session.state_path().exists());
        assert!(
            fs::metadata(session.state_path())
                .expect("state file")
                .len()
                < MAX_SAVE_STATE_SIZE as u64
        );

        let previous_save = vec![0x55; expected_save.len()];
        fs::write(session.save_path(), &previous_save).expect("newer cartridge save");
        session.machine_mut().step().expect("advance after state");
        assert_ne!(session.machine().clocks(), saved_clocks);
        session.load_state().expect("load quick state");
        assert_eq!(session.machine().clocks(), saved_clocks);
        assert_eq!(session.machine().save_data(), expected_save);
        assert!(!session.machine().save_dirty());
        assert_eq!(
            fs::read(session.save_path()).expect("synced save"),
            expected_save
        );
        assert_eq!(
            fs::read(sidecar(session.save_path(), ".bak")).expect("backup"),
            previous_save
        );
    }

    #[test]
    fn invalid_quick_state_leaves_machine_and_cartridge_file_unchanged() {
        let assets = Assets::new();
        let mut session = assets.session();
        session.save_state().expect("state file");
        session.machine_mut().step().expect("advance");
        let clocks = session.machine().clocks();
        let save_before = fs::read(session.save_path()).expect_err("no save yet");
        let mut corrupt = fs::read(session.state_path()).expect("state");
        *corrupt.last_mut().expect("payload") ^= 1;
        fs::write(session.state_path(), corrupt).expect("corrupt fixture");
        assert!(session.load_state().is_err());
        assert_eq!(session.machine().clocks(), clocks);
        assert_eq!(
            fs::read(session.save_path())
                .expect_err("save still absent")
                .kind(),
            save_before.kind()
        );
    }

    #[test]
    fn state_slot_cannot_alias_configured_cartridge_save() {
        let assets = Assets::new();
        let rom = assets.directory.join("game.gba");
        assert!(
            Session::try_new(
                &assets.directory.join("bios.bin"),
                &rom,
                Some(&assets.directory.join("game.state")),
            )
            .is_err()
        );
    }

    #[test]
    fn invalid_save_and_input_overwrite_are_rejected() {
        let assets = Assets::new();
        fs::write(assets.directory.join("game.sav"), b"bad").expect("bad fixture");
        assert!(
            Session::try_new(
                &assets.directory.join("bios.bin"),
                &assets.directory.join("game.gba"),
                None
            )
            .is_err()
        );
        assert!(
            Session::try_new(
                &assets.directory.join("bios.bin"),
                &assets.directory.join("game.gba"),
                Some(&assets.directory.join("game.gba"))
            )
            .is_err()
        );
    }

    #[test]
    fn backup_and_temporary_paths_cannot_alias_rom_inputs() {
        let assets = Assets::new();
        let save = assets.directory.join("protected.sav");
        for suffix in [".bak", ".tmp", ".bak.tmp"] {
            let protected = super::sidecar(&save, suffix);
            fs::copy(assets.directory.join("game.gba"), &protected).expect("ROM fixture");
            let before = fs::read(&protected).expect("original ROM");
            assert!(
                Session::try_new(&assets.directory.join("bios.bin"), &protected, Some(&save))
                    .is_err_and(|error| error.to_string().contains("must not overwrite")),
                "sidecar alias must be rejected"
            );
            assert_eq!(fs::read(&protected).expect("protected ROM"), before);
            fs::remove_file(protected).expect("remove fixture");
        }
    }

    #[test]
    fn failed_write_preserves_dirty_state_and_existing_data() {
        let assets = Assets::new();
        let mut session = assets.session();
        for _ in 0..3 {
            session.machine_mut().step().expect("SRAM write");
        }
        fs::write(assets.directory.join("game.sav.tmp"), b"do not overwrite")
            .expect("stale temporary fixture");
        assert!(session.flush_save().is_err());
        assert!(session.machine().save_dirty());
        assert_eq!(
            fs::read(assets.directory.join("game.sav.tmp")).expect("stale file remains"),
            b"do not overwrite"
        );
    }
}
