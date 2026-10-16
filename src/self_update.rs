use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

fn checksum(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn version(s: &str) -> Result<Vec<u64>> {
    s.trim_start_matches('v')
        .split('.')
        .map(|p| p.parse::<u64>().context("无法识别发行版本"))
        .collect()
}

pub fn replace_verified(path: &Path, bytes: &[u8], expected: &str) -> Result<()> {
    if !checksum(bytes).eq_ignore_ascii_case(expected) {
        bail!("SHA256 不匹配，保留当前程序");
    }
    let parent = path.parent().context("程序没有父目录")?;
    let backup = tempfile::NamedTempFile::new_in(parent)?;
    let backup_path = backup.path().to_path_buf();
    backup.close()?;
    let permissions = std::fs::metadata(path)?.permissions();
    std::fs::rename(path, &backup_path).context("无法备份当前程序")?;
    if let Err(error) = crate::files::atomic_write(path, bytes)
        .and_then(|_| std::fs::set_permissions(path, permissions).map_err(Into::into))
    {
        // Atomic write can succeed before chmod fails; remove only the new file.
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        std::fs::rename(&backup_path, path).context("恢复旧程序失败")?;
        return Err(error);
    }
    if std::fs::remove_file(&backup_path).is_err() {
        eprintln!("旧程序暂保留于 {}，退出后可删除", backup_path.display());
    }
    Ok(())
}

pub fn update() -> Result<()> {
    let content = ureq::get("https://api.github.com/repos/rime-rs/plum-rs/releases/latest")
        .header("User-Agent", "plum-rs")
        .call()?
        .into_body()
        .read_to_string()?;
    let release: Release = yaml_serde::from_str(&content)?;
    if version(&release.tag_name)? <= version(env!("CARGO_PKG_VERSION"))? {
        println!("当前已是最新版本: {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let asset_name = format!("rime-install-{}{suffix}", env!("PLUM_TARGET"));
    let binary = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .context("该版本没有当前平台的可执行文件")?;
    let sums = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS.txt")
        .context("发行版本没有 SHA256SUMS.txt")?;
    for asset in [binary, sums] {
        let url = url::Url::parse(&asset.browser_download_url)?;
        if url.scheme() != "https"
            || url.host_str() != Some("github.com")
            || !url
                .path()
                .starts_with("/rime-rs/plum-rs/releases/download/")
        {
            bail!("拒绝非官方发行地址");
        }
    }
    let sums = ureq::get(&sums.browser_download_url)
        .call()?
        .into_body()
        .read_to_string()?;
    let expected = sums
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            let sum = fields.next()?;
            let name = fields.next()?.trim_start_matches('*');
            (name == asset_name).then_some(sum)
        })
        .context("校验清单中没有当前程序")?;
    let bytes = ureq::get(&binary.browser_download_url)
        .call()?
        .into_body()
        .with_config()
        .limit(128 * 1024 * 1024)
        .read_to_vec()?;
    replace_verified(&std::env::current_exe()?, &bytes, expected)?;
    println!("已更新到 {}，请重新运行程序", release.tag_name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_bad_hash_and_replaces_verified_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("program");
        std::fs::write(&path, b"old").unwrap();
        assert!(replace_verified(&path, b"new", "bad").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        replace_verified(&path, b"new", &checksum(b"new")).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }
}
