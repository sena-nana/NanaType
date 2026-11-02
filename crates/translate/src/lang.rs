//! Target languages: the short list the settings offer, their names for the
//! chat prompt and DeepL's own codes.

/// A language the settings list: its code, its own name and its English
/// name (what the chat prompt asks for).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    pub code: &'static str,
    pub native: &'static str,
    english: &'static str,
}

const fn lang(code: &'static str, native: &'static str, english: &'static str) -> Language {
    Language {
        code,
        native,
        english,
    }
}

/// The targets offered, in the order the settings list them. Any other
/// BCP 47 code can be typed in.
pub const TARGETS: &[Language] = &[
    lang("en", "English", "English"),
    lang("ja", "日本語", "Japanese"),
    lang("zh-Hans", "简体中文", "Simplified Chinese"),
    lang("zh-Hant", "繁體中文", "Traditional Chinese"),
    lang("ko", "한국어", "Korean"),
    lang("es", "Español", "Spanish"),
    lang("fr", "Français", "French"),
    lang("de", "Deutsch", "German"),
    lang("ru", "Русский", "Russian"),
    lang("pt", "Português", "Portuguese"),
    lang("it", "Italiano", "Italian"),
    lang("id", "Bahasa Indonesia", "Indonesian"),
    lang("th", "ไทย", "Thai"),
    lang("vi", "Tiếng Việt", "Vietnamese"),
];

/// A listed language by its code, ignoring case.
pub fn find(code: &str) -> Option<&'static Language> {
    TARGETS.iter().find(|l| l.code.eq_ignore_ascii_case(code))
}

/// What the chat prompt calls `code`: the English name of a listed
/// language, else the code itself (models know `pt-BR` or `uk`).
pub fn prompt_name(code: &str) -> String {
    match find(code) {
        Some(l) => l.english.to_owned(),
        None => code.trim().to_owned(),
    }
}

/// DeepL's `target_lang` for `code`: it wants a region for English and
/// Portuguese and a script for Chinese.
pub fn deepl_code(code: &str) -> String {
    let code = code.trim();
    let lower = code.to_ascii_lowercase();
    match lower.as_str() {
        "en" => "EN-US".into(),
        "pt" => "PT-BR".into(),
        "zh" | "zh-hans" | "zh-cn" | "zh-sg" => "ZH-HANS".into(),
        "zh-hant" | "zh-tw" | "zh-hk" => "ZH-HANT".into(),
        _ => code.to_ascii_uppercase(),
    }
}

/// A target code worth sending: letters, digits and dashes, like `ja` or
/// `zh-Hant`.
pub fn valid_code(code: &str) -> bool {
    let code = code.trim();
    (2..=16).contains(&code.len())
        && code
            .split('-')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_map_for_each_provider() {
        assert_eq!(deepl_code("en"), "EN-US");
        assert_eq!(deepl_code("zh-Hans"), "ZH-HANS");
        assert_eq!(deepl_code("zh-TW"), "ZH-HANT");
        assert_eq!(deepl_code("ja"), "JA");
        assert_eq!(deepl_code("en-GB"), "EN-GB");
        assert_eq!(prompt_name("ZH-hant"), "Traditional Chinese");
        assert_eq!(prompt_name("uk"), "uk");
        assert!(valid_code("zh-Hant") && valid_code("pt-BR") && valid_code("ja"));
        assert!(!valid_code("") && !valid_code("e") && !valid_code("en_US"));
        assert!(!valid_code("en-") && !valid_code("a b"));
    }
}
