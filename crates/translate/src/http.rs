//! The real wire: ureq over rustls, with the system proxy, the one set in
//! the settings, or none for a server on this computer.

use std::time::Duration;

use crate::provider::HttpRequest;
use crate::{Config, Error, Transport};

/// Answers larger than this are not translations.
const BODY_LIMIT: u64 = 1 << 20;

pub struct Ureq {
    agent: ureq::Agent,
}

/// A server on this computer (Ollama, LM Studio) is never reached through
/// a proxy.
pub fn is_loopback(url: &str) -> bool {
    let Ok(url) = url::Url::parse(url) else {
        return false;
    };
    match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

impl Ureq {
    pub fn new(config: &Config) -> Result<Self, Error> {
        let proxy = if config.provider.endpoint().is_some_and(is_loopback) {
            None
        } else {
            match config.proxy.as_deref().map(str::trim) {
                Some(p) if !p.is_empty() => Some(ureq::Proxy::new(p).map_err(|_| Error::Proxy)?),
                _ => ureq::Proxy::try_from_env(),
            }
        };
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(config.timeout))
            .timeout_connect(Some(config.timeout.min(Duration::from_secs(5))))
            .http_status_as_error(false)
            .user_agent(concat!("NanaType/", env!("CARGO_PKG_VERSION")))
            .proxy(proxy)
            .build()
            .into();
        Ok(Self { agent })
    }
}

impl Transport for Ureq {
    fn post(&self, request: &HttpRequest) -> Result<(u16, Vec<u8>), Error> {
        let mut post = self.agent.post(&request.url);
        for (name, value) in &request.headers {
            post = post.header(*name, value);
        }
        let mut response = post.send(request.body.as_bytes()).map_err(|e| match e {
            ureq::Error::Timeout(_) => Error::Timeout,
            _ => Error::Network,
        })?;
        let code = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(BODY_LIMIT)
            .read_to_vec()
            .map_err(|e| match e {
                ureq::Error::Timeout(_) => Error::Timeout,
                _ => Error::Network,
            })?;
        Ok((code, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_servers_are_loopback() {
        assert!(is_loopback("http://localhost:11434/v1"));
        assert!(is_loopback("http://127.0.0.1:1234/v1"));
        assert!(is_loopback("http://[::1]:8080"));
        assert!(!is_loopback("https://api.openai.com/v1"));
        assert!(!is_loopback("not a url"));
    }
}
