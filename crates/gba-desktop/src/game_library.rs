//! XDG paths and top-level ROM discovery for the desktop game library.

use sha2::{Digest, Sha256};
use std::env;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const ROM_EXTENSIONS: [&str; 3] = ["gba", "agb", "bin"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Game {
    pub(crate) path: PathBuf,
    pub(crate) identity: [u8; 32],
    pub(crate) title: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StoragePaths {
    pub(crate) data: PathBuf,
    pub(crate) config: PathBuf,
    pub(crate) games: PathBuf,
    pub(crate) saves: PathBuf,
    pub(crate) states: PathBuf,
    pub(crate) settings: PathBuf,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct MigrationReport {
    pub(crate) copied: Vec<PathBuf>,
    pub(crate) conflicts: Vec<PathBuf>,
}

impl StoragePaths {
    pub(crate) fn from_env() -> io::Result<Self> {
        let home = env::var_os("HOME").map(PathBuf::from);
        let data_home = absolute_env_path("XDG_DATA_HOME")
            .or_else(|| home.as_ref().map(|home| home.join(".local/share")))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
        let config_home = absolute_env_path("XDG_CONFIG_HOME")
            .or_else(|| home.as_ref().map(|home| home.join(".config")))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
        let data = data_home.join("rgba");
        let config = config_home.join("rgba");
        Ok(Self {
            games: data.join("games"),
            saves: data.join("saves"),
            states: data.join("states"),
            settings: config.join("settings"),
            data,
            config,
        })
    }

    pub(crate) fn create(&self) -> io::Result<()> {
        for path in [&self.games, &self.saves, &self.states, &self.settings] {
            fs::create_dir_all(path)?;
        }
        Ok(())
    }

    pub(crate) fn save_path(&self, identity: &[u8; 32]) -> PathBuf {
        self.saves.join(format!("{}.sav", hex(identity)))
    }

    pub(crate) fn state_path(&self, identity: &[u8; 32]) -> PathBuf {
        self.states.join(format!("{}.state", hex(identity)))
    }

    pub(crate) fn settings_path(&self, identity: &[u8; 32]) -> PathBuf {
        self.settings.join(format!("{}.settings", hex(identity)))
    }
}

pub(crate) fn scan(paths: &StoragePaths) -> io::Result<Vec<Game>> {
    let mut entries = fs::read_dir(&paths.games)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::path);
    let mut games = Vec::new();
    for entry in entries {
        let path = entry.path();
        if !entry.file_type()?.is_file() || !is_rom_path(&path) {
            continue;
        }
        let game = match game_from_path(&path) {
            Ok(game) => game,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => continue,
            Err(error) => return Err(error),
        };
        if games
            .iter()
            .any(|existing: &Game| existing.identity == game.identity)
        {
            continue;
        }
        games.push(game);
    }
    games.sort_by(|left, right| {
        left.title
            .to_lowercase()
            .cmp(&right.title.to_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(games)
}

pub(crate) fn game_from_path(path: &Path) -> io::Result<Game> {
    let bytes = fs::read(path)?;
    if bytes.len() < 0xac {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ROM header is too short",
        ));
    }
    let identity: [u8; 32] = Sha256::digest(&bytes).into();
    let title = path
        .file_stem()
        .unwrap_or_else(|| OsStr::new("UNKNOWN GAME"))
        .to_string_lossy()
        .into_owned();
    Ok(Game {
        path: path.to_owned(),
        identity,
        title,
    })
}

pub(crate) fn migrate_legacy(
    rom_path: &Path,
    identity: &[u8; 32],
    paths: &StoragePaths,
) -> io::Result<MigrationReport> {
    let save_path = paths.save_path(identity);
    let mut central_backup = save_path.as_os_str().to_os_string();
    central_backup.push(".bak");
    let mut legacy_backup = rom_path.with_extension("sav").into_os_string();
    legacy_backup.push(".bak");
    let destinations = [
        (rom_path.with_extension("sav"), save_path),
        (PathBuf::from(legacy_backup), PathBuf::from(central_backup)),
        (rom_path.with_extension("state"), paths.state_path(identity)),
        (
            rom_path.with_extension("settings"),
            paths.settings_path(identity),
        ),
    ];
    let mut report = MigrationReport::default();
    for (source, destination) in destinations {
        match fs::read(&source) {
            Ok(bytes) => match copy_if_missing(&destination, &bytes)? {
                true => report.copied.push(source),
                false if fs::read(&destination)? != bytes => report.conflicts.push(source),
                false => {}
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(report)
}

fn copy_if_missing(path: &Path, bytes: &[u8]) -> io::Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = sidecar(path, ".migrate.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        match fs::hard_link(&temporary, path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(error),
        }
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn is_rom_path(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            ROM_EXTENSIONS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

fn absolute_env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::{Game, StoragePaths, migrate_legacy, scan};
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        paths: StoragePaths,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "rgba-library-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let paths = StoragePaths {
                data: root.join("data"),
                config: root.join("config"),
                games: root.join("data/games"),
                saves: root.join("data/saves"),
                states: root.join("data/states"),
                settings: root.join("config/settings"),
            };
            paths.create().expect("create paths");
            Self { root, paths }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn rom(title: &[u8], marker: u8) -> Vec<u8> {
        let mut bytes = vec![marker; 0xc0];
        bytes[0xa0..0xac].fill(0);
        let length = title.len().min(12);
        bytes[0xa0..0xa0 + length].copy_from_slice(&title[..length]);
        bytes
    }

    #[test]
    fn scan_is_top_level_sorted_and_deduplicates_content_hashes() {
        let fixture = Fixture::new();
        let first = rom(b"ZELDA", 1);
        fs::write(fixture.paths.games.join("z.gba"), &first).expect("ROM");
        fs::write(fixture.paths.games.join("copy.GBA"), &first).expect("duplicate");
        fs::write(fixture.paths.games.join("a.agb"), rom(b"HEADER", 2)).expect("ROM");
        fs::create_dir(fixture.paths.games.join("nested")).expect("subfolder");
        fs::write(
            fixture.paths.games.join("nested/ignored.gba"),
            rom(b"NESTED", 3),
        )
        .expect("nested ROM");
        fs::write(fixture.paths.games.join("notes.txt"), b"not a ROM").expect("notes");
        let games = scan(&fixture.paths).expect("scan");
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].title, "a");
        assert_eq!(games[1].title, "copy");
        assert!(games[1].path.ends_with("copy.GBA"));
        assert_eq!(games[1].identity, <[u8; 32]>::from(Sha256::digest(&first)));
    }

    #[test]
    fn migration_copies_missing_files_and_keeps_central_conflicts() {
        let fixture = Fixture::new();
        let rom_path = fixture.root.join("game.gba");
        fs::write(&rom_path, rom(b"TEST", 4)).expect("ROM");
        let identity = [7; 32];
        let legacy_save = rom_path.with_extension("sav");
        fs::write(&legacy_save, b"legacy save").expect("legacy save");
        let central_state = fixture.paths.state_path(&identity);
        fs::write(&central_state, b"central state").expect("central state");
        fs::write(rom_path.with_extension("state"), b"legacy state").expect("legacy state");
        let report = migrate_legacy(&rom_path, &identity, &fixture.paths).expect("migration");
        assert_eq!(report.copied.as_slice(), std::slice::from_ref(&legacy_save));
        assert_eq!(report.conflicts, [rom_path.with_extension("state")]);
        assert_eq!(
            fs::read(fixture.paths.save_path(&identity)).expect("central save"),
            b"legacy save"
        );
        assert_eq!(
            fs::read(legacy_save).expect("original retained"),
            b"legacy save"
        );
        assert_eq!(
            fs::read(central_state).expect("central wins"),
            b"central state"
        );
    }

    #[test]
    fn scan_uses_filename_even_when_rom_has_a_short_header_title() {
        let fixture = Fixture::new();
        let path = fixture.paths.games.join("Pokemon - Emerald Version.gba");
        let bytes = rom(b"POKEMON EMER", 8);
        fs::write(&path, &bytes).expect("ROM");
        let games = scan(&fixture.paths).expect("scan");
        assert_eq!(
            games,
            [Game {
                path,
                identity: Sha256::digest(bytes).into(),
                title: "Pokemon - Emerald Version".to_owned(),
            }]
        );
    }
}
