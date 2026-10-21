use super::*;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

const SECRET: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";

fn public_key() -> String {
    let key = ed25519_dalek::SigningKey::from_bytes(&unhex::<32>(SECRET).unwrap());
    hex(key.verifying_key().as_bytes())
}

/// A minimal HTTP server answering each path from a shared table; a path
/// not in it gets a 404.
struct Server {
    origin: String,
    routes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let routes: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::default();
        let table = routes.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let path = line.split(' ').nth(1).unwrap_or("/").to_owned();
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) <= 2 {
                        break;
                    }
                }
                let body = table.lock().unwrap().get(&path).cloned();
                let mut stream = stream;
                let (status, body) = match body {
                    Some(body) => ("200 OK", body),
                    None => ("404 Not Found", b"missing".to_vec()),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
        });
        Self { origin, routes }
    }

    fn serve(&self, path: &str, body: impl Into<Vec<u8>>) {
        self.routes.lock().unwrap().insert(path.into(), body.into());
    }
}

fn manifest_for(version: &str, file: &[u8]) -> Manifest {
    use sha2::Digest;
    Manifest {
        schema: MANIFEST_SCHEMA,
        version: version.into(),
        tag: format!("v{version}"),
        assets: vec![Asset {
            name: format!("NanaType-{version}-setup.exe"),
            target: Target::WindowsX86_64,
            kind: Kind::Installer,
            size: file.len() as u64,
            sha256: hex(&sha2::Sha256::digest(file)),
        }],
    }
}

/// Publish a signed release on `server` under its own origin, and return
/// the manifest path prefix used by proxies of that origin.
fn publish(server: &Server, origin: &str, manifest: &Manifest, file: &[u8]) {
    let json = serde_json::to_vec(manifest).unwrap();
    let base = format!("/{REPO}/releases");
    let at = |path: &str| {
        if origin == server.origin {
            path.to_owned()
        } else {
            format!("/{origin}{path}")
        }
    };
    server.serve(
        &at(&format!("{base}/latest/download/{MANIFEST_NAME}")),
        json.clone(),
    );
    server.serve(
        &at(&format!(
            "{base}/latest/download/{MANIFEST_NAME}{SIG_SUFFIX}"
        )),
        sign(&json, SECRET).unwrap(),
    );
    server.serve(
        &at(&format!(
            "{base}/download/{}/{}",
            manifest.tag, manifest.assets[0].name
        )),
        file.to_vec(),
    );
}

#[test]
fn versions() {
    assert!(is_newer("0.2.0", "0.1.0"));
    assert!(is_newer("v1.0.0", "0.9.9"));
    assert!(!is_newer("0.1.0", "0.1.0"));
    assert!(!is_newer("0.1.0", "0.2.0"));
    assert!(!is_newer("0.2.0-beta.1", "0.1.0"));
    assert!(is_newer("0.2.0-beta.2", "0.2.0-beta.1"));
    assert!(is_newer("0.2.0", "0.2.0-beta.1"));
    assert!(!is_newer("garbage", "0.1.0"));
}

#[test]
fn proxy_prefixes() {
    let p = Source::prefix("https://gh-proxy.com").unwrap();
    assert_eq!(p, Source::Prefix("https://gh-proxy.com/".into()));
    assert_eq!(
        p.url("https://github.com/a/b"),
        "https://gh-proxy.com/https://github.com/a/b"
    );
    assert_eq!(p.label(), "gh-proxy.com");
    assert_eq!(Source::Direct.url("https://x/y"), "https://x/y");
    assert!(Source::prefix("http://example.com/").is_none());
    assert!(Source::prefix("http://127.0.0.1:8080").is_some());
    assert!(Source::prefix("http://localhost.evil.com/").is_none());
    assert!(Source::prefix("ftp://x/").is_none());
    assert!(Source::prefix("https://").is_none());
    assert!(Source::prefix("https://a b/").is_none());
    assert!(Source::prefix("https://x/?u=").is_none());
    assert_eq!(Source::builtin().count(), BUILTIN.len());
}

#[test]
fn signatures() {
    let manifest = manifest_for("0.2.0", b"installer");
    let json = serde_json::to_vec(&manifest).unwrap();
    let sig = sign(&json, SECRET).unwrap();
    assert_eq!(verify(&json, sig.as_bytes(), &public_key()), Ok(manifest));

    let mut tampered = json.clone();
    let at = tampered.iter().position(|b| *b == b'2').unwrap();
    tampered[at] = b'9';
    assert!(matches!(
        verify(&tampered, sig.as_bytes(), &public_key()),
        Err(Error::BadManifest(_))
    ));
    assert!(verify(&json, b"<html>error</html>", &public_key()).is_err());
    // The release key is not the test key.
    assert!(verify(&json, sig.as_bytes(), PUBLIC_KEY).is_err());
}

#[test]
fn manifest_validation() {
    let good = manifest_for("0.2.0", b"x");
    let check = |edit: fn(&mut Manifest)| {
        let mut m = good.clone();
        edit(&mut m);
        Manifest::parse(&serde_json::to_vec(&m).unwrap())
    };
    assert!(check(|_| {}).is_ok());
    assert!(check(|m| m.tag = "v9.9.9".into()).is_err());
    assert!(check(|m| m.schema = 2).is_err());
    assert!(check(|m| m.assets[0].name = "../evil.exe".into()).is_err());
    assert!(check(|m| m.assets[0].name = "a\\b.exe".into()).is_err());
    assert!(check(|m| m.assets[0].sha256 = "zz".into()).is_err());
    assert!(check(|m| m.assets[0].size = 0).is_err());
    // Unknown targets from newer releases are read but never chosen.
    let json = String::from_utf8(serde_json::to_vec(&good).unwrap())
        .unwrap()
        .replace("windows-x86_64", "linux-riscv64");
    let parsed = Manifest::parse(json.as_bytes()).unwrap();
    assert_eq!(parsed.assets[0].target, Target::Unknown);
    assert!(
        parsed
            .select(Target::WindowsX86_64, &[Kind::Installer])
            .is_none()
    );
    assert!(
        good.select(Target::WindowsX86_64, &[Kind::Portable])
            .is_none()
    );
    assert!(
        good.select(Target::WindowsX86_64, &[Kind::Portable, Kind::Installer])
            .is_some()
    );
}

#[test]
fn manifest_from_packaged_files() {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "NanaType-0.3.0-setup.exe",
        "NanaType-0.3.0-portable.zip",
        "NanaType-0.3.0-macos-arm64.dmg",
        "NanaType-0.3.0-macos-x86_64.zip",
        "NanaType-0.2.0-setup.exe",
        "notes.txt",
    ] {
        std::fs::write(dir.path().join(name), name).unwrap();
    }
    let m = Manifest::from_dir(dir.path(), "0.3.0").unwrap();
    assert_eq!(m.tag, "v0.3.0");
    assert_eq!(m.assets.len(), 4);
    let dmg = m.select(Target::MacosAarch64, &[Kind::Dmg]).unwrap();
    assert_eq!(dmg.size, "NanaType-0.3.0-macos-arm64.dmg".len() as u64);
}

#[test]
fn check_picks_newest_verified_source() {
    let github = Server::start();
    let stale = Server::start();
    let broken = Server::start();
    let file = b"new installer".to_vec();
    let newest = manifest_for("0.3.0", &file);
    publish(&github, &github.origin, &newest, &file);
    // A proxy with a cached older release, and one answering HTML.
    publish(
        &stale,
        &github.origin,
        &manifest_for("0.2.0", b"old"),
        b"old",
    );
    let path = format!(
        "/{}/{REPO}/releases/latest/download/{MANIFEST_NAME}",
        github.origin
    );
    broken.serve(&path, "<html>rate limited</html>");
    broken.serve(&format!("{path}{SIG_SUFFIX}"), "<html>rate limited</html>");

    let client = Client::with(github.origin.clone(), public_key());
    let stale_src = Source::prefix(&format!("{}/", stale.origin)).unwrap();
    let broken_src = Source::prefix(&broken.origin).unwrap();
    let checked = client
        .check(&[broken_src.clone(), stale_src.clone(), Source::Direct])
        .unwrap();
    assert_eq!(checked.manifest, newest);
    assert_eq!(checked.sources[0], Source::Direct);
    assert_eq!(checked.sources.len(), 3);

    // Only the broken proxy: its rejection is the error.
    assert!(matches!(
        client.check(&[broken_src]),
        Err(Error::BadManifest(_))
    ));
    // Nothing published anywhere yet.
    let empty = Server::start();
    let client = Client::with(empty.origin.clone(), public_key());
    assert_eq!(client.check(&[Source::Direct]), Err(Error::NoRelease));
}

#[test]
fn download_falls_back_and_verifies() {
    let github = Server::start();
    let proxy = Server::start();
    let file = vec![7u8; 300 * 1024];
    let manifest = manifest_for("0.3.0", &file);
    publish(&github, &github.origin, &manifest, &file);
    // The proxy serves a corrupted copy of the installer.
    let mut corrupt = file.clone();
    corrupt[10] ^= 1;
    publish(&proxy, &github.origin, &manifest, &corrupt);

    let client = Client::with(github.origin.clone(), public_key());
    let dir = tempfile::tempdir().unwrap();
    let asset = &manifest.assets[0];
    let proxy_src = Source::prefix(&proxy.origin).unwrap();
    let mut seen = Vec::new();
    let path = client
        .download(
            &manifest,
            asset,
            &[proxy_src.clone(), Source::Direct],
            dir.path(),
            |p| seen.push(p),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), file);
    assert!(seen.iter().any(|p| p.source == proxy_src));
    assert_eq!(seen.last().unwrap().done, file.len() as u64);
    assert_eq!(seen.last().unwrap().source, Source::Direct);
    assert!(!dir.path().join(format!("{}.part", asset.name)).exists());

    // Only the corrupt proxy: nothing is kept.
    let other = tempfile::tempdir().unwrap();
    let err = client
        .download(
            &manifest,
            asset,
            &[proxy_src],
            other.path(),
            |_| {},
            &AtomicBool::new(false),
        )
        .unwrap_err();
    assert!(matches!(err, Error::Download(ref why) if why.contains("checksum")));
    assert_eq!(std::fs::read_dir(other.path()).unwrap().count(), 0);

    // Cancelled before starting.
    let err = client
        .download(
            &manifest,
            asset,
            &[Source::Direct],
            other.path(),
            |_| {},
            &AtomicBool::new(true),
        )
        .unwrap_err();
    assert_eq!(err, Error::Cancelled);
}
