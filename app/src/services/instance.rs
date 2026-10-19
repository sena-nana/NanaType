//! One NanaType per user. The first instance listens on a loopback port
//! derived from the user's identity; a second launch connects, proves it is
//! the same user, asks it to come forward, and exits.
//!
//! The reply matters: a port held by some other program, or by another
//! user's NanaType on a shared machine, never makes this launch quit.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

const PORT_BASE: u16 = 41_000;
const PORT_SPAN: u64 = 8_000;
const READY: &[u8] = b"nanatype:ok\n";

pub enum Instance {
    /// We are the first; `listener` reports later launches.
    Primary(TcpListener),
    /// Another instance of this user is running and was told to show itself.
    Secondary,
}

/// Who is launching: the account's home folder, which differs per user and
/// stays the same across that user's launches.
fn identity() -> u64 {
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in home.to_string_lossy().bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn hello() -> String {
    format!("nanatype:show:{:016x}\n", identity())
}

pub fn acquire() -> Instance {
    let port = PORT_BASE + (identity() % PORT_SPAN) as u16;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    match TcpListener::bind(addr) {
        Ok(listener) => Instance::Primary(listener),
        Err(_) if knock(&addr) => Instance::Secondary,
        // The port is taken by something that is not our instance; run anyway.
        Err(_) => match TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))) {
            Ok(l) => Instance::Primary(l),
            Err(_) => Instance::Secondary,
        },
    }
}

/// True when the listener at `addr` is this user's NanaType and agreed to show.
fn knock(addr: &SocketAddr) -> bool {
    let Ok(mut stream) = TcpStream::connect_timeout(addr, Duration::from_millis(500)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    if stream.write_all(hello().as_bytes()).is_err() {
        return false;
    }
    let mut reply = [0u8; READY.len()];
    stream.read_exact(&mut reply).is_ok() && reply == READY
}

/// Call `show` whenever another launch of this user knocks.
pub fn listen(listener: TcpListener, show: impl Fn() + Send + 'static) {
    let expected = hello();
    thread::Builder::new()
        .name("nanatype-instance".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let mut buf = [0u8; 64];
                if let Ok(n) = stream.read(&mut buf)
                    && buf[..n] == *expected.as_bytes()
                {
                    let _ = stream.write_all(READY);
                    show();
                }
            }
        })
        .expect("spawn instance listener");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn a_foreign_listener_is_not_mistaken_for_an_instance() {
        let foreign = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = foreign.local_addr().unwrap();
        thread::spawn(move || {
            for stream in foreign.incoming().flatten() {
                drop(stream);
            }
        });
        assert!(!knock(&addr));
    }

    #[test]
    fn the_same_user_is_shown() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        listen(listener, move || tx.send(()).unwrap());
        assert!(knock(&addr));
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }
}
