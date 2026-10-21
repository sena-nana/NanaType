/// Public GitHub file proxies, in fallback order. They take the full
/// `https://github.com/...` URL after their own address. None of them is
/// trusted: what they return is checked against the signed manifest.
/// Checked 2026-10; see docs/updates.md before adding or removing one.
pub const BUILTIN: &[&str] = &[
    "https://gh-proxy.com/",
    "https://ghfast.top/",
    "https://ghproxy.net/",
];

/// Where release files are fetched from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Source {
    /// github.com itself.
    Direct,
    /// A proxy taking the original URL after this prefix.
    Prefix(String),
}

impl Source {
    /// A user-supplied proxy prefix: an `https://` address, or `http://` on
    /// this machine. A missing trailing slash is added.
    pub fn prefix(text: &str) -> Option<Self> {
        let text = text.trim();
        let rest = text
            .strip_prefix("https://")
            .or_else(|| text.strip_prefix("http://").filter(|r| loopback(r)))?;
        let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if host.is_empty() || text.contains(char::is_whitespace) || text.contains(['?', '#']) {
            return None;
        }
        let mut prefix = text.to_owned();
        if !prefix.ends_with('/') {
            prefix.push('/');
        }
        Some(Self::Prefix(prefix))
    }

    /// The built-in proxies, in fallback order.
    pub fn builtin() -> impl Iterator<Item = Self> {
        BUILTIN.iter().map(|p| Self::Prefix((*p).into()))
    }

    /// `url` as fetched through this source.
    pub fn url(&self, url: &str) -> String {
        match self {
            Self::Direct => url.into(),
            Self::Prefix(prefix) => format!("{prefix}{url}"),
        }
    }

    /// A short name for progress text: `GitHub` or the proxy's host.
    pub fn label(&self) -> String {
        match self {
            Self::Direct => "GitHub".into(),
            Self::Prefix(prefix) => prefix
                .split("://")
                .nth(1)
                .and_then(|r| r.split('/').next())
                .unwrap_or(prefix)
                .into(),
        }
    }
}

fn loopback(rest: &str) -> bool {
    ["127.0.0.1", "localhost", "[::1]"].iter().any(|host| {
        rest.strip_prefix(host)
            .is_some_and(|r| r.is_empty() || r.starts_with([':', '/']))
    })
}
