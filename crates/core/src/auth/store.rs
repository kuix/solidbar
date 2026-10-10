//! Where the OAuth tokens live between launches.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{AuthError, TokenSet};

pub trait TokenStore: Send + Sync {
    fn load(&self, account: &str) -> Result<Option<TokenSet>, AuthError>;
    fn save(&self, account: &str, tokens: &TokenSet) -> Result<(), AuthError>;
    fn clear(&self, account: &str) -> Result<(), AuthError>;
}

/// The login keychain; used by release builds.
pub struct KeychainTokenStore {
    service: String,
}

impl KeychainTokenStore {
    /// `service` is the bundle identifier, so the items are easy to find in Keychain Access.
    pub fn new(service: &str) -> Result<Self, AuthError> {
        let store = apple_native_keyring_store::keychain::Store::new().map_err(store_error)?;
        keyring_core::set_default_store(store);
        Ok(Self {
            service: service.to_owned(),
        })
    }

    fn entry(&self, account: &str) -> Result<keyring_core::Entry, AuthError> {
        keyring_core::Entry::new(&self.service, account).map_err(store_error)
    }
}

impl TokenStore for KeychainTokenStore {
    fn load(&self, account: &str) -> Result<Option<TokenSet>, AuthError> {
        match self.entry(account)?.get_password() {
            Ok(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|error| AuthError::Store(error.to_string())),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(store_error(error)),
        }
    }

    fn save(&self, account: &str, tokens: &TokenSet) -> Result<(), AuthError> {
        let json =
            serde_json::to_string(tokens).map_err(|error| AuthError::Store(error.to_string()))?;
        self.entry(account)?
            .set_password(&json)
            .map_err(store_error)
    }

    fn clear(&self, account: &str) -> Result<(), AuthError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(store_error(error)),
        }
    }
}

fn store_error(error: keyring_core::Error) -> AuthError {
    AuthError::Store(error.to_string())
}

/// A JSON file readable only by the user. Debug builds use it because every rebuild changes
/// the code signature, which would make the keychain ask for access each time.
pub struct FileTokenStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileTokenStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    fn read_all(&self) -> HashMap<String, TokenSet> {
        std::fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn write_all(&self, all: &HashMap<String, TokenSet>) -> Result<(), AuthError> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let to_error = |error: std::io::Error| AuthError::Store(error.to_string());
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(to_error)?;
        }
        let json = serde_json::to_vec(all).map_err(|error| AuthError::Store(error.to_string()))?;
        let temporary = self.path.with_extension("tmp");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(to_error)?;
        file.write_all(&json).map_err(to_error)?;
        std::fs::rename(&temporary, &self.path).map_err(to_error)
    }
}

impl TokenStore for FileTokenStore {
    fn load(&self, account: &str) -> Result<Option<TokenSet>, AuthError> {
        let _guard = self.lock.lock().unwrap();
        Ok(self.read_all().remove(account))
    }

    fn save(&self, account: &str, tokens: &TokenSet) -> Result<(), AuthError> {
        let _guard = self.lock.lock().unwrap();
        let mut all = self.read_all();
        all.insert(account.to_owned(), tokens.clone());
        self.write_all(&all)
    }

    fn clear(&self, account: &str) -> Result<(), AuthError> {
        let _guard = self.lock.lock().unwrap();
        let mut all = self.read_all();
        if all.remove(account).is_some() {
            self.write_all(&all)?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct MemoryTokenStore {
    tokens: Mutex<HashMap<String, TokenSet>>,
}

impl TokenStore for MemoryTokenStore {
    fn load(&self, account: &str) -> Result<Option<TokenSet>, AuthError> {
        Ok(self.tokens.lock().unwrap().get(account).cloned())
    }

    fn save(&self, account: &str, tokens: &TokenSet) -> Result<(), AuthError> {
        self.tokens
            .lock()
            .unwrap()
            .insert(account.to_owned(), tokens.clone());
        Ok(())
    }

    fn clear(&self, account: &str) -> Result<(), AuthError> {
        self.tokens.lock().unwrap().remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tokens(access: &str) -> TokenSet {
        TokenSet {
            access_token: access.to_owned(),
            refresh_token: "refresh".to_owned(),
            expires_at: "2026-10-09T11:00:00Z".parse().unwrap(),
        }
    }

    #[test]
    fn file_store_round_trips_with_private_permissions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested").join("session.json");
        let store = FileTokenStore::new(path.clone());
        assert_eq!(store.load("a").unwrap(), None);

        store.save("a", &tokens("one")).unwrap();
        store.save("b", &tokens("two")).unwrap();
        assert_eq!(store.load("a").unwrap(), Some(tokens("one")));
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        store.clear("a").unwrap();
        assert_eq!(store.load("a").unwrap(), None);
        assert_eq!(store.load("b").unwrap(), Some(tokens("two")));
    }

    #[test]
    fn a_corrupt_file_reads_as_signed_out() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(FileTokenStore::new(path).load("a").unwrap(), None);
    }
}
