use crate::{
    i18n::{self, Locale},
    preferences,
};

const SOURCES: [(&str, &str); 5] = [
    ("en-US", include_str!("../assets/i18n/en-US.ftl")),
    ("es-ES", include_str!("../assets/i18n/es-ES.ftl")),
    ("fr-FR", include_str!("../assets/i18n/fr-FR.ftl")),
    ("ru-RU", include_str!("../assets/i18n/ru-RU.ftl")),
    ("zh-CN", include_str!("../assets/i18n/zh-CN.ftl")),
];

pub fn load_i18n_sources() -> usize {
    SOURCES
        .into_iter()
        .filter(|(code, source)| match i18n::install_locale(code, source) {
            Ok(_) => true,
            Err(error) => {
                log::warn!(target: "dzip_gui::i18n", "Failed to load locale {code}: {error}");
                false
            }
        })
        .count()
}

pub fn initial_locale() -> Locale {
    resolve_locale(
        preferences::read_locale().as_deref(),
        system_locale().as_deref(),
    )
}

fn resolve_locale(preference: Option<&str>, system_locale: Option<&str>) -> Locale {
    preference
        .and_then(Locale::supported_from_code)
        .or_else(|| system_locale.and_then(Locale::supported_from_code))
        .unwrap_or(Locale::EN_US)
}

#[cfg(feature = "web")]
fn system_locale() -> Option<String> {
    web_sys::window().and_then(|window| window.navigator().language())
}

#[cfg(feature = "desktop")]
fn system_locale() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"]
        .iter()
        .find_map(|key| std::env::var(key).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::I18n;
    use std::collections::BTreeSet;

    #[test]
    fn preference_wins_over_system_locale() {
        assert_eq!(load_i18n_sources(), 5);
        assert_eq!(resolve_locale(Some("zh-CN"), Some("en-US")), Locale::ZH_CN);
        assert_eq!(resolve_locale(None, Some("en-GB")), Locale::EN_US);
        assert_eq!(resolve_locale(None, Some("fr-CA")), Locale::FR_FR);
        assert_eq!(resolve_locale(None, Some("ru")), Locale::RU_RU);
        assert_eq!(resolve_locale(None, Some("es-MX")), Locale::ES_ES);
    }

    #[test]
    fn all_locales_have_complete_resources_and_native_labels() {
        assert_eq!(load_i18n_sources(), 5);
        let english_keys = message_ids(SOURCES[0].1);
        for (code, source) in SOURCES.iter().skip(1) {
            assert_eq!(message_ids(source), english_keys, "key mismatch in {code}");
        }

        let options = i18n::language_options();
        let labels = options
            .iter()
            .map(|option| (option.locale.code(), option.label.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec![
                ("en-US", "English"),
                ("es-ES", "Español"),
                ("fr-FR", "Français"),
                ("ru-RU", "Русский"),
                ("zh-CN", "简体中文"),
            ]
        );
        assert_eq!(I18n::new(Locale::ES_ES).t("settings"), "Configuración");
        assert_eq!(I18n::new(Locale::FR_FR).t("settings"), "Paramètres");
        assert_eq!(I18n::new(Locale::RU_RU).t("settings"), "Настройки");
    }

    fn message_ids(source: &str) -> BTreeSet<&str> {
        source
            .lines()
            .filter_map(|line| line.split_once(" =").map(|(key, _)| key))
            .collect()
    }
}
