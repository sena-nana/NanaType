//! Only MTProto authorization keys and DC endpoints are persisted. No phone,
//! API hash, password, contacts, messages, or update history is stored.
use grammers_session::{BoxFuture, Session, SessionData, storages::MemorySession, types::*};
use nanatype_secrets::SecretStore;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Serialize, Deserialize)]
struct Saved {
    home: i32,
    dcs: HashMap<i32, DcOption>,
}

pub struct Store {
    secret: SecretStore,
    saved: Mutex<Saved>,
    memory: MemorySession,
}

/// The authorization key grants full account access, so it lives in protected
/// storage: DPAPI ciphertext at `path` on Windows, the Keychain on macOS.
fn secret(path: &Path) -> SecretStore {
    SecretStore::at(path.to_owned(), "telegram")
}

impl Store {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let secret = secret(&path);
        let data = SessionData::default();
        let saved = match secret.load()? {
            Some(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
            None => match migrate_plaintext(&path, &secret)? {
                Some(saved) => saved,
                None => Saved {
                    home: data.home_dc,
                    dcs: data.dc_options,
                },
            },
        };
        Ok(Self {
            secret,
            saved: Mutex::new(saved),
            memory: Default::default(),
        })
    }
    fn save(&self, saved: &Saved) -> io::Result<()> {
        self.secret.save(&serde_json::to_vec(saved)?)
    }
    fn data(&self) -> io::Result<std::sync::MutexGuard<'_, Saved>> {
        self.saved
            .lock()
            .map_err(|_| io::Error::other("Telegram session lock poisoned"))
    }
}

/// Earlier macOS builds wrote the session as plain JSON at `path`; move it
/// into the Keychain and delete the file.
#[cfg(not(windows))]
fn migrate_plaintext(path: &Path, secret: &SecretStore) -> io::Result<Option<Saved>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let saved = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    secret.save(&bytes)?;
    fs::remove_file(path)?;
    Ok(Some(saved))
}
#[cfg(windows)]
fn migrate_plaintext(_: &Path, _: &SecretStore) -> io::Result<Option<Saved>> {
    Ok(None)
}

impl Session for Store {
    type Error = io::Error;
    fn home_dc_id(&self) -> io::Result<i32> {
        Ok(self.data()?.home)
    }
    fn set_home_dc_id(&self, dc: i32) -> BoxFuture<'_, io::Result<()>> {
        Box::pin(async move {
            let mut data = self.data()?;
            data.home = dc;
            self.save(&data)
        })
    }
    fn dc_option(&self, id: i32) -> io::Result<Option<DcOption>> {
        Ok(self.data()?.dcs.get(&id).cloned())
    }
    fn set_dc_option(&self, dc: &DcOption) -> BoxFuture<'_, io::Result<()>> {
        let dc = dc.clone();
        Box::pin(async move {
            let mut data = self.data()?;
            data.dcs.insert(dc.id, dc);
            self.save(&data)
        })
    }
    fn peer(&self, id: PeerId) -> BoxFuture<'_, io::Result<Option<PeerInfo>>> {
        Box::pin(async move { self.memory.peer(id).await.map_err(io::Error::other) })
    }
    fn cache_peer(&self, peer: PeerInfo) -> BoxFuture<'_, io::Result<()>> {
        Box::pin(async move { self.memory.cache_peer(peer).await.map_err(io::Error::other) })
    }
    fn updates_state(&self) -> BoxFuture<'_, io::Result<UpdatesState>> {
        Box::pin(async move { self.memory.updates_state().await.map_err(io::Error::other) })
    }
    fn set_update_state(&self, update: UpdateState) -> BoxFuture<'_, io::Result<()>> {
        Box::pin(async move {
            self.memory
                .set_update_state(update)
                .await
                .map_err(io::Error::other)
        })
    }
}

pub fn remove(path: &Path) -> io::Result<()> {
    secret(path).clear()?;
    match fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

// Real protected storage: DPAPI needs no prompt; the Keychain may.
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn authorization_keys_round_trip_without_login_secrets() {
        let path = std::env::temp_dir().join(format!("nanatype-tg-session-{}", std::process::id()));
        let store = Store::open(path.clone()).unwrap();
        let mut data = store.data().unwrap();
        data.home = 4;
        data.dcs.get_mut(&4).unwrap().auth_key = Some([42; 256]);
        store.save(&data).unwrap();
        drop(data);
        let loaded = Store::open(path.clone()).unwrap();
        assert_eq!(loaded.home_dc_id().unwrap(), 4);
        assert_eq!(
            loaded.dc_option(4).unwrap().unwrap().auth_key,
            Some([42; 256])
        );
        assert!(!String::from_utf8_lossy(&fs::read(&path).unwrap()).contains("auth_key"));
        remove(&path).unwrap();
    }
}
