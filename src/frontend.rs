use std::path::PathBuf;

pub fn default_plum_dir() -> Option<PathBuf> {
    dirs_next::cache_dir().map(|p| p.join("plum-rs"))
}

#[cfg(windows)]
fn weasel_user_dir() -> Option<PathBuf> {
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY},
    };
    let root = RegKey::predef(HKEY_CURRENT_USER);
    for view in [0, KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        if let Ok(key) = root.open_subkey_with_flags("Software\\Rime\\Weasel", KEY_READ | view)
            && let Ok(value) = key.get_value::<String, _>("RimeUserDir")
            && !value.trim().is_empty()
        {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                return Some(path);
            }
        }
    }
    None
}

pub fn guess_rime_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        return dirs_next::home_dir().map(|p| p.join("Library/Rime"));
    }
    #[cfg(windows)]
    {
        if let Some(path) = weasel_user_dir() {
            return Some(path);
        }
        return std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("Rime"));
    }
    #[cfg(target_os = "linux")]
    {
        let home = dirs_next::home_dir()?;
        let paths = [
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share"))
                .join("fcitx5/rime"),
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"))
                .join("fcitx/rime"),
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"))
                .join("ibus/rime"),
        ];
        return paths.into_iter().find(|p| p.is_dir());
    }
    #[allow(unreachable_code)]
    None
}
