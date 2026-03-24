use std::path::PathBuf;

/// 对应原 frontend.sh 的 guess_rime_user_dir
/// 按前端优先级依次探测
pub fn guess_rime_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        // Squirrel
        let p = dirs_next::home_dir()?.join("Library/Rime");
        if p.exists() {
            return Some(p);
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Weasel
        if let Ok(appdata) = std::env::var("APPDATA") {
            let p = PathBuf::from(appdata).join("Rime");
            if p.exists() {
                return Some(p);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let home = dirs_next::home_dir()?;
        // fcitx5-rime
        let p = home.join(".local/share/fcitx5/rime");
        if p.exists() {
            return Some(p);
        }
        // fcitx-rime
        let p = home.join(".config/fcitx/rime");
        if p.exists() {
            return Some(p);
        }
        // ibus-rime
        let p = home.join(".config/ibus/rime");
        if p.exists() {
            return Some(p);
        }
    }

    None
}
