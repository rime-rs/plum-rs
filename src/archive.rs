use crate::files::safe_join;
use anyhow::{Context, Result, bail};
use std::{
    fs,
    io::{Read, Seek},
    path::{Path, PathBuf},
};

/// Extract ZIPs into an isolated directory, rejecting traversal and links.
pub fn unpack<R: Read + Seek>(reader: R, directory: &Path) -> Result<PathBuf> {
    let mut zip = zip::ZipArchive::new(reader)?;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        safe_join(directory, &entry.name()?)?;
        if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            bail!("ZIP 中不允许符号链接");
        }
        let name = entry.enclosed_name().context("ZIP 路径超出目录")?;
        let path = safe_join(directory, &name.to_string_lossy())?;
        total = total.checked_add(entry.size()).context("ZIP 文件过大")?;
        if total > 512 * 1024 * 1024 {
            bail!("ZIP 解压大小超过 512 MiB");
        }
        if entry.is_dir() {
            fs::create_dir_all(path)?;
        } else {
            fs::create_dir_all(path.parent().unwrap())?;
            let mut output = fs::File::create(path)?;
            let size = entry.size();
            let copied = std::io::copy(&mut entry.by_ref().take(size + 1), &mut output)?;
            if copied != size {
                bail!("ZIP 文件大小不匹配");
            }
        }
    }
    let entries = fs::read_dir(directory)?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .filter(|e| {
            !e.file_name().to_string_lossy().starts_with('.') && e.file_name() != "__MACOSX"
        })
        .collect::<Vec<_>>();
    if entries.len() == 1
        && entries[0].file_type()?.is_dir()
        && !matches!(
            entries[0].file_name().to_str(),
            Some("opencc" | "lua" | "dict" | "dicts" | "build" | "schemas")
        )
    {
        Ok(entries[0].path())
    } else {
        Ok(directory.into())
    }
}

pub fn install(
    source: &str,
    plum: &Path,
    output: &Path,
    no_update: bool,
    recursive: bool,
) -> Result<usize> {
    fs::create_dir_all(plum)?;
    let temp = tempfile::tempdir_in(plum)?;
    let root = if source.starts_with("https://") || source.starts_with("http://") {
        let bytes = ureq::get(source)
            .call()?
            .into_body()
            .with_config()
            .limit(128 * 1024 * 1024)
            .read_to_vec()?;
        unpack(std::io::Cursor::new(bytes), temp.path())?
    } else {
        unpack(
            fs::File::open(source).with_context(|| format!("读取 ZIP: {source}"))?,
            temp.path(),
        )?
    };
    let recipe = root.join("recipe.yaml");
    if recipe.is_file() {
        crate::recipe::install_recipe(&recipe, &root, output, &[], no_update)
    } else if recursive {
        crate::recipe::install_recursive_files(&root, output)
    } else {
        crate::recipe::install_default_files(&root, output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    fn archive(name: &str) -> Cursor<Vec<u8>> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"schema: test").unwrap();
        zip.finish().unwrap()
    }
    #[test]
    fn extracts_wrapper_and_rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let root = unpack(archive("repo-main/a.yaml"), dir.path()).unwrap();
        assert!(root.join("a.yaml").is_file());
        assert!(unpack(archive("../escape.yaml"), dir.path()).is_err());
        assert!(unpack(archive("C:/escape.yaml"), dir.path()).is_err());
    }
    #[test]
    fn unwraps_lua_only_repositories_but_preserves_data_directories() {
        let repo = tempfile::tempdir().unwrap();
        let root = unpack(archive("repo-main/lua/tool.lua"), repo.path()).unwrap();
        assert!(root.join("lua/tool.lua").is_file());
        let data = tempfile::tempdir().unwrap();
        assert_eq!(
            unpack(archive("opencc/data.json"), data.path()).unwrap(),
            data.path()
        );
    }
}
