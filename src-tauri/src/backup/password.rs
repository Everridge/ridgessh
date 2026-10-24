//! Where the backup password is remembered: the OS credential store (Windows Credential
//! Manager, macOS Keychain). Tests use an in-memory store.

use crate::error::{AppError, AppResult};

pub const MIN_LENGTH: usize = 8;

pub trait SecretStore: Send + Sync {
    fn get(&self) -> AppResult<Option<String>>;
    fn set(&self, secret: &str) -> AppResult<()>;
    fn delete(&self) -> AppResult<()>;
}

pub fn check_strength(password: &str) -> AppResult<()> {
    if password.chars().count() < MIN_LENGTH {
        return Err(AppError::Validation(format!(
            "the backup password needs at least {MIN_LENGTH} characters"
        )));
    }
    Ok(())
}

/// The OS credential store, entry "RidgeSSH" / "backup-password".
pub struct KeyringStore;

const SERVICE: &str = "RidgeSSH";
const USER: &str = "backup-password";

fn keyring_err(e: keyring::Error) -> AppError {
    AppError::Other(format!("credential store: {e}"))
}

impl KeyringStore {
    fn entry() -> AppResult<keyring::Entry> {
        keyring::Entry::new(SERVICE, USER).map_err(keyring_err)
    }
}

impl SecretStore for KeyringStore {
    fn get(&self) -> AppResult<Option<String>> {
        match Self::entry()?.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_err(e)),
        }
    }

    fn set(&self, secret: &str) -> AppResult<()> {
        Self::entry()?.set_password(secret).map_err(keyring_err)
    }

    fn delete(&self) -> AppResult<()> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keyring_err(e)),
        }
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(std::sync::Mutex<Option<String>>);

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self) -> AppResult<Option<String>> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn set(&self, secret: &str) -> AppResult<()> {
        *self.0.lock().unwrap() = Some(secret.to_string());
        Ok(())
    }
    fn delete(&self) -> AppResult<()> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}
