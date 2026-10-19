//! The worker's OS confinement really denies files, network and processes.
//! The test re-runs itself as a child so the parent stays unconfined.
use std::process::Command;

const CHILD: &str = "NANATYPE_SANDBOX_CHILD";

/// In the confined child: report each attempt as `name=allowed|denied`.
fn probe() {
    let confinement = nanatype_plugins::confine_worker().unwrap();
    let dir = std::env::var("NANATYPE_SANDBOX_DIR").unwrap();
    let attempts: [(&str, bool); 6] = [
        ("read_file", std::fs::read("/etc/hosts").is_ok()),
        (
            "read_prepared_file",
            std::fs::read(format!("{dir}/secret.txt")).is_ok(),
        ),
        (
            "write_file",
            std::fs::write(format!("{dir}/written.txt"), b"x").is_ok(),
        ),
        ("list_dir", std::fs::read_dir(&dir).is_ok()),
        (
            "connect",
            std::net::TcpStream::connect_timeout(
                &"127.0.0.1:9".parse().unwrap(),
                std::time::Duration::from_millis(300),
            )
            .is_ok()
                || std::net::UdpSocket::bind("127.0.0.1:0").is_ok(),
        ),
        ("spawn", Command::new("/usr/bin/true").status().is_ok()),
    ];
    println!("confinement={confinement:?}");
    for (name, allowed) in attempts {
        println!("{name}={}", if allowed { "allowed" } else { "denied" });
    }
}

#[test]
fn confined_workers_cannot_touch_files_network_or_processes() {
    if std::env::var_os(CHILD).is_some() {
        probe();
        return;
    }
    if !cfg!(target_os = "macos") {
        eprintln!("worker-side OS confinement is only verified on macOS here");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("secret.txt"), b"host data").unwrap();
    let out = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "confined_workers_cannot_touch_files_network_or_processes",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .env("NANATYPE_SANDBOX_DIR", dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("confinement=Seatbelt"), "{stdout}");
    for name in [
        "read_file",
        "read_prepared_file",
        "write_file",
        "list_dir",
        "connect",
        "spawn",
    ] {
        assert!(
            stdout.contains(&format!("{name}=denied")),
            "{name} was not denied:\n{stdout}"
        );
    }
    assert!(!dir.path().join("written.txt").exists());
}
