//! A credential kept in the system secret store, read the first time it is
//! needed.

use std::path::Path;

use nanatype_diag::EventDescriptor;
use nanatype_secrets::SecretStore;

pub struct LazySecret {
    store: SecretStore,
    value: Option<Option<String>>,
    /// Reported with `op` 0 when the stored value cannot be read.
    failed: &'static EventDescriptor,
    /// Surrounding whitespace is not part of the value (a pasted code), as
    /// opposed to a password, where it may be.
    trim: bool,
}

impl LazySecret {
    pub fn new(
        config_dir: &Path,
        name: &str,
        failed: &'static EventDescriptor,
        trim: bool,
    ) -> Self {
        Self {
            store: SecretStore::new(config_dir, name),
            value: None,
            failed,
            trim,
        }
    }

    fn clean<'a>(&self, value: &'a str) -> &'a str {
        if self.trim { value.trim() } else { value }
    }

    pub fn get(&mut self) -> Option<String> {
        if self.value.is_none() {
            let loaded = match self.store.load() {
                Ok(bytes) => bytes
                    .and_then(|b| String::from_utf8(b).ok())
                    .filter(|v| !self.clean(v).is_empty()),
                Err(e) => {
                    nanatype_diag::report!(*self.failed, op = 0u64; "{e}");
                    None
                }
            };
            self.value = Some(loaded);
        }
        self.value.clone().flatten()
    }

    /// A value is known for this run, without reading the store for it.
    pub fn held(&self) -> bool {
        self.value.as_ref().is_some_and(Option::is_some)
    }

    /// Keep `value` (or forget it when empty). It stays for this run even
    /// when the system will not store it.
    pub fn set(&mut self, value: &str) -> std::io::Result<()> {
        let value = self.clean(value);
        self.value = Some((!value.is_empty()).then(|| value.to_owned()));
        if value.is_empty() {
            self.store.clear()
        } else {
            self.store.save(value.as_bytes())
        }
    }
}
