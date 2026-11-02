//! What each service is sent and how its answer is read. Pure functions:
//! no network here, so both directions are tested on their own.

use serde_json::{Value, json};

use crate::{Error, Provider, Request, lang};

/// One HTTP POST, ready for a [`crate::Transport`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: String,
}

/// Lower temperatures keep the same line translated the same way.
const TEMPERATURE: f64 = 0.2;

/// The chat prompt: translate only, short, names kept, no quotes or notes.
pub fn system_prompt(target: &str, speaker: &str) -> String {
    let language = lang::prompt_name(target);
    let mut prompt = format!(
        "You translate lines of a live stream's dialogue into {language}. \
         Reply with the translation only: no quotes, notes, romanization or explanations. \
         Keep it short and natural to say aloud. Keep names, emoji and kaomoji as they are. \
         If the line is already in {language}, reply with it unchanged."
    );
    if !speaker.trim().is_empty() {
        prompt.push_str(&format!(" The line is said by {}.", speaker.trim()));
    }
    prompt
}

/// `…/chat/completions` under `base_url`, unless it already names it.
pub fn chat_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        base.to_owned()
    } else {
        format!("{base}/chat/completions")
    }
}

/// DeepL's free API takes keys ending in `:fx`; every other key is Pro.
pub fn deepl_url(key: &str) -> &'static str {
    if key.trim().ends_with(":fx") {
        "https://api-free.deepl.com/v2/translate"
    } else {
        "https://api.deepl.com/v2/translate"
    }
}

pub fn build(provider: &Provider, request: &Request) -> Result<HttpRequest, Error> {
    let mut headers = vec![("Content-Type", "application/json".to_owned())];
    match provider {
        Provider::Chat {
            base_url,
            model,
            key,
        } => {
            if let Some(key) = key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
                headers.push(("Authorization", format!("Bearer {key}")));
            }
            let body = json!({
                "model": model.trim(),
                "temperature": TEMPERATURE,
                "stream": false,
                "messages": [
                    {"role": "system", "content": system_prompt(&request.target, &request.speaker)},
                    {"role": "user", "content": request.text},
                ],
            });
            Ok(HttpRequest {
                url: chat_url(base_url),
                headers,
                body: body.to_string(),
            })
        }
        Provider::DeepL { key } => {
            let key = key.trim();
            if key.is_empty() {
                return Err(Error::NoKey);
            }
            headers.push(("Authorization", format!("DeepL-Auth-Key {key}")));
            let mut body = json!({
                "text": [request.text],
                "target_lang": lang::deepl_code(&request.target),
            });
            if !request.speaker.trim().is_empty() {
                // Context is not translated or billed; it steers the wording.
                body["context"] = Value::from(format!(
                    "A line of dialogue said by {} on a live stream.",
                    request.speaker.trim()
                ));
            }
            Ok(HttpRequest {
                url: deepl_url(key).to_owned(),
                headers,
                body: body.to_string(),
            })
        }
    }
}

fn status(code: u16) -> Result<(), Error> {
    match code {
        200..=299 => Ok(()),
        401 | 403 => Err(Error::Unauthorized),
        429 | 456 => Err(Error::Limited),
        _ => Err(Error::Status(code)),
    }
}

/// The translation in a service's answer.
pub fn parse(provider: &Provider, code: u16, body: &[u8]) -> Result<String, Error> {
    status(code)?;
    let value: Value = serde_json::from_slice(body).map_err(|_| Error::Parse)?;
    let text = match provider {
        Provider::Chat { .. } => value["choices"][0]["message"]["content"].as_str(),
        Provider::DeepL { .. } => value["translations"][0]["text"].as_str(),
    }
    .ok_or(Error::Parse)?;
    let text = clean(text);
    if text.is_empty() {
        return Err(Error::Empty);
    }
    Ok(text)
}

/// The reply without a reasoning model's `<think>` block, surrounding
/// whitespace or a pair of quotes around the whole line.
pub fn clean(text: &str) -> String {
    let mut text = text;
    if let Some(end) = text.find("</think>")
        && text.trim_start().starts_with("<think>")
    {
        text = &text[end + "</think>".len()..];
    }
    let mut text = text.trim();
    for (open, close) in [
        ('"', '"'),
        ('“', '”'),
        ('「', '」'),
        ('『', '』'),
        ('\'', '\''),
    ] {
        if let Some(inner) = text.strip_prefix(open).and_then(|t| t.strip_suffix(close))
            && !inner.contains([open, close])
        {
            text = inner.trim();
            break;
        }
    }
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(text: &str, speaker: &str, target: &str) -> Request {
        Request {
            id: 1,
            text: text.into(),
            speaker: speaker.into(),
            target: target.into(),
            lane: None,
        }
    }

    fn header<'a>(r: &'a HttpRequest, name: &str) -> Option<&'a str> {
        r.headers
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn chat_requests_carry_the_prompt_the_speaker_and_the_key() {
        let provider = Provider::Chat {
            base_url: "https://api.deepseek.com/v1/".into(),
            model: "deepseek-chat".into(),
            key: Some(" sk-test ".into()),
        };
        let r = build(&provider, &request("今日もよろしく", "奈奈", "en")).unwrap();
        assert_eq!(r.url, "https://api.deepseek.com/v1/chat/completions");
        assert_eq!(header(&r, "Authorization"), Some("Bearer sk-test"));
        let body: Value = serde_json::from_str(&r.body).unwrap();
        assert_eq!(body["model"], "deepseek-chat");
        assert_eq!(body["temperature"], 0.2);
        assert_eq!(body["messages"][1]["content"], "今日もよろしく");
        let system = body["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains("into English"), "{system}");
        assert!(system.contains("translation only"), "{system}");
        assert!(system.contains("said by 奈奈"), "{system}");

        // A local server needs no key, and a full endpoint is kept.
        let local = Provider::Chat {
            base_url: "http://localhost:11434/v1/chat/completions".into(),
            model: "qwen3".into(),
            key: None,
        };
        let r = build(&local, &request("hi", "", "ja")).unwrap();
        assert_eq!(r.url, "http://localhost:11434/v1/chat/completions");
        assert_eq!(header(&r, "Authorization"), None);
        let body: Value = serde_json::from_str(&r.body).unwrap();
        let system = body["messages"][0]["content"].as_str().unwrap();
        assert!(!system.contains("said by"), "{system}");
    }

    #[test]
    fn deepl_picks_its_endpoint_by_key_and_sends_context() {
        let free = Provider::DeepL {
            key: "abc:fx".into(),
        };
        let r = build(&free, &request("你好", "Nana", "en")).unwrap();
        assert_eq!(r.url, "https://api-free.deepl.com/v2/translate");
        assert_eq!(header(&r, "Authorization"), Some("DeepL-Auth-Key abc:fx"));
        let body: Value = serde_json::from_str(&r.body).unwrap();
        assert_eq!(body["text"][0], "你好");
        assert_eq!(body["target_lang"], "EN-US");
        assert!(body["context"].as_str().unwrap().contains("Nana"));
        assert!(body.get("source_lang").is_none(), "the source is detected");

        let pro = Provider::DeepL { key: "abc".into() };
        assert_eq!(
            build(&pro, &request("x", "", "ja")).unwrap().url,
            "https://api.deepl.com/v2/translate"
        );
        let none = Provider::DeepL { key: " ".into() };
        assert_eq!(build(&none, &request("x", "", "ja")), Err(Error::NoKey));
    }

    #[test]
    fn answers_are_read_and_cleaned() {
        let chat = Provider::Chat {
            base_url: String::new(),
            model: String::new(),
            key: None,
        };
        let ok =
            br#"{"choices":[{"message":{"role":"assistant","content":" \"Good evening!\" "}}]}"#;
        assert_eq!(parse(&chat, 200, ok).unwrap(), "Good evening!");
        let think = br#"{"choices":[{"message":{"content":"<think>hmm</think>\n\nHello"}}]}"#;
        assert_eq!(parse(&chat, 200, think).unwrap(), "Hello");
        assert_eq!(parse(&chat, 200, b"{}"), Err(Error::Parse));
        assert_eq!(parse(&chat, 200, b"<html>"), Err(Error::Parse));
        let blank = br#"{"choices":[{"message":{"content":"  "}}]}"#;
        assert_eq!(parse(&chat, 200, blank), Err(Error::Empty));
        let err = br#"{"error":{"message":"Incorrect API key"}}"#;
        assert_eq!(parse(&chat, 401, err), Err(Error::Unauthorized));
        assert_eq!(parse(&chat, 429, err), Err(Error::Limited));
        assert_eq!(parse(&chat, 500, err), Err(Error::Status(500)));

        let deepl = Provider::DeepL { key: "k".into() };
        let ok =
            br#"{"translations":[{"detected_source_language":"JA","text":"Thanks for coming"}]}"#;
        assert_eq!(parse(&deepl, 200, ok).unwrap(), "Thanks for coming");
        assert_eq!(parse(&deepl, 456, b""), Err(Error::Limited));
        assert_eq!(parse(&deepl, 403, b""), Err(Error::Unauthorized));
    }

    #[test]
    fn only_a_quote_around_the_whole_line_goes() {
        assert_eq!(clean("「こんにちは」"), "こんにちは");
        assert_eq!(clean("\"a\" and \"b\""), "\"a\" and \"b\"");
        assert_eq!(clean("He said \"hi\""), "He said \"hi\"");
        assert_eq!(clean("'tis"), "'tis");
    }
}
