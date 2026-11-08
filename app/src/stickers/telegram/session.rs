//! Only MTProto authorization keys and DC endpoints are persisted. No phone,
//! API hash, password, contacts, messages, or update history is stored.
use grammers_session::{BoxFuture, Session, SessionData, storages::MemorySession, types::*};
use nanatype_secrets::SecretStore;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::Write as _,
    fs, io,
    net::{SocketAddrV4, SocketAddrV6},
    path::{Path, PathBuf},
    sync::Mutex,
};

struct Saved {
    home: i32,
    dcs: HashMap<i32, DcOption>,
}

/// On-disk form of [`Saved`]. Matches what grammers' own `serde` feature
/// wrote, so existing sessions still load: addresses as strings and the
/// authorization key as lowercase hex.
#[derive(Serialize, Deserialize)]
struct Disk {
    home: i32,
    dcs: HashMap<i32, DiskDc>,
}

#[derive(Serialize, Deserialize)]
struct DiskDc {
    id: i32,
    ipv4: SocketAddrV4,
    ipv6: SocketAddrV6,
    auth_key: Option<String>,
}

fn encode(saved: &Saved) -> io::Result<Vec<u8>> {
    let dcs = saved.dcs.iter().map(|(&id, dc)| {
        let auth_key = dc.auth_key.map(|key| {
            key.iter().fold(String::with_capacity(512), |mut hex, b| {
                let _ = write!(hex, "{b:02x}");
                hex
            })
        });
        let dc = DiskDc {
            id: dc.id,
            ipv4: dc.ipv4,
            ipv6: dc.ipv6,
            auth_key,
        };
        (id, dc)
    });
    let disk = Disk {
        home: saved.home,
        dcs: dcs.collect(),
    };
    Ok(serde_json::to_vec(&disk)?)
}

fn decode(bytes: &[u8]) -> io::Result<Saved> {
    let disk: Disk = serde_json::from_slice(bytes).map_err(io::Error::other)?;
    let mut dcs = HashMap::with_capacity(disk.dcs.len());
    for (id, dc) in disk.dcs {
        let auth_key = dc.auth_key.as_deref().map(unhex).transpose()?;
        let dc = DcOption {
            id: dc.id,
            ipv4: dc.ipv4,
            ipv6: dc.ipv6,
            auth_key,
        };
        dcs.insert(id, dc);
    }
    Ok(Saved {
        home: disk.home,
        dcs,
    })
}

fn unhex(hex: &str) -> io::Result<[u8; 256]> {
    let invalid = || io::Error::other("invalid Telegram authorization key");
    let hex = hex.as_bytes();
    if hex.len() != 512 {
        return Err(invalid());
    }
    let mut key = [0; 256];
    for (byte, &[high, low]) in key.iter_mut().zip(hex.as_chunks::<2>().0) {
        let nibble = |c: u8| (c as char).to_digit(16).ok_or_else(invalid);
        *byte = (nibble(high)? << 4 | nibble(low)?) as u8;
    }
    Ok(key)
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
            Some(bytes) => decode(&bytes)?,
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
        self.secret.save(&encode(saved)?)
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
    let saved = decode(&bytes)?;
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

#[cfg(test)]
mod format_tests {
    use super::*;

    /// The shape grammers' `serde` feature wrote; saved sessions must still load.
    #[test]
    fn reads_and_writes_the_grammers_serde_format() {
        let key = "0aF1".repeat(128);
        let json = format!(
            r#"{{"home":4,"dcs":{{"4":{{"id":4,"ipv4":"149.154.167.91:443","ipv6":"[2001:67c:4e8:f004::a]:443","auth_key":"{key}"}},"5":{{"id":5,"ipv4":"91.108.56.130:443","ipv6":"[2001:b28:f23f:f005::a]:443","auth_key":null}}}}}}"#
        );
        let saved = decode(json.as_bytes()).unwrap();
        assert_eq!(saved.home, 4);
        let dc = &saved.dcs[&4];
        assert_eq!(dc.ipv4, "149.154.167.91:443".parse().unwrap());
        assert_eq!(dc.ipv6, "[2001:67c:4e8:f004::a]:443".parse().unwrap());
        assert_eq!(dc.auth_key.unwrap()[..2], [0x0a, 0xf1]);
        assert_eq!(saved.dcs[&5].auth_key, None);

        let written: serde_json::Value = serde_json::from_slice(&encode(&saved).unwrap()).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(&json.replace(&key, &key.to_lowercase())).unwrap();
        assert_eq!(written, expected);
    }

    #[test]
    fn rejects_malformed_keys() {
        assert!(unhex(&"0".repeat(510)).is_err());
        assert!(unhex(&"g".repeat(512)).is_err());
        assert!(unhex(&"+f".repeat(256)).is_err());
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
