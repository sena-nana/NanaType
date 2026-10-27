//! Platform protected storage for account secrets. Plaintext never touches a
//! file: Windows keeps DPAPI ciphertext beside the settings, macOS uses the
//! login Keychain, and other platforms refuse to store anything.
use std::io;
use std::path::{Path, PathBuf};

pub struct SecretStore {
    #[cfg_attr(not(windows), allow(dead_code))]
    path: PathBuf,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    service: String,
}

/// Keychain item not found.
#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
#[cfg(target_os = "macos")]
const ACCOUNT: &str = "session";

impl SecretStore {
    /// `name` picks the file `<directory>/<name>.dpapi` on Windows and the
    /// Keychain service `dev.sena-nana.nanatype.<name>` on macOS.
    pub fn new(directory: &Path, name: &str) -> Self {
        Self::at(directory.join(format!("{name}.dpapi")), name)
    }

    /// Like [`new`](Self::new) with an explicit Windows file path.
    pub fn at(path: PathBuf, name: &str) -> Self {
        Self {
            path,
            service: format!("dev.sena-nana.nanatype.{name}"),
        }
    }

    pub fn load(&self) -> io::Result<Option<Vec<u8>>> {
        #[cfg(windows)]
        {
            match std::fs::read(&self.path) {
                Ok(b) => protect(&b, false).map(Some),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(unavailable()),
            }
        }
        #[cfg(target_os = "macos")]
        {
            match security_framework::passwords::get_generic_password(&self.service, ACCOUNT) {
                Ok(b) => Ok(Some(b)),
                Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
                Err(_) => Err(unavailable()),
            }
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            Ok(None)
        }
    }

    pub fn save(&self, bytes: &[u8]) -> io::Result<()> {
        #[cfg(windows)]
        {
            let encrypted = protect(bytes, true)?;
            std::fs::create_dir_all(self.path.parent().ok_or_else(unavailable)?)
                .map_err(|_| unavailable())?;
            // Atomic replacement contains only DPAPI ciphertext, including the temporary file.
            let temporary = self.path.with_extension("tmp");
            std::fs::write(&temporary, encrypted).map_err(|_| unavailable())?;
            std::fs::rename(temporary, &self.path).map_err(|_| unavailable())
        }
        #[cfg(target_os = "macos")]
        {
            security_framework::passwords::set_generic_password(&self.service, ACCOUNT, bytes)
                .map_err(|_| unavailable())
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = bytes;
            Err(unavailable())
        }
    }

    pub fn clear(&self) -> io::Result<()> {
        #[cfg(windows)]
        {
            let mut failed = false;
            for path in [&self.path, &self.path.with_extension("tmp")] {
                match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(_) => failed = true,
                }
            }
            if failed { Err(unavailable()) } else { Ok(()) }
        }
        #[cfg(target_os = "macos")]
        {
            match security_framework::passwords::delete_generic_password(&self.service, ACCOUNT) {
                Ok(()) => Ok(()),
                Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
                Err(_) => Err(unavailable()),
            }
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            Ok(())
        }
    }
}

fn unavailable() -> io::Error {
    io::Error::other("protected account storage unavailable")
}

#[cfg(windows)]
fn protect(bytes: &[u8], encrypt: bool) -> io::Result<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len().try_into().map_err(|_| unavailable())?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: input is borrowed for this call; DPAPI allocates output, copied then freed once.
    unsafe {
        let ok = if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(unavailable());
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn saved_secret_is_encrypted_and_clear_removes_it() {
        let directory = tempfile::tempdir().unwrap();
        let store = SecretStore::new(directory.path(), "fixture");
        let fixture = b"session=fixture-only; token=test; user_id=7";
        assert!(store.load().unwrap().is_none());
        store.save(fixture).unwrap();
        let encrypted = std::fs::read(&store.path).unwrap();
        assert!(
            !encrypted
                .windows(fixture.len())
                .any(|bytes| bytes == fixture)
        );
        assert_eq!(store.load().unwrap().unwrap(), fixture);
        store.save(b"replacement fixture").unwrap();
        assert_eq!(store.load().unwrap().unwrap(), b"replacement fixture");
        std::fs::copy(&store.path, store.path.with_extension("tmp")).unwrap();
        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
        assert!(!store.path.with_extension("tmp").exists());
    }
    #[test]
    fn unavailable_storage_reports_failure_without_plaintext_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("file");
        std::fs::write(&file, b"existing").unwrap();
        let store = SecretStore::new(&file, "fixture");
        assert!(store.save(b"fixture").is_err());
        assert_eq!(std::fs::read(file).unwrap(), b"existing");
    }
}
