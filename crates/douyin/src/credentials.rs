//! Platform protected storage. Plaintext credentials never touch a file.
use nanatype_secrets::SecretStore;
use std::path::Path;

pub struct Store(SecretStore);
impl Store {
    pub fn new(directory: &Path) -> Self {
        Self(SecretStore::new(directory, "douyin"))
    }
    pub fn load(&self) -> Result<Option<Vec<u8>>, String> {
        self.0.load().map_err(|_| storage_error())
    }
    pub fn save(&self, bytes: &[u8]) -> Result<(), String> {
        self.0.save(bytes).map_err(|_| storage_error())
    }
    pub fn clear(&self) -> Result<(), String> {
        self.0.clear().map_err(|_| storage_error())
    }
}
fn storage_error() -> String {
    "protected-storage-unavailable".into()
}
