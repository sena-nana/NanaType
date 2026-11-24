use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use tokio::sync::{RwLock, watch};

use crate::link::{Carrier, Inbound, Outbound, Pipe};
use crate::picture::{self, Role};
use crate::protocol::{self, MAX_ASSET, Room, TrackSnapshot};

use super::wire::open_asset;
use super::{Emit, Event, Resources};

#[derive(Clone)]
pub(super) struct AssetOffer {
    pub source: PathBuf,
    pub len: u64,
    pub modified: Option<std::time::SystemTime>,
    pub encoded: PathBuf,
    pub preview: Vec<u8>,
    /// `Picture` covers a later use of the same file as an avatar.
    pub role: Role,
}

pub(super) type Offers = Arc<RwLock<BTreeMap<String, AssetOffer>>>;

/// Prepare an outbound track by encoding any newly attached local resources
/// and stripping all non-`blob:p2p-` references without a JSON DOM round-trip.
pub(super) async fn prepare(
    track: &TrackSnapshot,
    resources: &Resources,
    offers: &Offers,
    cache: &Path,
) -> Result<TrackSnapshot, String> {
    let mut out = track.clone();
    if resources.is_empty() {
        out.sanitize_assets();
        return Ok(out);
    }
    let _ = tokio::fs::create_dir_all(cache).await;
    let picture_roles = out.asset_roles();
    // One file can be an avatar and a portrait. Encode it once, at the larger size.
    let mut role_for_path: BTreeMap<&PathBuf, Role> = BTreeMap::new();
    for (reference, path) in resources {
        let role = picture_roles
            .get(reference)
            .copied()
            .unwrap_or(Role::Picture);
        role_for_path
            .entry(path)
            .and_modify(|existing| {
                if role == Role::Picture {
                    *existing = Role::Picture;
                }
            })
            .or_insert(role);
    }
    let mut replacements: BTreeMap<String, String> = BTreeMap::new();
    let mut encoded_for_path: BTreeMap<PathBuf, String> = BTreeMap::new();
    for (reference, path) in resources {
        if let Some(hash) = encoded_for_path.get(path) {
            replacements.insert(reference.clone(), hash.clone());
            continue;
        }
        let Ok(meta) = tokio::fs::metadata(path).await else {
            continue;
        };
        if meta.len() > MAX_ASSET as u64 {
            continue;
        }
        let modified = meta.modified().ok();
        let role = role_for_path.get(path).copied().unwrap_or(Role::Picture);
        let cached = offers
            .read()
            .await
            .iter()
            .find(|(_, offer)| {
                offer.source == *path
                    && offer.len == meta.len()
                    && offer.modified == modified
                    && (offer.role == Role::Picture || role == Role::Avatar)
            })
            .map(|(hash, _)| hash.clone());
        if let Some(hash) = cached {
            encoded_for_path.insert(path.clone(), hash.clone());
            replacements.insert(reference.clone(), hash);
            continue;
        }
        let Ok(bytes) = tokio::fs::read(path).await else {
            continue;
        };
        let encoded =
            tokio::task::spawn_blocking(move || picture::encode_picture(&bytes, role)).await;
        let Ok(Ok(encoded)) = encoded else {
            continue;
        };
        let hash = blake3::hash(&encoded.bytes).to_hex().to_string();
        let encoded_path = cache.join(format!("p2p-{hash}.{}", encoded.extension));
        if !encoded_path.is_file() {
            let temp = cache.join(format!("p2p-{hash}.partial"));
            if tokio::fs::write(&temp, &encoded.bytes).await.is_err() {
                continue;
            }
            if tokio::fs::rename(&temp, &encoded_path).await.is_err() {
                let _ = tokio::fs::remove_file(&temp).await;
                continue;
            }
        }
        offers.write().await.insert(
            hash.clone(),
            AssetOffer {
                source: path.clone(),
                len: meta.len(),
                modified,
                encoded: encoded_path,
                preview: encoded.preview,
                role,
            },
        );
        encoded_for_path.insert(path.clone(), hash.clone());
        replacements.insert(reference.clone(), hash);
    }
    out.rewrite_and_sanitize_assets(Some(&replacements));
    Ok(out)
}

pub(super) async fn pending_previews(
    offers: &Offers,
    cache: &Path,
    hashes: &BTreeSet<String>,
    sent: &mut BTreeSet<String>,
) -> BTreeMap<String, String> {
    if hashes.is_empty() || hashes.iter().all(|h| sent.contains(h)) {
        return BTreeMap::new();
    }
    let offers = offers.read().await;
    let mut out = BTreeMap::new();
    let mut relayed = Vec::new();
    for hash in hashes {
        if sent.contains(hash) {
            continue;
        }
        if let Some(offer) = offers.get(hash) {
            sent.insert(hash.clone());
            out.insert(hash.clone(), URL_SAFE_NO_PAD.encode(&offer.preview));
        } else {
            relayed.push(hash.clone());
        }
    }
    drop(offers);
    for hash in relayed {
        let Some(bytes) = cached_preview(cache, &hash).await else {
            continue;
        };
        sent.insert(hash.clone());
        out.insert(hash, URL_SAFE_NO_PAD.encode(bytes));
    }
    out
}

/// A preview another participant already sent, small enough to ride on the next frame.
async fn cached_preview(cache: &Path, hash: &str) -> Option<Vec<u8>> {
    if !protocol::valid_hash(hash) {
        return None;
    }
    let bytes = tokio::fs::read(cache.join(format!("p2p-{hash}.preview.webp")))
        .await
        .ok()?;
    if bytes.len() > picture::PREVIEW_MAX || picture::wire_kind(&bytes) != Some(picture::Kind::Webp)
    {
        None
    } else {
        Some(bytes)
    }
}

/// Bytes the host can hand to someone else: its own encode, or a picture already stored.
pub(super) async fn offered_asset(offers: &Offers, hash: &str) -> Option<Vec<u8>> {
    if !protocol::valid_hash(hash) {
        return None;
    }
    let path = offers
        .read()
        .await
        .get(hash)
        .map(|offer| offer.encoded.clone())?;
    let bytes = tokio::fs::read(&path).await.ok()?;
    if bytes.len() <= picture::WIRE_ASSET && blake3::hash(&bytes).to_hex().as_str() == hash {
        Some(bytes)
    } else {
        None
    }
}

pub(super) fn asset_owner(room: &Room, hash: &str) -> Option<String> {
    let target = format!("blob:p2p-{hash}");
    room.tracks.iter().find_map(|(id, track)| {
        let mut found = false;
        track.visit_assets(|reference, _| {
            if reference == target {
                found = true;
            }
        });
        found.then(|| id.clone())
    })
}

/// Another seated guest published this picture. The host is already on the
/// session pipe, and someone who left is not dialed.
pub(super) fn direct_publisher(room: &Room, local: &str, hash: &str) -> Option<u64> {
    let owner = asset_owner(room, hash)?;
    if owner == local || owner == room.host || !room.guests.iter().any(|id| id == &owner) {
        return None;
    }
    owner.parse().ok()
}

pub(super) async fn remember_seated(seated: &RwLock<BTreeSet<String>>, room: &Room) {
    let mut ids = BTreeSet::new();
    ids.insert(room.host.clone());
    ids.extend(room.guests.iter().cloned());
    *seated.write().await = ids;
}

/// Accept picture fetches from people the host has seated. Anyone else is closed
/// before a byte is sent. The session secret is not on this pipe.
pub(super) async fn serve_assets(
    carrier: Carrier,
    offers: Offers,
    seated: Arc<RwLock<BTreeSet<String>>>,
    mut stopped: watch::Receiver<bool>,
) {
    loop {
        if *stopped.borrow() {
            break;
        }
        let accept = open_asset(
            &carrier,
            None,
            Instant::now() + Duration::from_secs(24 * 60 * 60),
        );
        tokio::pin!(accept);
        let pipe = tokio::select! {
            _ = stopped.changed() => break,
            result = &mut accept => result,
        };
        let Ok(pipe) = pipe else {
            continue;
        };
        let allowed = seated.read().await.contains(&pipe.remote_id);
        if pipe.remote_id.is_empty() || !allowed {
            let _ = pipe.send.send(Outbound::Close);
            continue;
        }
        let offers = offers.clone();
        tokio::spawn(serve_one(pipe, offers));
    }
}

async fn serve_one(mut pipe: Pipe, offers: Offers) {
    while let Some(message) = pipe.recv.recv().await {
        match message {
            Inbound::Want(hash) => {
                let Some(bytes) = offered_asset(&offers, &hash).await else {
                    continue;
                };
                if pipe.send.send(Outbound::Asset { hash, bytes }).is_err() {
                    break;
                }
            }
            Inbound::Closed | Inbound::Frame(_) | Inbound::Asset { .. } => break,
        }
    }
    let _ = pipe.send.send(Outbound::Close);
}

pub(super) async fn fetch_asset(
    carrier: Carrier,
    publisher: u64,
    hash: String,
) -> Result<Vec<u8>, String> {
    let mut pipe = open_asset(
        &carrier,
        Some(publisher),
        Instant::now() + Duration::from_secs(8),
    )
    .await?;
    if pipe.send.send(Outbound::Want(hash.clone())).is_err() {
        return Err("Connection closed".into());
    }
    let hash_for_wait = hash.clone();
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match pipe.recv.recv().await {
                Some(Inbound::Asset { hash: got, bytes }) if got == hash_for_wait => {
                    return Ok(bytes);
                }
                Some(Inbound::Closed) | None => return Err("Connection closed".into()),
                Some(_) => {}
            }
        }
    })
    .await;
    let _ = pipe.send.send(Outbound::Close);
    match result {
        Ok(bytes) => bytes,
        Err(_) => Err("Connection timed out".into()),
    }
}

pub(super) async fn stored_asset(offers: &Offers, cache: &Path, hash: &str) -> Option<Vec<u8>> {
    if !protocol::valid_hash(hash) {
        return None;
    }
    if let Some(path) = offers
        .read()
        .await
        .get(hash)
        .map(|offer| offer.encoded.clone())
        && let Ok(bytes) = tokio::fs::read(&path).await
        && bytes.len() <= picture::WIRE_ASSET
        && blake3::hash(&bytes).to_hex().as_str() == hash
    {
        return Some(bytes);
    }
    let path = protocol::session_full_picture(cache, hash)?;
    let bytes = tokio::fs::read(&path).await.ok()?;
    if bytes.len() <= picture::WIRE_ASSET && blake3::hash(&bytes).to_hex().as_str() == hash {
        Some(bytes)
    } else {
        None
    }
}

pub(super) async fn store_previews(cache: &Path, previews: BTreeMap<String, String>, emit: &Emit) {
    if previews.is_empty() {
        return;
    }
    if tokio::fs::create_dir_all(cache).await.is_err() {
        return;
    }
    for (hash, encoded) in previews {
        if !protocol::valid_hash(&hash) || protocol::session_full_picture(cache, &hash).is_some() {
            continue;
        }
        let Ok(bytes) = URL_SAFE_NO_PAD.decode(encoded) else {
            continue;
        };
        if bytes.len() > picture::PREVIEW_MAX
            || picture::wire_kind(&bytes) != Some(picture::Kind::Webp)
        {
            continue;
        }
        let path = cache.join(format!("p2p-{hash}.preview.webp"));
        if path.is_file() {
            continue;
        }
        let temp = cache.join(format!("p2p-{hash}.preview.partial"));
        if tokio::fs::write(&temp, &bytes).await.is_err() {
            continue;
        }
        if tokio::fs::rename(&temp, &path).await.is_err() {
            let _ = tokio::fs::remove_file(&temp).await;
            continue;
        }
        emit(Event::AssetReady(hash));
    }
}

pub(super) async fn store_asset(cache: &Path, hash: &str, bytes: &[u8]) -> Result<(), String> {
    if !protocol::valid_hash(hash) || bytes.len() > picture::WIRE_ASSET {
        return Err("Asset exceeds limit".into());
    }
    if blake3::hash(bytes).to_hex().as_str() != hash {
        return Err("Asset hash mismatch".into());
    }
    if protocol::session_full_picture(cache, hash).is_some() {
        return Ok(());
    }
    let extension = match picture::wire_kind(bytes) {
        Some(picture::Kind::Webp) => "webp",
        Some(picture::Kind::Gif) => "gif",
        None => return Err("Unexpected picture".into()),
    };
    tokio::fs::create_dir_all(cache)
        .await
        .map_err(|error| error.to_string())?;
    let temp = cache.join(format!("p2p-{hash}.partial"));
    let final_path = cache.join(format!("p2p-{hash}.{extension}"));
    tokio::fs::write(&temp, bytes)
        .await
        .map_err(|error| error.to_string())?;
    tokio::fs::rename(temp, final_path)
        .await
        .map_err(|error| error.to_string())
}
