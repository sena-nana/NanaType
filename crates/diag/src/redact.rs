//! Free text on its way into a record. Logs never hold cookies, tokens,
//! passwords or the home directory; URLs keep scheme, host and path only.
//! Chat text and user names must not reach these functions at all: record
//! counts and codes instead.

use std::fmt::Display;

/// Longest message kept, in bytes.
pub const MAX_BYTES: usize = 512;

/// Keys whose value is a secret, compared case-insensitively. A key matches
/// only at a word start, so `monkey=1` is not `key=1`.
const SECRET_KEYS: &[&str] = &[
    "sessdata",
    "bili_jct",
    "dedeuserid",
    "buvid3",
    "access_token",
    "refresh_token",
    "id_token",
    "token",
    "auth_code",
    "cookie",
    "set-cookie",
    "authorization",
    "oauth",
    "password",
    "passwd",
    "secret",
    "client_secret",
    "api_key",
    "apikey",
    "key",
];

const MASK: &str = "***";

/// `scheme://host[:port]/path`: no user info, query or fragment.
pub fn url(url: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (Some(s), r),
        None => (None, url),
    };
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    let rest = &rest[..end];
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    match scheme {
        Some(s) => format!("{s}://{host}{path}"),
        None => format!("{host}{path}"),
    }
}

/// `error` formatted, then [`text`].
pub fn error(error: &dyn Display) -> String {
    text(&error.to_string())
}

/// A message safe to record: the home directory becomes `~`, URLs lose
/// their query, secret values become `***`, and it is cut to [`MAX_BYTES`].
pub fn text(message: &str) -> String {
    let mut out = home(message);
    out = urls(&out);
    out = secrets(&out);
    truncate(&mut out, MAX_BYTES);
    out
}

fn home(message: &str) -> String {
    let mut out = message.to_owned();
    for var in ["HOME", "USERPROFILE"] {
        let Some(home) = std::env::var_os(var) else {
            continue;
        };
        let home = home.to_string_lossy();
        // `/` alone (or an empty value) would mangle every path.
        if home.len() > 1 {
            out = out.replace(home.as_ref(), "~");
            // Windows paths also show up with forward slashes.
            if home.contains('\\') {
                out = out.replace(&home.replace('\\', "/"), "~");
            }
        }
    }
    out
}

fn urls(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(start) = find_url(rest) {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']'))
            .unwrap_or(tail.len());
        out.push_str(&url(&tail[..end]));
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn find_url(s: &str) -> Option<usize> {
    ["https://", "http://", "wss://", "ws://"]
        .iter()
        .filter_map(|scheme| s.find(scheme))
        .min()
}

fn secrets(message: &str) -> String {
    let lower = message.to_ascii_lowercase().into_bytes();
    let bytes = message.as_bytes();
    let mut out = String::with_capacity(message.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        let at_word_start = i == 0 || !is_word(bytes[i - 1]);
        let key = at_word_start
            .then(|| {
                SECRET_KEYS
                    .iter()
                    .filter(|k| lower[i..].starts_with(k.as_bytes()))
                    .max_by_key(|k| k.len())
            })
            .flatten();
        let Some(key) = key else {
            i += 1;
            continue;
        };
        let after_key = i + key.len();
        if after_key < bytes.len() && is_word(bytes[after_key]) {
            i += 1;
            continue;
        }
        let Some(value) = value_start(bytes, after_key) else {
            i = after_key;
            continue;
        };
        let end = value_end(bytes, value);
        if end > value {
            out.push_str(&message[copied..value]);
            out.push_str(MASK);
            copied = end;
        }
        i = end.max(after_key);
    }
    out.push_str(&message[copied..]);
    out
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// After a key: `=`, `:` or `": "`, then an optional quote and an optional
/// auth scheme (`Bearer`, `OAuth`, `Basic`). `None` when no separator follows.
fn value_start(bytes: &[u8], mut i: usize) -> Option<usize> {
    if bytes.get(i) == Some(&b'"') {
        i += 1;
    }
    while bytes.get(i) == Some(&b' ') {
        i += 1;
    }
    if !matches!(bytes.get(i), Some(b'=' | b':')) {
        return None;
    }
    i += 1;
    while matches!(bytes.get(i), Some(b' ' | b'"' | b'\'')) {
        i += 1;
    }
    for scheme in ["bearer ", "oauth ", "basic "] {
        let candidate = bytes.get(i..i + scheme.len());
        if candidate.is_some_and(|c| c.eq_ignore_ascii_case(scheme.as_bytes())) {
            i += scheme.len();
        }
    }
    Some(i)
}

fn value_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|b| b.is_ascii_whitespace() || matches!(b, b';' | b'&' | b',' | b'"' | b'\'' | b'}' | b']' | b')'))
        .map_or(bytes.len(), |p| start + p)
}

fn truncate(s: &mut String, max: usize) {
    if s.len() <= max {
        return;
    }
    let mut end = max - '…'.len_utf8();
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s.push('…');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_keeps_scheme_host_and_path() {
        assert_eq!(
            url("https://user:pw@api.live.bilibili.com:443/room/v1?id=1&access_token=x#f"),
            "https://api.live.bilibili.com:443/room/v1"
        );
        assert_eq!(url("example.com/a?b"), "example.com/a");
        assert_eq!(url("wss://h/sub"), "wss://h/sub");
    }

    #[test]
    fn urls_inside_text_lose_their_query() {
        assert_eq!(
            text("GET https://h/p?token=abc failed (\"https://x/y?z=1\")"),
            "GET https://h/p failed (\"https://x/y\")"
        );
    }

    #[test]
    fn secret_values_are_masked() {
        assert_eq!(
            text("Cookie: SESSDATA=abc%2C123; bili_jct=ff00; other=1"),
            "Cookie: ***; bili_jct=***; other=1"
        );
        assert_eq!(text("PASS oauth:abcdef"), "PASS oauth:***");
        assert_eq!(text("Authorization: Bearer eyJhbGci.x.y"), "Authorization: Bearer ***");
        assert_eq!(text(r#"{"access_token": "t0k", "n": 2}"#), r#"{"access_token": "***", "n": 2}"#);
        assert_eq!(text("refresh_token=r&x=1"), "refresh_token=***&x=1");
    }

    #[test]
    fn keys_match_only_whole_words() {
        assert_eq!(text("monkey=3 tokens: 4 keyboard"), "monkey=3 tokens: 4 keyboard");
        assert_eq!(text("decode: ok"), "decode: ok");
        assert_eq!(text("status code: 403"), "status code: 403");
        assert_eq!(text("名前 token=甲乙 ok"), "名前 token=*** ok");
    }

    #[test]
    fn home_becomes_tilde() {
        let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
        else {
            return;
        };
        let home = home.to_string_lossy();
        if home.len() > 1 {
            let path = format!("{home}/Library/x.json");
            assert_eq!(text(&format!("open {path}: denied")), "open ~/Library/x.json: denied");
        }
    }

    #[test]
    fn long_text_is_cut_on_a_char_boundary() {
        let long = "字".repeat(400);
        let cut = text(&long);
        assert!(cut.len() <= MAX_BYTES);
        assert!(cut.ends_with('…'));
    }
}
