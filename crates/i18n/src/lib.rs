//! UI strings.
//!
//! Strings live in Fluent files under `locales/`, one per language, compiled
//! into the binary: `locales/<locale>.ftl` holds what both products show,
//! `locales/app/` only NanaType's (feature `app`) and `locales/editor/` only
//! NanaType Editor's (feature `editor`), so neither ships the other's strings.
//! zh-CN is the source of truth: tests require every other locale to define
//! exactly its keys with the same variables. Lookups fall back from the active
//! locale to en-US to the key itself, so a missing string is visible rather
//! than blank. `locales/framework/` translates what NanaUI's own controls say
//! ([`framework_strings`]).

#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, OnceLock};

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Locale {
    #[default]
    ZhCn,
    EnUs,
    JaJp,
}

impl Locale {
    pub const ALL: [Self; 3] = [Self::ZhCn, Self::EnUs, Self::JaJp];

    pub fn tag(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::EnUs => "en-US",
            Self::JaJp => "ja-JP",
        }
    }

    /// The language's own name, for the language picker.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::ZhCn => "简体中文",
            Self::EnUs => "English",
            Self::JaJp => "日本語",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|l| l.tag().eq_ignore_ascii_case(tag))
    }

    /// The Fluent sources this build carries for the locale: the shared
    /// strings, then each enabled product's.
    fn sources(self) -> Vec<&'static str> {
        let mut sources = vec![match self {
            Self::ZhCn => include_str!("../locales/zh-CN.ftl"),
            Self::EnUs => include_str!("../locales/en-US.ftl"),
            Self::JaJp => include_str!("../locales/ja-JP.ftl"),
        }];
        #[cfg(feature = "app")]
        sources.push(match self {
            Self::ZhCn => include_str!("../locales/app/zh-CN.ftl"),
            Self::EnUs => include_str!("../locales/app/en-US.ftl"),
            Self::JaJp => include_str!("../locales/app/ja-JP.ftl"),
        });
        #[cfg(feature = "editor")]
        sources.push(match self {
            Self::ZhCn => include_str!("../locales/editor/zh-CN.ftl"),
            Self::EnUs => include_str!("../locales/editor/en-US.ftl"),
            Self::JaJp => include_str!("../locales/editor/ja-JP.ftl"),
        });
        sources
    }

    /// The supported locale for a user who reads `preferred` (BCP-47 tags,
    /// most preferred first), English when none is. A language in the script
    /// they read wins over one in another script: `zh-TW` then `en` is
    /// English, `zh-TW` alone Simplified Chinese.
    pub fn negotiate<T: AsRef<str>>(preferred: &[T]) -> Self {
        nana_ui_platform::negotiate_language(preferred, &Self::ALL.map(Self::tag))
            .and_then(Self::from_tag)
            .unwrap_or(Self::EnUs)
    }

    /// The operating system's preferred languages, negotiated.
    pub fn system() -> Self {
        Self::negotiate(&nana_ui_platform::preferred_languages())
    }

    /// NanaUI's own strings in this locale: Chinese as NanaUI ships them,
    /// `locales/framework/` over them otherwise.
    fn framework_source(self) -> Option<&'static str> {
        match self {
            Self::ZhCn => None,
            Self::EnUs => Some(include_str!("../locales/framework/en-US.ftl")),
            Self::JaJp => Some(include_str!("../locales/framework/ja-JP.ftl")),
        }
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

/// A Fluent argument value.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg<'a> {
    Str(Cow<'a, str>),
    Num(f64),
}

impl<'a> From<&'a str> for Arg<'a> {
    fn from(v: &'a str) -> Self {
        Self::Str(Cow::Borrowed(v))
    }
}

impl From<String> for Arg<'_> {
    fn from(v: String) -> Self {
        Self::Str(Cow::Owned(v))
    }
}

macro_rules! num_arg {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg<'_> {
            fn from(v: $t) -> Self {
                Self::Num(v as f64)
            }
        }
    )*};
}
num_arg!(u8, u16, u32, u64, usize, i32, i64, f32, f64);

pub struct I18n {
    locale: Locale,
    bundles: HashMap<Locale, FluentBundle<FluentResource>>,
}

impl fmt::Debug for I18n {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("I18n")
            .field("locale", &self.locale)
            .finish()
    }
}

fn bundle(locale: Locale) -> FluentBundle<FluentResource> {
    bundle_of(locale, locale.sources())
}

fn bundle_of(locale: Locale, sources: Vec<&'static str>) -> FluentBundle<FluentResource> {
    let id: LanguageIdentifier = locale.tag().parse().expect("static locale tags parse");
    let mut bundle = FluentBundle::new_concurrent(vec![id]);
    bundle.set_use_isolating(false);
    for source in sources {
        let resource = FluentResource::try_new(source.to_owned())
            .unwrap_or_else(|(_, errors)| panic!("{locale}.ftl does not parse: {errors:?}"));
        bundle
            .add_resource(resource)
            .unwrap_or_else(|errors| panic!("{locale}.ftl has duplicate keys: {errors:?}"));
    }
    bundle
}

impl I18n {
    pub fn new(locale: Locale) -> Self {
        Self {
            locale,
            bundles: Locale::ALL.into_iter().map(|l| (l, bundle(l))).collect(),
        }
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    pub fn t(&self, key: &str) -> String {
        self.format_in(self.locale, key, None)
    }

    pub fn tr(&self, key: &str, args: &[(&str, Arg<'_>)]) -> String {
        self.tr_in(self.locale, key, args)
    }

    /// Format in `locale` regardless of the current one (UI bindings pass the
    /// locale they track).
    pub fn t_in(&self, locale: Locale, key: &str) -> String {
        self.format_in(locale, key, None)
    }

    pub fn tr_in(&self, locale: Locale, key: &str, args: &[(&str, Arg<'_>)]) -> String {
        let mut fluent = FluentArgs::new();
        for (name, value) in args {
            match value {
                Arg::Str(s) => fluent.set(*name, FluentValue::from(s.to_string())),
                Arg::Num(n) => fluent.set(*name, FluentValue::from(*n)),
            }
        }
        self.format_in(locale, key, Some(&fluent))
    }

    pub fn has(&self, key: &str) -> bool {
        self.bundles[&self.locale].has_message(key)
    }

    fn format_in(&self, locale: Locale, key: &str, args: Option<&FluentArgs<'_>>) -> String {
        let (message_key, attribute) = match key.split_once('.') {
            Some((m, a)) => (m, Some(a)),
            None => (key, None),
        };
        for locale in [locale, Locale::EnUs] {
            let bundle = &self.bundles[&locale];
            let Some(message) = bundle.get_message(message_key) else {
                continue;
            };
            let pattern = match attribute {
                Some(a) => message.get_attribute(a).map(|attr| attr.value()),
                None => message.value(),
            };
            if let Some(pattern) = pattern {
                let mut errors = Vec::new();
                return bundle
                    .format_pattern(pattern, args, &mut errors)
                    .into_owned();
            }
        }
        key.to_owned()
    }
}

/// The Fluent id of NanaUI's string `key`: `toast.close` is
/// `framework-toast-close`.
pub fn framework_id(key: &str) -> String {
    format!("framework-{}", key.replace(['.', '_'], "-"))
}

/// What NanaUI's own controls say in `locale` (a toast's close button, an
/// unlabelled color field, a menu's search line), for every window document
/// in that language. A string the locale lacks falls back to en-US, then to
/// NanaUI's Chinese. NanaUI fills `{name}` in its templates itself, so a
/// Fluent variable is written back as that.
pub fn framework_strings(locale: Locale) -> Arc<nana_ui_core::FrameworkStrings> {
    static TABLES: [OnceLock<Arc<nana_ui_core::FrameworkStrings>>; 3] =
        [OnceLock::new(), OnceLock::new(), OnceLock::new()];
    let index = Locale::ALL.iter().position(|l| *l == locale).unwrap_or(0);
    Arc::clone(TABLES[index].get_or_init(|| {
        let mut strings = nana_ui_core::FrameworkStrings::default();
        if locale.framework_source().is_none() {
            return Arc::new(strings);
        }
        let bundles: Vec<_> = [locale, Locale::EnUs]
            .into_iter()
            .filter_map(|l| {
                l.framework_source()
                    .map(|source| bundle_of(l, vec![source]))
            })
            .collect();
        for key in nana_ui_core::FrameworkStrings::KEYS {
            let id = framework_id(key);
            let Some((bundle, pattern)) = bundles.iter().find_map(|bundle| {
                let pattern = bundle.get_message(&id)?.value()?;
                Some((bundle, pattern))
            }) else {
                continue;
            };
            let mut args = FluentArgs::new();
            for name in pattern_vars(pattern) {
                args.set(name.clone(), FluentValue::from(format!("{{{name}}}")));
            }
            let mut errors = Vec::new();
            let text = bundle.format_pattern(pattern, Some(&args), &mut errors);
            strings.set(key, text.into_owned());
        }
        Arc::new(strings)
    }))
}

/// The variables a pattern reads, selectors included, sorted.
fn pattern_vars(pattern: &fluent_syntax::ast::Pattern<&str>) -> Vec<String> {
    use fluent_syntax::ast::{Expression, InlineExpression, Pattern, PatternElement};

    fn inline(e: &InlineExpression<&str>, out: &mut Vec<String>) {
        if let InlineExpression::VariableReference { id } = e {
            out.push(id.name.to_owned());
        }
    }
    fn vars(pattern: &Pattern<&str>, out: &mut Vec<String>) {
        for element in &pattern.elements {
            if let PatternElement::Placeable { expression } = element {
                match expression {
                    Expression::Inline(i) => inline(i, out),
                    Expression::Select { selector, variants } => {
                        inline(selector, out);
                        for v in variants {
                            vars(&v.value, out);
                        }
                    }
                }
            }
        }
    }

    let mut out = Vec::new();
    vars(pattern, &mut out);
    out.sort();
    out.dedup();
    out
}

/// Message ids and their variables in a locale source, for completeness tests.
pub fn catalog(locale: Locale) -> Vec<(String, Vec<String>)> {
    catalog_of(locale.sources())
}

fn catalog_of(sources: Vec<&'static str>) -> Vec<(String, Vec<String>)> {
    use fluent_syntax::ast::Entry;
    use fluent_syntax::parser::parse;

    let resources = sources
        .iter()
        .map(|source| parse(*source).expect("locale parses"));
    let mut out = Vec::new();
    for entry in resources.flat_map(|resource| resource.body) {
        if let Entry::Message(message) = entry {
            for attr in &message.attributes {
                out.push((
                    format!("{}.{}", message.id.name, attr.id.name),
                    pattern_vars(&attr.value),
                ));
            }
            let v = message.value.as_ref().map(pattern_vars).unwrap_or_default();
            out.push((message.id.name.to_owned(), v));
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_locale_has_the_source_keys_and_variables() {
        let source = catalog(Locale::ZhCn);
        assert!(!source.is_empty());
        for locale in [Locale::EnUs, Locale::JaJp] {
            let other = catalog(locale);
            let missing: Vec<_> = source
                .iter()
                .filter(|(k, _)| !other.iter().any(|(o, _)| o == k))
                .map(|(k, _)| k.as_str())
                .collect();
            assert!(missing.is_empty(), "{locale} is missing {missing:?}");
            let extra: Vec<_> = other
                .iter()
                .filter(|(k, _)| !source.iter().any(|(s, _)| s == k))
                .map(|(k, _)| k.as_str())
                .collect();
            assert!(extra.is_empty(), "{locale} has unknown keys {extra:?}");
            for (key, vars) in &source {
                let theirs = &other.iter().find(|(k, _)| k == key).unwrap().1;
                assert_eq!(vars, theirs, "{locale} {key} variables differ");
            }
        }
    }

    #[test]
    fn lookups_format_and_fall_back() {
        let mut i18n = I18n::new(Locale::EnUs);
        assert_eq!(i18n.t("common-cancel"), "Cancel");
        assert_eq!(
            i18n.tr("marker-wait", &[("ms", 500.into())]),
            MARKER_WAIT_EN
        );
        i18n.set_locale(Locale::ZhCn);
        assert_eq!(i18n.tr("marker-wait", &[("ms", 500.into())]), "停 500ms");
        assert_eq!(i18n.t("no-such-key"), "no-such-key");
    }

    const MARKER_WAIT_EN: &str = "wait 500ms";

    /// Each product's strings are in its own build only.
    #[test]
    fn product_strings_follow_features() {
        let i18n = I18n::new(Locale::EnUs);
        assert_eq!(i18n.has("app-name"), cfg!(feature = "app"));
        assert_eq!(i18n.has("pkgedit-title"), cfg!(feature = "editor"));
    }

    #[cfg(feature = "app")]
    #[test]
    fn plurals_format() {
        let mut i18n = I18n::new(Locale::EnUs);
        assert_eq!(i18n.tr("history-count", &[("count", 1.into())]), "1 line");
        assert_eq!(i18n.tr("history-count", &[("count", 3.into())]), "3 lines");
        i18n.set_locale(Locale::ZhCn);
        assert_eq!(i18n.tr("history-count", &[("count", 3.into())]), "3 条");
    }

    #[test]
    fn negotiation_reads_every_preferred_language() {
        assert_eq!(Locale::negotiate(&["zh-Hans-SG"]), Locale::ZhCn);
        assert_eq!(Locale::negotiate(&["ja"]), Locale::JaJp);
        assert_eq!(Locale::negotiate(&["de-DE"]), Locale::EnUs);
        assert_eq!(Locale::negotiate::<&str>(&[]), Locale::EnUs);
        // Traditional Chinese readers get Simplified only when they read
        // nothing else NanaType ships.
        assert_eq!(Locale::negotiate(&["zh_TW"]), Locale::ZhCn);
        assert_eq!(Locale::negotiate(&["zh-TW", "en"]), Locale::EnUs);
        assert_eq!(Locale::negotiate(&["zh-Hant-HK", "ja-JP"]), Locale::JaJp);
        // A language past the first one counts.
        assert_eq!(Locale::negotiate(&["de-DE", "ja-JP"]), Locale::JaJp);
    }

    /// Each framework file names exactly NanaUI's keys, with the arguments
    /// NanaUI's own template fills.
    #[test]
    fn framework_files_cover_nanaui_strings() {
        use nana_ui_core::FrameworkStrings;
        fn braces(template: &str) -> Vec<String> {
            let mut out: Vec<String> = template
                .split('{')
                .skip(1)
                .filter_map(|rest| rest.split_once('}').map(|(name, _)| name.to_owned()))
                .collect();
            out.sort();
            out.dedup();
            out
        }
        let chinese = FrameworkStrings::default();
        let mut expected: Vec<(String, Vec<String>)> = FrameworkStrings::KEYS
            .iter()
            .map(|key| (framework_id(key), braces(chinese.get(key).unwrap())))
            .collect();
        expected.sort();
        for locale in [Locale::EnUs, Locale::JaJp] {
            let source = locale.framework_source().expect("translated");
            assert_eq!(
                catalog_of(vec![source]),
                expected,
                "{locale} framework strings"
            );
        }
        assert!(Locale::ZhCn.framework_source().is_none());
    }

    #[test]
    fn framework_strings_keep_nanaui_templates() {
        let zh = framework_strings(Locale::ZhCn);
        assert_eq!(*zh, nana_ui_core::FrameworkStrings::default());
        let en = framework_strings(Locale::EnUs);
        assert_eq!(&*en.toast_close, "Dismiss notification");
        assert_eq!(&*en.toast_percent, "{percent}%");
        assert_eq!(&*en.diff_accept_line, "Accept line {hunk}.{line}");
        let ja = framework_strings(Locale::JaJp);
        assert_eq!(&*ja.toast_close, "通知を閉じる");
        assert_eq!(&*ja.calendar_month, "{month}月");
        // Every string is translated, none left in NanaUI's Chinese except
        // where the languages write it alike.
        let same: Vec<_> = nana_ui_core::FrameworkStrings::KEYS
            .iter()
            .filter(|key| en.get(key) == zh.get(key))
            .collect();
        assert_eq!(same, [&"toast.percent"]);
    }
}
