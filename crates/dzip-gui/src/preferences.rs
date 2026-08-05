use dzip_gui::model::{CompressionChoice, DzCompressionOptions};
use serde::{Deserialize, Serialize};

const THEME_KEY: &str = "theme";
const LOCALE_KEY: &str = "locale";
const ARCHIVE_PREFERENCES_KEY: &str = "archive-preferences";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArchivePreferences {
    pub compression: CompressionChoice,
    pub alignment: u32,
    pub random_access: bool,
    pub dz_options: DzCompressionOptions,
}

impl Default for ArchivePreferences {
    fn default() -> Self {
        Self {
            compression: CompressionChoice::Dz,
            alignment: 0,
            random_access: false,
            dz_options: DzCompressionOptions::default(),
        }
    }
}

pub fn read_theme() -> Option<String> {
    read_preference(THEME_KEY)
}

pub fn save_theme(value: &str) -> Result<(), String> {
    save_preference(THEME_KEY, value)
}

pub fn read_locale() -> Option<String> {
    read_preference(LOCALE_KEY)
}

pub fn save_locale(value: &str) -> Result<(), String> {
    save_preference(LOCALE_KEY, value)
}

pub fn read_archive_preferences() -> ArchivePreferences {
    read_preference(ARCHIVE_PREFERENCES_KEY)
        .as_deref()
        .and_then(decode_archive_preferences)
        .unwrap_or_default()
}

pub fn save_archive_preferences(value: &ArchivePreferences) -> Result<(), String> {
    let value = serde_json::to_string(value).map_err(|error| error.to_string())?;
    save_preference(ARCHIVE_PREFERENCES_KEY, &value)
}

/// Persist editor preferences without blocking a render/input event.
#[cfg(feature = "desktop")]
pub fn queue_archive_preferences(value: ArchivePreferences) -> Result<(), String> {
    use std::sync::{OnceLock, mpsc};
    use std::time::Duration;

    static WRITER: OnceLock<mpsc::Sender<ArchivePreferences>> = OnceLock::new();
    let writer = WRITER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<ArchivePreferences>();
        std::thread::Builder::new()
            .name("dzip-preference-writer".to_string())
            .spawn(move || {
                while let Ok(mut pending) = receiver.recv() {
                    loop {
                        match receiver.recv_timeout(Duration::from_millis(250)) {
                            Ok(latest) => pending = latest,
                            Err(mpsc::RecvTimeoutError::Timeout) => break,
                            Err(mpsc::RecvTimeoutError::Disconnected) => {
                                let _ = save_archive_preferences(&pending);
                                return;
                            }
                        }
                    }
                    if let Err(error) = save_archive_preferences(&pending) {
                        log::error!(target: "dzip_gui::preferences", "{error}");
                    }
                }
            })
            .expect("failed to start preference writer");
        sender
    });
    writer
        .send(value)
        .map_err(|_| "preference writer is unavailable".to_string())
}

#[cfg(feature = "web")]
pub fn queue_archive_preferences(value: ArchivePreferences) -> Result<(), String> {
    save_archive_preferences(&value)
}

fn decode_archive_preferences(value: &str) -> Option<ArchivePreferences> {
    serde_json::from_str(value)
        .ok()
        .map(sanitize_archive_preferences)
}

fn sanitize_archive_preferences(mut value: ArchivePreferences) -> ArchivePreferences {
    let defaults = ArchivePreferences::default();
    if !matches!(value.alignment, 0 | 512 | 2048 | 4096) {
        value.alignment = defaults.alignment;
    }
    value.dz_options = value.dz_options.sanitized();

    value
}

#[cfg(feature = "desktop")]
fn read_preference(key: &str) -> Option<String> {
    let path = preference_path(key)?;
    std::fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(feature = "desktop")]
fn save_preference(key: &str, value: &str) -> Result<(), String> {
    let path = preference_path(key).ok_or_else(|| "无法确定 Dzip Archive 设置目录".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, value).map_err(|error| error.to_string())
}

#[cfg(feature = "desktop")]
fn preference_path(key: &str) -> Option<std::path::PathBuf> {
    let base = if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(std::path::PathBuf::from)?
            .join("Library/Application Support")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(std::path::PathBuf::from)?
    } else if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        std::path::PathBuf::from(config)
    } else {
        std::env::var_os("HOME")
            .map(std::path::PathBuf::from)?
            .join(".config")
    };
    Some(base.join("dzip-archive").join(format!("{key}.txt")))
}

#[cfg(feature = "web")]
fn read_preference(key: &str) -> Option<String> {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(&web_key(key)).ok().flatten())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(feature = "web")]
fn save_preference(key: &str, value: &str) -> Result<(), String> {
    let storage = web_sys::window()
        .ok_or_else(|| "浏览器窗口不可用".to_string())?
        .local_storage()
        .map_err(|error| format!("{error:?}"))?
        .ok_or_else(|| "浏览器本地存储不可用".to_string())?;
    storage
        .set_item(&web_key(key), value)
        .map_err(|error| format!("{error:?}"))
}

#[cfg(feature = "web")]
fn web_key(key: &str) -> String {
    format!("dzip-archive-{key}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_preferences_round_trip() {
        let mut expected = ArchivePreferences {
            compression: CompressionChoice::Lzma,
            alignment: 2048,
            random_access: true,
            dz_options: DzCompressionOptions::default(),
        };
        expected.dz_options.win_size = 18;
        expected.dz_options.trim_reference_factor = 42;

        let encoded = serde_json::to_string(&expected).unwrap();
        assert_eq!(decode_archive_preferences(&encoded), Some(expected));
    }

    #[test]
    fn invalid_archive_preferences_are_rejected_or_sanitized() {
        assert_eq!(decode_archive_preferences("not json"), None);

        let stored = ArchivePreferences {
            alignment: 7,
            dz_options: DzCompressionOptions {
                win_size: 31,
                offset_contexts: 0,
                ..DzCompressionOptions::default()
            },
            ..ArchivePreferences::default()
        };
        let encoded = serde_json::to_string(&stored).unwrap();
        let decoded = decode_archive_preferences(&encoded).unwrap();

        assert_eq!(decoded.alignment, 0);
        assert_eq!(decoded.dz_options.win_size, 16);
        assert_eq!(decoded.dz_options.offset_contexts, 3);
    }
}
