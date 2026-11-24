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
                Err(e) => Err(e),
            }
        }
        #[cfg(target_os = "macos")]
        {
            match security_framework::passwords::get_generic_password(&self.service, ACCOUNT) {
                Ok(b) => Ok(Some(b)),
                Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
                Err(e) => Err(keychain_error("load", &e)),
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
            let parent = self.path.parent().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "secret path has no parent directory",
                )
            })?;
            std::fs::create_dir_all(parent)?;
            // Atomic replacement contains only DPAPI ciphertext, including the temporary file.
            let temporary = temp_path(&self.path);
            std::fs::write(&temporary, encrypted)?;
            if let Err(err) = std::fs::rename(&temporary, &self.path) {
                let _ = std::fs::remove_file(&temporary);
                return Err(err);
            }
            Ok(())
        }
        #[cfg(target_os = "macos")]
        {
            security_framework::passwords::set_generic_password(&self.service, ACCOUNT, bytes)
                .map_err(|e| keychain_error("save", &e))
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = bytes;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "protected account storage unavailable on this platform",
            ))
        }
    }

    pub fn clear(&self) -> io::Result<()> {
        #[cfg(windows)]
        {
            let mut first_err: Option<io::Error> = None;
            let temporary = temp_path(&self.path);
            for path in [&self.path, &temporary] {
                match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) if first_err.is_none() => first_err = Some(e),
                    Err(_) => {}
                }
            }
            match first_err {
                Some(err) => Err(err),
                None => Ok(()),
            }
        }
        #[cfg(target_os = "macos")]
        {
            match security_framework::passwords::delete_generic_password(&self.service, ACCOUNT) {
                Ok(()) => Ok(()),
                Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
                Err(e) => Err(keychain_error("clear", &e)),
            }
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            Ok(())
        }
    }
}

#[cfg(any(windows, test))]
fn temp_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".tmp");
    PathBuf::from(os)
}

#[cfg(target_os = "macos")]
fn keychain_error(op: &str, err: &security_framework::base::Error) -> io::Error {
    let code = err.code();
    // Preserve OSStatus code and Security framework message; never include secret bytes.
    let kind = match code {
        -25293 | -25244 | -25308 => io::ErrorKind::PermissionDenied,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(
        kind,
        format!("keychain {op} failed (OSStatus {code}): {err}"),
    )
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
        cbData: bytes.len().try_into().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "secret exceeds DPAPI size limit",
            )
        })?,
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
            return Err(io::Error::last_os_error());
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}

#[cfg(test)]
mod common_tests {
    use super::*;

    #[test]
    fn temp_path_appends_tmp_without_replacing_extension() {
        assert_eq!(
            temp_path(Path::new("dir/twitch.dpapi")),
            PathBuf::from("dir/twitch.dpapi.tmp")
        );
        assert_eq!(
            temp_path(Path::new("dir/plain")),
            PathBuf::from("dir/plain.tmp")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn keychain_error_preserves_osstatus_and_kind_without_secrets() {
        let err = security_framework::base::Error::from(-25293);
        let io_err = keychain_error("load", &err);
        assert_eq!(io_err.kind(), io::ErrorKind::PermissionDenied);
        assert!(io_err.to_string().contains("-25293"));
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
        let tmp = temp_path(&store.path);
        std::fs::copy(&store.path, &tmp).unwrap();
        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
        assert!(!tmp.exists());
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
