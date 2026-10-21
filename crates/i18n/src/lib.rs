//! UI strings.
//!
//! Strings live in Fluent files under `locales/`, one per language, compiled
//! into the binary: `locales/<locale>.ftl` holds what both products show,
//! `locales/app/` only NanaType's (feature `app`) and `locales/editor/` only
//! NanaType Editor's (feature `editor`), so neither ships the other's strings.
//! zh-CN is the source of truth: tests require every other locale to define
//! exactly its keys with the same variables. Lookups fall back from the active
//! locale to en-US to the key itself, so a missing string is visible rather
//! than blank.

#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

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

    /// Pick the best supported locale for a BCP-47 tag such as `zh-Hans-SG`.
    pub fn negotiate(tag: &str) -> Self {
        let lower = tag.replace('_', "-").to_ascii_lowercase();
        let lang = lower.split('-').next().unwrap_or("");
        match lang {
            "zh" => Self::ZhCn,
            "ja" => Self::JaJp,
            _ => Self::EnUs,
        }
    }

    /// The operating system's preferred language.
    pub fn system() -> Self {
        sys_locale::get_locale()
            .map(|tag| Self::negotiate(&tag))
            .unwrap_or(Self::EnUs)
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
    let id: LanguageIdentifier = locale.tag().parse().expect("static locale tags parse");
    let mut bundle = FluentBundle::new_concurrent(vec![id]);
    bundle.set_use_isolating(false);
    for source in locale.sources() {
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

/// Message ids and their variables in a locale source, for completeness tests.
pub fn catalog(locale: Locale) -> Vec<(String, Vec<String>)> {
    use fluent_syntax::ast::{Entry, Expression, InlineExpression, Pattern, PatternElement};
    use fluent_syntax::parser::parse;

    fn vars(pattern: &Pattern<&str>, out: &mut Vec<String>) {
        for element in &pattern.elements {
            if let PatternElement::Placeable { expression } = element {
                expr(expression, out);
            }
        }
    }
    fn inline(e: &InlineExpression<&str>, out: &mut Vec<String>) {
        if let InlineExpression::VariableReference { id } = e {
            out.push(id.name.to_owned());
        }
    }
    fn expr(e: &Expression<&str>, out: &mut Vec<String>) {
        match e {
            Expression::Inline(i) => inline(i, out),
            Expression::Select { selector, variants } => {
                inline(selector, out);
                for v in variants {
                    vars(&v.value, out);
                }
            }
        }
    }

    let sources = locale.sources();
    let resources = sources
        .iter()
        .map(|source| parse(*source).expect("locale parses"));
    let mut out = Vec::new();
    for entry in resources.flat_map(|resource| resource.body) {
        if let Entry::Message(message) = entry {
            let mut v = Vec::new();
            if let Some(value) = &message.value {
                vars(value, &mut v);
            }
            for attr in &message.attributes {
                let mut av = Vec::new();
                vars(&attr.value, &mut av);
                av.sort();
                av.dedup();
                out.push((format!("{}.{}", message.id.name, attr.id.name), av));
            }
            v.sort();
            v.dedup();
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
    fn negotiation_maps_regional_tags() {
        assert_eq!(Locale::negotiate("zh-Hans-SG"), Locale::ZhCn);
        assert_eq!(Locale::negotiate("zh_TW"), Locale::ZhCn);
        assert_eq!(Locale::negotiate("ja"), Locale::JaJp);
        assert_eq!(Locale::negotiate("de-DE"), Locale::EnUs);
    }
}
