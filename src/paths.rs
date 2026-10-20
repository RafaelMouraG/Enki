use std::path::PathBuf;

const APP_DIR: &str = "Enki";

/// Settings live here: `~/Library/Application Support/Enki` or `~/.config/Enki`.
pub fn config_dir() -> PathBuf {
    std::env::var_os("ENKI_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::config_dir().map(|d| d.join(APP_DIR)))
        .unwrap_or_else(|| PathBuf::from(".enki"))
}

/// The session database lives here.
pub fn data_dir() -> PathBuf {
    std::env::var_os("ENKI_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|d| d.join(APP_DIR)))
        .unwrap_or_else(|| PathBuf::from(".enki"))
}

/// Disposable files: processed wallpapers, album art.
pub fn cache_dir() -> PathBuf {
    std::env::var_os("ENKI_HOME")
        .map(|home| PathBuf::from(home).join("cache"))
        .or_else(|| dirs::cache_dir().map(|d| d.join(APP_DIR)))
        .unwrap_or_else(|| PathBuf::from(".enki/cache"))
}
