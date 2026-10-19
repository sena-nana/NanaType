//! Platform protected storage. Plaintext credentials never touch a file.
use crate::ApiError;
use nanatype_secrets::SecretStore;
use std::path::Path;

pub struct Store(SecretStore);
impl Store {
    pub fn new(directory: &Path) -> Self {
        Self(SecretStore::new(directory, "bilibili"))
    }
    pub fn load(&self) -> Result<Option<Vec<u8>>, ApiError> {
        self.0.load().map_err(storage_error)
    }
    pub fn save(&self, bytes: &[u8]) -> Result<(), ApiError> {
        self.0.save(bytes).map_err(storage_error)
    }
    pub fn clear(&self) -> Result<(), ApiError> {
        self.0.clear().map_err(storage_error)
    }
}
fn storage_error(e: std::io::Error) -> ApiError {
    ApiError::Io(e.to_string())
}
