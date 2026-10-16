use anyhow::{Result, bail};
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
    for view in [0, KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("Software\\Rime\\Weasel", KEY_READ | view)
            && let Ok(value) = key.get_value::<String, _>("RimeUserDir")
            && !value.is_empty()
        {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                return Some(path);
            }
        }
    }
    None
}

fn frontend_path(
    name: &str,
    home: &std::path::Path,
    appdata: Option<PathBuf>,
) -> Result<Option<PathBuf>> {
    Ok(Some(match name {
        "fcitx/fcitx-rime" | "fcitx-rime" => home.join(".config/fcitx/rime"),
        "fcitx5/fcitx5-rime" | "fcitx5-rime" => home.join(".local/share/fcitx5/rime"),
        "rime/ibus-rime" | "ibus-rime" => home.join(".config/ibus/rime"),
        "rime/squirrel" | "squirrel" => home.join("Library/Rime"),
        "rime/weasel" | "weasel" => {
            let Some(appdata) = appdata else {
                return Ok(None);
            };
            appdata.join("Rime")
        }
        _ => bail!("未知 rime_frontend: {name}，请显式指定 --rime-dir"),
    }))
}

pub fn guess_rime_dir(frontend: Option<&str>) -> Result<Option<PathBuf>> {
    let Some(home) = dirs_next::home_dir() else {
        return Ok(None);
    };
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    if let Some(frontend) = frontend {
        return frontend_path(frontend, &home, appdata);
    }
    #[cfg(windows)]
    {
        return Ok(weasel_user_dir().or_else(|| appdata.map(|p| p.join("Rime"))));
    }
    #[cfg(target_os = "macos")]
    {
        return frontend_path("squirrel", &home, appdata);
    }
    #[cfg(target_os = "linux")]
    {
        return frontend_path("ibus-rime", &home, appdata);
    }
    #[allow(unreachable_code)]
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_frontends_do_not_require_existing_directories() {
        let home = std::path::Path::new("/nonexistent/home");
        assert_eq!(
            frontend_path("fcitx5-rime", home, None).unwrap(),
            Some(home.join(".local/share/fcitx5/rime"))
        );
        assert_eq!(
            frontend_path("ibus-rime", home, None).unwrap(),
            Some(home.join(".config/ibus/rime"))
        );
        assert!(frontend_path("unknown", home, None).is_err());
    }
}
