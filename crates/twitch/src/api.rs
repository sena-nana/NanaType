//! Twitch's OAuth device flow and the Helix calls sending needs.
use crate::Profile;
use serde_json::{Value, json};
use std::{fmt, time::Duration};

/// NanaType's public Twitch application. Public clients have no secret.
pub const CLIENT_ID: &str = match option_env!("NANATYPE_TWITCH_CLIENT_ID") {
    Some(id) => id,
    None => "9685icdld5y6okf57g98nwhvlz8ae2",
};
pub const SCOPES: &str = "user:write:chat";
const ID: &str = "https://id.twitch.tv/oauth2";
const HELIX: &str = "https://api.twitch.tv/helix";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    Network,
    /// The access token is invalid or expired.
    Unauthorized,
    /// Twitch refused the request; the message is Twitch's own.
    Rejected {
        status: u16,
        message: String,
    },
    Parse,
}
impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network => write!(f, "network error"),
            Self::Unauthorized => write!(f, "authorization expired"),
            Self::Rejected { status, message } => write!(f, "twitch {status}: {message}"),
            Self::Parse => write!(f, "unexpected Twitch response"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
}
impl Tokens {
    pub fn to_bytes(&self) -> Vec<u8> {
        json!({"access": self.access, "refresh": self.refresh})
            .to_string()
            .into_bytes()
    }
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let value: Value = serde_json::from_slice(bytes).ok()?;
        Some(Self {
            access: value["access"].as_str()?.to_owned(),
            refresh: value["refresh"].as_str()?.to_owned(),
        })
    }
    fn from_response(value: &Value) -> Result<Self, ApiError> {
        Ok(Self {
            access: value["access_token"]
                .as_str()
                .ok_or(ApiError::Parse)?
                .to_owned(),
            refresh: value["refresh_token"]
                .as_str()
                .ok_or(ApiError::Parse)?
                .to_owned(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenPoll {
    Pending,
    SlowDown,
    Ready(Tokens),
    /// Denied, expired or already used; start again.
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    Accepted {
        id: String,
    },
    /// Twitch received the message but did not post it (AutoMod, rules…).
    Dropped(String),
}

/// One HTTP exchange: the status and the JSON body (`Null` when empty).
pub(crate) type Reply = (u16, Value);

/// The wire, so the worker can be driven without a network in tests.
pub(crate) trait Http: Send {
    fn form(&self, url: &str, form: &[(&str, &str)]) -> Result<Reply, ApiError>;
    fn get(&self, url: &str, auth: &str) -> Result<Reply, ApiError>;
    fn post(&self, url: &str, auth: &str, body: &Value) -> Result<Reply, ApiError>;
}

pub(crate) struct Ureq(ureq::Agent);
impl Ureq {
    pub fn new() -> Self {
        Self(
            ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(10)))
                .max_redirects(0)
                .http_status_as_error(false)
                .build()
                .into(),
        )
    }
}
fn reply(result: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<Reply, ApiError> {
    let mut response = result.map_err(|_| ApiError::Network)?;
    let status = response.status().as_u16();
    let bytes = response
        .body_mut()
        .with_config()
        .limit(1 << 20)
        .read_to_vec()
        .map_err(|_| ApiError::Network)?;
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).map_err(|_| ApiError::Parse)?
    };
    Ok((status, body))
}
impl Http for Ureq {
    fn form(&self, url: &str, form: &[(&str, &str)]) -> Result<Reply, ApiError> {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form)
            .finish();
        reply(
            self.0
                .post(url)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .send(body.as_bytes()),
        )
    }
    fn get(&self, url: &str, auth: &str) -> Result<Reply, ApiError> {
        reply(
            self.0
                .get(url)
                .header("Authorization", auth)
                .header("Client-Id", CLIENT_ID)
                .call(),
        )
    }
    fn post(&self, url: &str, auth: &str, body: &Value) -> Result<Reply, ApiError> {
        reply(
            self.0
                .post(url)
                .header("Authorization", auth)
                .header("Client-Id", CLIENT_ID)
                .header("Content-Type", "application/json")
                .send(body.to_string().as_bytes()),
        )
    }
}

fn message(body: &Value) -> String {
    body["message"].as_str().unwrap_or_default().to_owned()
}
fn ok(reply: Reply) -> Result<Value, ApiError> {
    match reply {
        (200..=299, body) => Ok(body),
        (401, _) => Err(ApiError::Unauthorized),
        (status, body) => Err(ApiError::Rejected {
            status,
            message: message(&body),
        }),
    }
}

pub(crate) fn device(http: &dyn Http) -> Result<Device, ApiError> {
    let body = ok(http.form(
        &format!("{ID}/device"),
        &[("client_id", CLIENT_ID), ("scopes", SCOPES)],
    )?)?;
    Ok(Device {
        device_code: body["device_code"]
            .as_str()
            .ok_or(ApiError::Parse)?
            .to_owned(),
        user_code: body["user_code"]
            .as_str()
            .ok_or(ApiError::Parse)?
            .to_owned(),
        verification_uri: body["verification_uri"]
            .as_str()
            .ok_or(ApiError::Parse)?
            .to_owned(),
        expires_in: body["expires_in"].as_u64().unwrap_or(1800),
        interval: body["interval"].as_u64().unwrap_or(5).max(1),
    })
}

pub(crate) fn poll(http: &dyn Http, device_code: &str) -> Result<TokenPoll, ApiError> {
    let (status, body) = http.form(
        &format!("{ID}/token"),
        &[
            ("client_id", CLIENT_ID),
            ("scopes", SCOPES),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ],
    )?;
    if (200..300).contains(&status) {
        return Tokens::from_response(&body).map(TokenPoll::Ready);
    }
    Ok(match message(&body).as_str() {
        "authorization_pending" => TokenPoll::Pending,
        "slow_down" => TokenPoll::SlowDown,
        other => TokenPoll::Failed(other.to_owned()),
    })
}

/// Public-client refresh tokens are single-use: the caller must keep the new pair.
pub(crate) fn refresh(http: &dyn Http, tokens: &Tokens) -> Result<Tokens, ApiError> {
    let (status, body) = http.form(
        &format!("{ID}/token"),
        &[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", &tokens.refresh),
        ],
    )?;
    match status {
        200..=299 => Tokens::from_response(&body),
        400 | 401 => Err(ApiError::Unauthorized),
        status => Err(ApiError::Rejected {
            status,
            message: message(&body),
        }),
    }
}

/// Twitch asks apps to validate their token at start-up and hourly.
pub(crate) fn validate(http: &dyn Http, tokens: &Tokens) -> Result<String, ApiError> {
    let body = ok(http.get(
        &format!("{ID}/validate"),
        &format!("OAuth {}", tokens.access),
    )?)?;
    let user = body["user_id"].as_str().ok_or(ApiError::Parse)?;
    let scopes: Vec<&str> = body["scopes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !scopes.contains(&SCOPES) {
        // Granted before NanaType asked for chat; signing in again fixes it.
        return Err(ApiError::Unauthorized);
    }
    Ok(user.to_owned())
}

pub(crate) fn profile(
    http: &dyn Http,
    tokens: &Tokens,
    user_id: &str,
) -> Result<Profile, ApiError> {
    let body = ok(http.get(
        &format!("{HELIX}/users?id={user_id}"),
        &format!("Bearer {}", tokens.access),
    )?)?;
    let user = &body["data"][0];
    Ok(Profile {
        user_id: user["id"].as_str().ok_or(ApiError::Parse)?.to_owned(),
        login: user["login"].as_str().unwrap_or_default().to_owned(),
        name: user["display_name"].as_str().unwrap_or_default().to_owned(),
        avatar: user["profile_image_url"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    })
}

pub(crate) fn send(
    http: &dyn Http,
    tokens: &Tokens,
    broadcaster: &str,
    sender: &str,
    text: &str,
) -> Result<Sent, ApiError> {
    let body = ok(http.post(
        &format!("{HELIX}/chat/messages"),
        &format!("Bearer {}", tokens.access),
        &json!({"broadcaster_id": broadcaster, "sender_id": sender, "message": text}),
    )?)?;
    let result = &body["data"][0];
    if result["is_sent"].as_bool() == Some(true) {
        return Ok(Sent::Accepted {
            id: result["message_id"].as_str().unwrap_or_default().to_owned(),
        });
    }
    let reason = &result["drop_reason"];
    Ok(Sent::Dropped(
        reason["message"]
            .as_str()
            .or_else(|| reason["code"].as_str())
            .unwrap_or("message dropped")
            .to_owned(),
    ))
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    type Answer = (&'static str, Result<Reply, ApiError>);

    /// Answers queued per URL prefix; records what was asked.
    #[derive(Clone, Default)]
    pub struct Fake {
        pub replies: Arc<Mutex<VecDeque<Answer>>>,
        pub asked: Arc<Mutex<Vec<(String, String)>>>,
    }
    impl Fake {
        pub fn answer(&self, prefix: &'static str, reply: Result<Reply, ApiError>) {
            self.replies.lock().unwrap().push_back((prefix, reply));
        }
        fn take(&self, url: &str, detail: String) -> Result<Reply, ApiError> {
            self.asked.lock().unwrap().push((url.to_owned(), detail));
            let mut replies = self.replies.lock().unwrap();
            let index = replies
                .iter()
                .position(|(prefix, _)| url.starts_with(prefix))
                .unwrap_or_else(|| panic!("unexpected request {url}"));
            replies.remove(index).unwrap().1
        }
    }
    impl Http for Fake {
        fn form(&self, url: &str, form: &[(&str, &str)]) -> Result<Reply, ApiError> {
            self.take(url, format!("{form:?}"))
        }
        fn get(&self, url: &str, auth: &str) -> Result<Reply, ApiError> {
            self.take(url, auth.to_owned())
        }
        fn post(&self, url: &str, auth: &str, body: &Value) -> Result<Reply, ApiError> {
            self.take(url, format!("{auth} {body}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fake::Fake, *};

    fn tokens() -> Tokens {
        Tokens {
            access: "a".into(),
            refresh: "r".into(),
        }
    }

    #[test]
    fn device_polling_reads_every_answer() {
        let fake = Fake::default();
        let token = "https://id.twitch.tv/oauth2/token";
        fake.answer(
            token,
            Ok((
                400,
                json!({"status": 400, "message": "authorization_pending"}),
            )),
        );
        fake.answer(token, Ok((400, json!({"message": "slow_down"}))));
        fake.answer(token, Ok((400, json!({"message": "invalid device code"}))));
        fake.answer(
            token,
            Ok((200, json!({"access_token": "A", "refresh_token": "R"}))),
        );
        assert_eq!(poll(&fake, "d").unwrap(), TokenPoll::Pending);
        assert_eq!(poll(&fake, "d").unwrap(), TokenPoll::SlowDown);
        assert_eq!(
            poll(&fake, "d").unwrap(),
            TokenPoll::Failed("invalid device code".into())
        );
        assert_eq!(
            poll(&fake, "d").unwrap(),
            TokenPoll::Ready(Tokens {
                access: "A".into(),
                refresh: "R".into()
            })
        );
        let asked = fake.asked.lock().unwrap();
        assert!(
            asked[0]
                .1
                .contains("urn:ietf:params:oauth:grant-type:device_code")
        );
        assert!(asked[0].1.contains(CLIENT_ID));
    }

    #[test]
    fn validation_requires_the_chat_scope() {
        let fake = Fake::default();
        let url = "https://id.twitch.tv/oauth2/validate";
        fake.answer(
            url,
            Ok((200, json!({"user_id": "7", "scopes": ["user:write:chat"]}))),
        );
        fake.answer(url, Ok((200, json!({"user_id": "7", "scopes": []}))));
        fake.answer(url, Ok((401, json!({"message": "invalid access token"}))));
        assert_eq!(validate(&fake, &tokens()).unwrap(), "7");
        assert_eq!(validate(&fake, &tokens()), Err(ApiError::Unauthorized));
        assert_eq!(validate(&fake, &tokens()), Err(ApiError::Unauthorized));
        assert_eq!(fake.asked.lock().unwrap()[0].1, "OAuth a");
    }

    #[test]
    fn sending_tells_accepted_dropped_and_refused_apart() {
        let fake = Fake::default();
        let url = "https://api.twitch.tv/helix/chat/messages";
        fake.answer(
            url,
            Ok((
                200,
                json!({"data": [{"message_id": "m1", "is_sent": true}]}),
            )),
        );
        fake.answer(
            url,
            Ok((
                200,
                json!({"data": [{"message_id": "", "is_sent": false,
            "drop_reason": {"code": "msg_duplicate", "message": "Your message is identical"}}]}),
            )),
        );
        fake.answer(url, Ok((403, json!({"message": "The sender is banned"}))));
        assert_eq!(
            send(&fake, &tokens(), "1", "2", "hi").unwrap(),
            Sent::Accepted { id: "m1".into() }
        );
        assert_eq!(
            send(&fake, &tokens(), "1", "2", "hi").unwrap(),
            Sent::Dropped("Your message is identical".into())
        );
        assert_eq!(
            send(&fake, &tokens(), "1", "2", "hi"),
            Err(ApiError::Rejected {
                status: 403,
                message: "The sender is banned".into()
            })
        );
        let asked = fake.asked.lock().unwrap();
        assert!(asked[0].1.starts_with("Bearer a "));
        assert!(asked[0].1.contains("\"broadcaster_id\":\"1\""));
        assert!(asked[0].1.contains("\"sender_id\":\"2\""));
    }

    #[test]
    fn saved_tokens_round_trip_and_reject_garbage() {
        assert_eq!(Tokens::from_bytes(&tokens().to_bytes()), Some(tokens()));
        assert_eq!(Tokens::from_bytes(b"cookie=1"), None);
    }
}
