//! One instance of each product per user. The first instance listens on a
//! loopback port derived from the user's identity; a second launch connects,
//! proves it is the same user and product, asks it to come forward, and exits.
//!
//! The reply matters: a port held by some other program, by another user's
//! instance on a shared machine, or by the other product, never makes this
//! launch quit.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

/// Both products' ports span this many below their base.
const PORT_SPAN: u16 = 8_000;

/// Which program is asking. NanaType and NanaType Editor use disjoint port
/// ranges (below the systems' ephemeral range) and their own greeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Product {
    pub tag: &'static str,
    pub port_base: u16,
}

impl Product {
    pub const APP: Self = Self {
        tag: "nanatype",
        port_base: 41_000,
    };
    pub const EDITOR: Self = Self {
        tag: "nanatype-editor",
        port_base: 33_000,
    };

    fn port(self) -> u16 {
        self.port_base + (identity() % u64::from(PORT_SPAN)) as u16
    }

    fn hello(self) -> String {
        format!("{}:show:{:016x}\n", self.tag, identity())
    }

    fn ready(self) -> String {
        format!("{}:ok\n", self.tag)
    }
}

enum Instance {
    /// We are the first; `listener` reports later launches.
    Primary(TcpListener),
    /// Another instance of this user is running and was told to show itself.
    Secondary,
}

static LISTENER: OnceLock<TcpListener> = OnceLock::new();

/// Become this user's instance of `product`. `false` when one is already
/// running: it was told to come forward, and this launch should exit.
pub fn claim(product: Product) -> bool {
    match acquire(product) {
        Instance::Primary(listener) => {
            LISTENER.set(listener).ok();
            true
        }
        Instance::Secondary => false,
    }
}

/// Answer later launches with `show`; call once the program can dispatch.
pub fn serve(product: Product, show: impl Fn() + Send + 'static) {
    if let Some(listener) = LISTENER.get().and_then(|l| l.try_clone().ok()) {
        listen(product, listener, show);
    }
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

fn acquire(product: Product) -> Instance {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, product.port()));
    match TcpListener::bind(addr) {
        Ok(listener) => Instance::Primary(listener),
        Err(_) if knock(product, &addr) => Instance::Secondary,
        // The port is taken by something that is not our instance; run anyway.
        Err(_) => match TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))) {
            Ok(l) => Instance::Primary(l),
            Err(_) => Instance::Secondary,
        },
    }
}

/// True when the listener at `addr` is this user's `product` and agreed to show.
fn knock(product: Product, addr: &SocketAddr) -> bool {
    let Ok(mut stream) = TcpStream::connect_timeout(addr, Duration::from_millis(500)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    if stream.write_all(product.hello().as_bytes()).is_err() {
        return false;
    }
    let ready = product.ready();
    let mut reply = vec![0u8; ready.len()];
    stream.read_exact(&mut reply).is_ok() && reply == ready.as_bytes()
}

/// Call `show` whenever another launch of this user's `product` knocks.
fn listen(product: Product, listener: TcpListener, show: impl Fn() + Send + 'static) {
    let expected = product.hello();
    let ready = product.ready();
    thread::Builder::new()
        .name(format!("{}-instance", product.tag))
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let mut buf = [0u8; 64];
                if let Ok(n) = stream.read(&mut buf)
                    && buf[..n] == *expected.as_bytes()
                {
                    let _ = stream.write_all(ready.as_bytes());
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
        assert!(!knock(Product::APP, &addr));
    }

    #[test]
    fn the_same_user_is_shown() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        listen(Product::APP, listener, move || tx.send(()).unwrap());
        assert!(knock(Product::APP, &addr));
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn the_other_product_is_not_shown() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        listen(Product::APP, listener, move || tx.send(()).unwrap());
        assert!(!knock(Product::EDITOR, &addr));
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn product_ports_do_not_overlap() {
        let (a, e) = (Product::APP, Product::EDITOR);
        assert!(e.port_base + PORT_SPAN <= a.port_base);
        // Stay below the ephemeral range Windows and macOS hand out.
        assert!(u32::from(a.port_base) + u32::from(PORT_SPAN) <= 49_152);
    }
}
