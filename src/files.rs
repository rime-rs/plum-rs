use anyhow::{Context, Result, bail};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

pub fn safe_join(root: &Path, name: &str) -> Result<PathBuf> {
    let normalized = name.replace('\\', "/");
    if normalized.is_empty() || normalized.contains(':') || normalized.contains('\0') {
        bail!("非法相对路径: {name}");
    }
    let rel = Path::new(&normalized);
    if rel
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        bail!("路径超出目录: {name}");
    }
    let mut path = root.to_path_buf();
    for part in rel.components() {
        path.push(part);
        if let Ok(meta) = fs::symlink_metadata(&path)
            && meta.file_type().is_symlink()
        {
            bail!("不允许符号链接: {}", path.display());
        }
    }
    Ok(path)
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("文件没有父目录")?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Prepare a package first; roll back earlier writes on failure.
/// Does not guarantee multi-file atomicity in case of process/power loss.
pub fn commit(output: &Path, files: BTreeMap<String, Vec<u8>>) -> Result<usize> {
    let mut changes = Vec::new();
    for (name, bytes) in files {
        let path = safe_join(output, &name)?;
        let old = match fs::read(&path) {
            Ok(old) => Some(old),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if old.as_ref() != Some(&bytes) {
            changes.push((path, bytes, old));
        }
    }
    for (index, (path, bytes, _)) in changes.iter().enumerate() {
        if let Err(error) = atomic_write(path, bytes) {
            let mut rollback_errors = Vec::new();
            for (written, _, old) in changes[..index].iter().rev() {
                let result = match old {
                    Some(old) => atomic_write(written, old),
                    None => fs::remove_file(written).map_err(Into::into),
                };
                if let Err(e) = result {
                    rollback_errors.push(format!("{}: {e}", written.display()));
                }
            }
            bail!(
                "写入 {} 失败: {error:#}; 回滚错误: {:?}",
                path.display(),
                rollback_errors
            );
        }
        println!("安装/更新: {}", path.display());
    }
    Ok(changes.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rollback_on_later_write_failure() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.yaml"), b"original").unwrap();
        let changes = BTreeMap::from([
            ("a.yaml".into(), b"changed".to_vec()),
            ("b".into(), b"blocks parent".to_vec()),
            ("b/c.yaml".into(), b"cannot write".to_vec()),
        ]);
        assert!(commit(dir.path(), changes).is_err());
        assert_eq!(fs::read(dir.path().join("a.yaml")).unwrap(), b"original");
        assert!(!dir.path().join("b").exists());
    }

    #[test]
    fn rejects_escape_paths() {
        for path in [
            "../escape",
            "C:/escape",
            "/absolute",
            "a/../../escape",
            "a\\..\\escape",
        ] {
            assert!(safe_join(Path::new("root"), path).is_err());
        }
    }
}
