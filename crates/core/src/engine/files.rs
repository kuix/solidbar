//! Settings and the data cache on disk, written atomically (temporary file, then rename).

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use solidbar_shared::settings::Settings;

use super::state::Data;

/// Bump when [`Data`] changes incompatibly; older caches are then ignored.
const CACHE_VERSION: u32 = 1;

pub(crate) struct Files {
    settings: PathBuf,
    cache: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Cache {
    version: u32,
    account: String,
    data: Data,
}

impl Files {
    pub fn new(directory: &Path) -> Self {
        Self {
            settings: directory.join("settings.json"),
            cache: directory.join("cache.json"),
        }
    }

    pub fn load_settings(&self) -> Settings {
        read_json(&self.settings).unwrap_or_default()
    }

    pub fn save_settings(&self, settings: &Settings) {
        write_json(&self.settings, settings);
    }

    /// The cached data, if it belongs to `account` and has the current format.
    pub fn load_cache(&self, account: &str) -> Option<Data> {
        let cache: Cache = read_json(&self.cache)?;
        (cache.version == CACHE_VERSION && cache.account == account).then_some(cache.data)
    }

    pub fn save_cache(&self, account: &str, data: &Data) {
        let cache = Cache {
            version: CACHE_VERSION,
            account: account.to_owned(),
            data: data.clone(),
        };
        write_json(&self.cache, &cache);
    }

    pub fn clear_cache(&self) {
        let _ = std::fs::remove_file(&self.cache);
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(value) => Some(value),
        Err(error) => {
            log::warn!("ignoring unreadable {}: {error}", path.display());
            None
        }
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) {
    let result = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("tmp");
        // Only this user: the settings hold the key agents use.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(value)?)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if let Err(error) = result {
        log::warn!("could not write {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidbar_shared::settings::Theme;

    #[test]
    fn settings_round_trip_and_default_when_missing() {
        let directory = tempfile::tempdir().unwrap();
        let files = Files::new(directory.path());
        assert_eq!(files.load_settings(), Settings::default());
        let settings = Settings {
            theme: Theme::Light,
            ..Settings::default()
        };
        files.save_settings(&settings);
        assert_eq!(files.load_settings().theme, Theme::Light);
        let mode = std::fs::metadata(directory.path().join("settings.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn the_cache_belongs_to_one_account() {
        let directory = tempfile::tempdir().unwrap();
        let files = Files::new(directory.path());
        files.save_cache("a", &Data::default());
        assert!(files.load_cache("a").is_some());
        assert!(files.load_cache("b").is_none());
        files.clear_cache();
        assert!(files.load_cache("a").is_none());
    }

    #[test]
    fn a_corrupt_cache_is_ignored() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("cache.json"), b"[1, 2").unwrap();
        assert!(Files::new(directory.path()).load_cache("a").is_none());
    }
}
