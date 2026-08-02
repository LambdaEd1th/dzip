use std::{cell::RefCell, collections::HashMap};

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use unic_langid::LanguageIdentifier;

thread_local! {
    static LOCALE_BUNDLES: RefCell<HashMap<&'static str, FluentBundle<FluentResource>>> = RefCell::new(HashMap::new());
    static INTERNED_LOCALE_CODES: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Locale {
    code: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageOption {
    pub locale: Locale,
    pub label: String,
}

impl Locale {
    pub const EN_US: Self = Self { code: "en-US" };
    pub const ZH_CN: Self = Self { code: "zh-CN" };
    pub const FR_FR: Self = Self { code: "fr-FR" };
    pub const RU_RU: Self = Self { code: "ru-RU" };
    pub const ES_ES: Self = Self { code: "es-ES" };

    pub fn supported_from_code(code: &str) -> Option<Self> {
        let normalized = canonical_locale_code(code)?;
        installed_locale_from_exact(&normalized)
            .or_else(|| installed_locale_from_language(&normalized))
    }

    pub const fn code(self) -> &'static str {
        self.code
    }

    fn constant_from_exact(code: &str) -> Option<Self> {
        if code.eq_ignore_ascii_case(Self::EN_US.code) {
            Some(Self::EN_US)
        } else if code.eq_ignore_ascii_case(Self::ZH_CN.code) {
            Some(Self::ZH_CN)
        } else if code.eq_ignore_ascii_case(Self::FR_FR.code) {
            Some(Self::FR_FR)
        } else if code.eq_ignore_ascii_case(Self::RU_RU.code) {
            Some(Self::RU_RU)
        } else if code.eq_ignore_ascii_case(Self::ES_ES.code) {
            Some(Self::ES_ES)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct I18n {
    locale: Locale,
}

impl I18n {
    pub const fn new(locale: Locale) -> Self {
        Self { locale }
    }

    pub fn t(self, key: &str) -> String {
        self.t_args(key, &[])
    }

    pub fn t_args(self, key: &str, args: &[(&str, String)]) -> String {
        render(self.locale, key, args)
    }
}

pub fn install_locale(code: &str, source: &str) -> Result<Locale, String> {
    let code = canonical_locale_code(code).ok_or_else(|| "invalid locale code".to_string())?;
    let bundle = build_bundle_from_source(&code, source)?;
    let locale = intern_locale_code(&code);
    LOCALE_BUNDLES.with(|bundles| {
        bundles.borrow_mut().insert(locale.code(), bundle);
    });
    Ok(locale)
}

pub fn available_locales() -> Vec<Locale> {
    let mut locales = LOCALE_BUNDLES.with(|bundles| {
        bundles
            .borrow()
            .keys()
            .copied()
            .map(|code| Locale { code })
            .collect::<Vec<_>>()
    });
    locales.sort_by(|left, right| left.code().cmp(right.code()));
    if locales.is_empty() {
        locales.push(Locale::EN_US);
    }
    locales
}

pub fn language_options() -> Vec<LanguageOption> {
    available_locales()
        .into_iter()
        .map(|locale| LanguageOption {
            locale,
            label: format_locale_message(locale, "language-self", &[])
                .unwrap_or_else(|| locale.code().to_string()),
        })
        .collect()
}

fn render(locale: Locale, key: &str, args: &[(&str, String)]) -> String {
    format_locale_message(locale, key, args)
        .or_else(|| {
            (locale != Locale::EN_US)
                .then(|| format_locale_message(Locale::EN_US, key, args))
                .flatten()
        })
        .unwrap_or_else(|| key.to_string())
}

fn format_locale_message(locale: Locale, key: &str, args: &[(&str, String)]) -> Option<String> {
    LOCALE_BUNDLES.with(|bundles| {
        bundles
            .borrow()
            .get(locale.code())
            .and_then(|bundle| format_message(bundle, key, args))
    })
}

fn build_bundle_from_source(
    locale_code: &str,
    source: &str,
) -> Result<FluentBundle<FluentResource>, String> {
    let langid: LanguageIdentifier = locale_code
        .parse()
        .map_err(|error| format!("invalid locale identifier: {error}"))?;
    let resource = FluentResource::try_new(source.to_string())
        .map_err(|(_, errors)| format!("invalid FTL: {errors:?}"))?;
    let mut bundle = FluentBundle::new(vec![langid]);
    bundle.set_use_isolating(false);
    bundle
        .add_resource(resource)
        .map_err(|errors| format!("invalid FTL resource: {errors:?}"))?;
    Ok(bundle)
}

fn format_message(
    bundle: &FluentBundle<FluentResource>,
    key: &str,
    args: &[(&str, String)],
) -> Option<String> {
    let pattern = bundle.get_message(key)?.value()?;
    let mut fluent_args = FluentArgs::new();
    for (name, value) in args {
        fluent_args.set(*name, value.as_str());
    }
    let mut errors = Vec::new();
    Some(
        bundle
            .format_pattern(pattern, Some(&fluent_args), &mut errors)
            .into_owned(),
    )
}

fn installed_locale_from_exact(code: &str) -> Option<Locale> {
    LOCALE_BUNDLES.with(|bundles| {
        bundles
            .borrow()
            .keys()
            .copied()
            .find(|candidate| candidate.eq_ignore_ascii_case(code))
            .map(|code| Locale { code })
    })
}

fn installed_locale_from_language(code: &str) -> Option<Locale> {
    let language = code.split('-').next().unwrap_or(code);
    let mut matches = LOCALE_BUNDLES.with(|bundles| {
        bundles
            .borrow()
            .keys()
            .copied()
            .filter(|candidate| {
                candidate
                    .split('-')
                    .next()
                    .unwrap_or(candidate)
                    .eq_ignore_ascii_case(language)
            })
            .collect::<Vec<_>>()
    });
    matches.sort_unstable();
    matches.first().copied().map(|code| Locale { code })
}

fn intern_locale_code(code: &str) -> Locale {
    if let Some(locale) = Locale::constant_from_exact(code) {
        return locale;
    }
    INTERNED_LOCALE_CODES.with(|codes| {
        let mut codes = codes.borrow_mut();
        if let Some(code) = codes
            .iter()
            .copied()
            .find(|candidate| candidate.eq_ignore_ascii_case(code))
        {
            return Locale { code };
        }
        let code = Box::leak(code.to_string().into_boxed_str());
        codes.push(code);
        Locale { code }
    })
}

fn canonical_locale_code(code: &str) -> Option<String> {
    let normalized = code
        .trim()
        .split(['.', ':'])
        .next()
        .unwrap_or(code)
        .replace('_', "-");
    let parts = normalized
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let language = *parts.first()?;
    if !(2..=8).contains(&language.len()) || !language.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }

    let mut canonical = vec![language.to_ascii_lowercase()];
    for part in parts.iter().skip(1) {
        if !(1..=8).contains(&part.len()) || !part.chars().all(|ch| ch.is_ascii_alphanumeric()) {
            return None;
        }
        if part.len() == 2 && part.chars().all(|ch| ch.is_ascii_alphabetic()) {
            canonical.push(part.to_ascii_uppercase());
        } else if part.len() == 4 && part.chars().all(|ch| ch.is_ascii_alphabetic()) {
            let mut chars = part.chars();
            canonical.push(format!(
                "{}{}",
                chars.next()?.to_ascii_uppercase(),
                chars.as_str().to_ascii_lowercase()
            ));
        } else {
            canonical.push(part.to_ascii_lowercase());
        }
    }
    Some(canonical.join("-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_matches_locale_codes() {
        install_locale("en-US", "language-self = English\nhello = Hello").unwrap();
        install_locale("zh-CN", "language-self = 简体中文\nhello = 你好").unwrap();
        assert_eq!(
            Locale::supported_from_code("zh_CN.UTF-8"),
            Some(Locale::ZH_CN)
        );
        assert_eq!(Locale::supported_from_code("en-GB"), Some(Locale::EN_US));
    }

    #[test]
    fn renders_arguments_and_falls_back_to_english() {
        install_locale(
            "en-US",
            "language-self = English\nopened = Opened { $name }",
        )
        .unwrap();
        install_locale("zh-CN", "language-self = 简体中文").unwrap();
        assert_eq!(
            I18n::new(Locale::ZH_CN).t_args("opened", &[("name", "demo.dz".into())]),
            "Opened demo.dz"
        );
    }
}
