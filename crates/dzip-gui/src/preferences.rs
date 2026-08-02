const THEME_KEY: &str = "theme";
const LOCALE_KEY: &str = "locale";

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
