//! Platform protected storage. Tokens never touch a plain file.
use nanatype_secrets::SecretStore;
use std::path::Path;

pub(crate) enum Store {
    Secret(SecretStore),
    #[cfg(test)]
    Memory(std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>>),
}
impl Store {
    pub fn new(directory: &Path) -> Self {
        Self::Secret(SecretStore::new(directory, "twitch"))
    }
    pub fn load(&self) -> Result<Option<Vec<u8>>, String> {
        match self {
            Self::Secret(store) => store.load().map_err(|_| storage_error()),
            #[cfg(test)]
            Self::Memory(slot) => Ok(slot.lock().unwrap().clone()),
        }
    }
    pub fn save(&self, bytes: &[u8]) -> Result<(), String> {
        match self {
            Self::Secret(store) => store.save(bytes).map_err(|_| storage_error()),
            #[cfg(test)]
            Self::Memory(slot) => {
                *slot.lock().unwrap() = Some(bytes.to_vec());
                Ok(())
            }
        }
    }
    pub fn clear(&self) -> Result<(), String> {
        match self {
            Self::Secret(store) => store.clear().map_err(|_| storage_error()),
            #[cfg(test)]
            Self::Memory(slot) => {
                *slot.lock().unwrap() = None;
                Ok(())
            }
        }
    }
}
fn storage_error() -> String {
    "protected-storage-unavailable".into()
}
