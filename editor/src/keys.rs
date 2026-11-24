//! Host-side signing key persistence (`crates/secrets` or in-memory for tests).
use nanatype_theme::SealIdentity;
use std::{path::Path, sync::Mutex};

/// Where the host keeps the package signing key. The key never enters a
/// package, a log, the resource interface or a plugin process.
pub trait KeyStore: Send + Sync {
    fn load(&self) -> Result<Option<SealIdentity>, String>;
    fn save(&self, identity: &SealIdentity) -> Result<(), String>;
}

/// The platform's protected storage (`crates/secrets`).
pub struct SecretKeys(nanatype_secrets::SecretStore);

impl SecretKeys {
    pub fn new(directory: &Path) -> Self {
        Self(nanatype_secrets::SecretStore::new(
            directory,
            "package-signing",
        ))
    }
}

impl KeyStore for SecretKeys {
    fn load(&self) -> Result<Option<SealIdentity>, String> {
        match self.0.load().map_err(|e| e.to_string())? {
            None => Ok(None),
            Some(bytes) => <[u8; 32]>::try_from(bytes.as_slice())
                .map(|b| Some(SealIdentity::from_bytes(b)))
                .map_err(|_| "stored signing key is malformed".to_owned()),
        }
    }

    fn save(&self, identity: &SealIdentity) -> Result<(), String> {
        self.0
            .save(&identity.secret_bytes())
            .map_err(|e| e.to_string())
    }
}

/// In-memory keys, for tests and platforms without protected storage.
#[derive(Default)]
pub struct MemoryKeys(Mutex<Option<[u8; 32]>>);

impl KeyStore for MemoryKeys {
    fn load(&self) -> Result<Option<SealIdentity>, String> {
        Ok(self
            .0
            .lock()
            .map_err(|e| e.to_string())?
            .map(SealIdentity::from_bytes))
    }

    fn save(&self, identity: &SealIdentity) -> Result<(), String> {
        *self.0.lock().map_err(|e| e.to_string())? = Some(identity.secret_bytes());
        Ok(())
    }
}
