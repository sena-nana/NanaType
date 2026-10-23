use crate::{Asset, Error, MANIFEST_NAME, Manifest, REPO, SIG_SUFFIX, Source};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Whole-request limit for the manifest and its signature.
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// How long slower sources may still answer once one has, so a stale proxy
/// cache cannot hide a newer release and the download order reflects speed.
const GRACE: Duration = Duration::from_millis(1500);
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
const CHUNK: usize = 64 * 1024;

pub struct Client {
    origin: String,
    public_key: String,
    check: ureq::Agent,
    download: ureq::Agent,
}

/// A verified manifest and the sources ordered for downloading: those that
/// answered, fastest first, then the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub manifest: Manifest,
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    pub source: Source,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    /// The client for github.com and the compiled-in release key.
    pub fn new() -> Self {
        Self::with("https://github.com".into(), crate::PUBLIC_KEY.into())
    }

    pub fn with(origin: String, public_key: String) -> Self {
        let user_agent = concat!("NanaType/", env!("CARGO_PKG_VERSION"));
        Self {
            origin: origin.trim_end_matches('/').into(),
            public_key,
            check: ureq::Agent::config_builder()
                .timeout_global(Some(CHECK_TIMEOUT))
                .user_agent(user_agent)
                .build()
                .into(),
            download: ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(10)))
                .timeout_recv_response(Some(Duration::from_secs(20)))
                .user_agent(user_agent)
                .build()
                .into(),
        }
    }

    pub fn manifest_url(&self) -> String {
        format!(
            "{}/{REPO}/releases/latest/download/{MANIFEST_NAME}",
            self.origin
        )
    }

    pub fn asset_url(&self, manifest: &Manifest, asset: &Asset) -> String {
        format!(
            "{}/{REPO}/releases/download/{}/{}",
            self.origin, manifest.tag, asset.name
        )
    }

    /// Ask every source at once for the signed manifest. The newest verified
    /// manifest wins.
    pub fn check(&self, sources: &[Source]) -> Result<Checked, Error> {
        if sources.is_empty() {
            return Err(Error::Unreachable("no sources".into()));
        }
        let (tx, rx) = crossbeam_channel::unbounded();
        let url = self.manifest_url();
        // Detached, so a slow source cannot hold the answer back; it gives
        // up on its own after the request timeout.
        for (i, source) in sources.iter().enumerate() {
            let (tx, url, source) = (tx.clone(), url.clone(), source.clone());
            let (agent, key) = (self.check.clone(), self.public_key.clone());
            std::thread::spawn(move || {
                let started = Instant::now();
                let result = fetch_manifest(&agent, &key, &source, &url);
                let _ = tx.send((i, result, started.elapsed()));
            });
        }
        drop(tx);
        let mut answered: Vec<(usize, Manifest, Duration)> = Vec::new();
        let mut failures: Vec<(usize, Error)> = Vec::new();
        let mut deadline = Instant::now() + CHECK_TIMEOUT + GRACE;
        while let Ok((i, result, took)) = rx.recv_deadline(deadline) {
            nanatype_diag::event!(
                crate::diag::SOURCE_CHECKED,
                source = i,
                ok = result.is_ok(),
                ms = took.as_millis() as u64,
            );
            if let Err(error) = &result {
                nanatype_diag::report!(crate::diag::SOURCE_FAILED, source = i; "{error}");
            }
            match result {
                Ok(manifest) => {
                    if answered.is_empty() {
                        deadline = deadline.min(Instant::now() + GRACE);
                    }
                    answered.push((i, manifest, took));
                }
                Err(e) => failures.push((i, e)),
            }
        }
        let manifest = answered
            .iter()
            .max_by(|a, b| version_order(&a.1, &b.1).then(b.2.cmp(&a.2)))
            .map(|(_, m, _)| m.clone())
            .ok_or_else(|| summarize(sources, &failures))?;
        answered.sort_by_key(|(_, _, took)| *took);
        let mut ordered: Vec<Source> = answered
            .iter()
            .filter(|(_, m, _)| m.version == manifest.version)
            .map(|(i, _, _)| sources[*i].clone())
            .collect();
        for source in sources {
            if !ordered.contains(source) {
                ordered.push(source.clone());
            }
        }
        Ok(Checked {
            manifest,
            sources: ordered,
        })
    }

    /// Download `asset` into `dir`, trying `sources` in order until one
    /// delivers the published size and digest.
    pub fn download(
        &self,
        manifest: &Manifest,
        asset: &Asset,
        sources: &[Source],
        dir: &Path,
        mut progress: impl FnMut(Progress),
        cancel: &AtomicBool,
    ) -> Result<PathBuf, Error> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(&asset.name);
        let partial = dir.join(format!("{}.part", asset.name));
        let url = self.asset_url(manifest, asset);
        let mut failures = Vec::new();
        for (index, source) in sources.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let report = |done| Progress {
                done,
                total: asset.size,
                source: source.clone(),
            };
            match self.fetch_to(&source.url(&url), asset, &partial, cancel, |done| {
                progress(report(done))
            }) {
                Ok(()) => {
                    std::fs::rename(&partial, &path)?;
                    nanatype_diag::event!(
                        crate::diag::DOWNLOADED,
                        source = index,
                        bytes = asset.size
                    );
                    return Ok(path);
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&partial);
                    if e == Error::Cancelled {
                        return Err(e);
                    }
                    nanatype_diag::report!(crate::diag::DOWNLOAD_FAILED, source = index; "{e}");
                    failures.push(format!("{}: {e}", source.label()));
                }
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        Err(Error::Download(failures.join("; ")))
    }

    fn fetch_to(
        &self,
        url: &str,
        asset: &Asset,
        partial: &Path,
        cancel: &AtomicBool,
        mut progress: impl FnMut(u64),
    ) -> Result<(), Error> {
        let response = self
            .download
            .get(url)
            .call()
            .map_err(|e| Error::Download(e.to_string()))?;
        // One byte over the size, so an oversized body is seen, not truncated.
        let mut body = response
            .into_body()
            .into_with_config()
            .limit(asset.size + 1)
            .reader();
        let mut file = std::fs::File::create(partial)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0; CHUNK];
        let (mut done, mut reported) = (0u64, 0u64);
        progress(0);
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let n = body
                .read(&mut buf)
                .map_err(|e| Error::Download(e.to_string()))?;
            if n == 0 {
                break;
            }
            done += n as u64;
            if done > asset.size {
                return Err(Error::Download("larger than published".into()));
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])?;
            // About a hundred updates per file.
            if done - reported >= (asset.size / 100).max(CHUNK as u64) || done == asset.size {
                reported = done;
                progress(done);
            }
        }
        file.sync_all()?;
        if done != asset.size {
            return Err(Error::Download("shorter than published".into()));
        }
        if crate::hex(&hasher.finalize()) != asset.sha256 {
            return Err(Error::Download("checksum mismatch".into()));
        }
        Ok(())
    }
}

/// Why no source answered: a rejected manifest outranks "no release",
/// which outranks network failures.
fn summarize(sources: &[Source], failures: &[(usize, Error)]) -> Error {
    let detail = |pick: fn(&Error) -> bool| {
        failures
            .iter()
            .filter(|(_, e)| pick(e))
            .map(|(i, e)| format!("{}: {e}", sources[*i].label()))
            .collect::<Vec<_>>()
            .join("; ")
    };
    if failures
        .iter()
        .any(|(_, e)| matches!(e, Error::BadManifest(_)))
    {
        Error::BadManifest(detail(|e| matches!(e, Error::BadManifest(_))))
    } else if failures.iter().any(|(_, e)| *e == Error::NoRelease) {
        Error::NoRelease
    } else {
        Error::Unreachable(detail(|_| true))
    }
}

fn fetch_manifest(
    agent: &ureq::Agent,
    public_key: &str,
    source: &Source,
    url: &str,
) -> Result<Manifest, Error> {
    let manifest = get_small(agent, &source.url(url))?;
    let signature = get_small(agent, &source.url(&format!("{url}{SIG_SUFFIX}")))?;
    crate::verify(&manifest, &signature, public_key)
}

fn get_small(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, Error> {
    let mut response = agent.get(url).call().map_err(|e| match e {
        ureq::Error::StatusCode(404) => Error::NoRelease,
        e => Error::Unreachable(e.to_string()),
    })?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_MANIFEST_BYTES)
        .read_to_vec()
        .map_err(|e| Error::Unreachable(e.to_string()))
}

fn version_order(a: &Manifest, b: &Manifest) -> std::cmp::Ordering {
    let parse = |m: &Manifest| crate::version::parse(&m.version);
    parse(a).cmp(&parse(b))
}
