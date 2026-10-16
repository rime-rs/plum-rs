use anyhow::{Context, Result, bail};
use std::{collections::BTreeMap, fs, path::Path};

pub fn collect(root: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<String, Vec<u8>>) -> Result<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                bail!("构建产物不能包含符号链接");
            }
            if kind.is_dir() {
                visit(root, &entry.path(), files)?;
            } else {
                files.insert(
                    entry
                        .path()
                        .strip_prefix(root)?
                        .to_string_lossy()
                        .replace('\\', "/"),
                    fs::read(entry.path())?,
                );
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

pub fn publish(stage: &Path, output: &Path) -> Result<()> {
    let mut files = collect(stage)?;
    let previous = fs::read_to_string(output.join(".plum-rs-build-files")).unwrap_or_default();
    let stale = previous
        .lines()
        .filter(|name| !files.contains_key(*name))
        .map(|name| crate::files::safe_join(output, name))
        .collect::<Result<Vec<_>>>()?;
    let manifest = files.keys().cloned().collect::<Vec<_>>().join("\n");
    files.insert(".plum-rs-build-files".into(), manifest.into_bytes());
    crate::files::commit(output, files)?;
    for path in stale {
        if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn clean(output: &Path) -> Result<()> {
    let marker = output.join(".plum-rs-build-files");
    let content = fs::read_to_string(&marker).context("没有构建清单，拒绝清理未知目录")?;
    let paths = content
        .lines()
        .map(|name| crate::files::safe_join(output, name))
        .collect::<Result<Vec<_>>>()?;
    for path in paths {
        if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    fs::remove_file(marker)?;
    Ok(())
}

pub fn compile(output: &Path) -> Result<()> {
    let status = std::process::Command::new("rime_deployer")
        .arg("--build")
        .arg(output)
        .status()
        .context("编译方案需要安装 librime 的 rime_deployer")?;
    if !status.success() {
        bail!("rime_deployer 编译失败: {status}");
    }
    let user = output.join("user.yaml");
    if user.is_file() {
        fs::remove_file(user)?;
    }
    Ok(())
}

fn minimal_version(line: &str, key: &str) -> String {
    if let Some(value) = line.strip_prefix(key) {
        let value = value.trim();
        let quoted = value.starts_with('"') && value.ends_with('"');
        let version = value.trim_matches('"');
        if !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return format!(
                "{key} {}{version}.minimal{}",
                if quoted { "\"" } else { "" },
                if quoted { "\"" } else { "" }
            );
        }
    }
    line.into()
}

pub fn minimal(output: &Path) -> Result<()> {
    let essay = output.join("essay.txt");
    let content = fs::read_to_string(&essay)?;
    let filtered = content
        .lines()
        .filter(|line| {
            line.split_whitespace()
                .nth(1)
                .and_then(|n| n.parse::<f64>().ok())
                .unwrap_or(0.0)
                >= 500.0
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    crate::files::atomic_write(&essay, filtered.as_bytes())?;
    let dictionary = output.join("luna_pinyin.dict.yaml");
    let content = fs::read_to_string(&dictionary)?;
    let filtered = content
        .lines()
        .take_while(|line| *line != "#以下爲詞組")
        .map(|line| minimal_version(line, "version:"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    crate::files::atomic_write(&dictionary, filtered.as_bytes())?;
    let mut schemas = Vec::new();
    for entry in fs::read_dir(output)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(schema) = name.strip_suffix(".schema.yaml") {
            schemas.push(schema.to_string());
            let content = fs::read_to_string(entry.path())?;
            let transformed = content
                .lines()
                .map(|line| {
                    let line = minimal_version(line, "version:");
                    if line.ends_with("- stroke") || line.ends_with("- reverse_lookup_translator") {
                        line.replace("- stroke", "#- stroke").replace(
                            "- reverse_lookup_translator",
                            "#- reverse_lookup_translator",
                        )
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            crate::files::atomic_write(&entry.path(), transformed.as_bytes())?;
        }
    }
    let default = output.join("default.yaml");
    let content = fs::read_to_string(&default)?;
    let selected = content
        .lines()
        .filter(|line| {
            schemas
                .iter()
                .any(|schema| line.contains(&format!("  - schema: {schema}")))
        })
        .collect::<Vec<_>>();
    let mut transformed = String::new();
    for line in content.lines() {
        if line.contains("- schema:") {
            continue;
        }
        transformed.push_str(&minimal_version(line, "config_version:"));
        transformed.push('\n');
        if line == "schema_list:" {
            for selected in &selected {
                transformed.push_str(selected);
                transformed.push('\n');
            }
        }
    }
    crate::files::atomic_write(&default, transformed.as_bytes())?;
    Ok(())
}

pub fn install_shared(output: &Path, destination: &Path) -> Result<()> {
    let files = collect(output)?
        .into_iter()
        .filter(|(name, _)| {
            let parts = name.split('/').collect::<Vec<_>>();
            (parts.len() == 1 || (parts.len() == 2 && matches!(parts[0], "build" | "opencc")))
                && parts
                    .last()
                    .is_some_and(|part| part.contains('.') && !part.starts_with('.'))
        })
        .collect::<BTreeMap<_, _>>();
    let names = files.keys().cloned().collect::<Vec<_>>();
    crate::files::commit(destination, files)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in names {
            fs::set_permissions(
                crate::files::safe_join(destination, &name)?,
                fs::Permissions::from_mode(0o644),
            )?;
        }
    }
    #[cfg(not(unix))]
    let _ = names;
    Ok(())
}

pub fn dist(source: &Path, plum: &Path, destination: &Path) -> Result<()> {
    if !source.join("Cargo.toml").is_file() {
        bail!("--dist 需要 plum-rs 源码目录，请使用 --source-dir");
    }
    let mut archive = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::default(),
    ));
    fn append(
        builder: &mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>,
        root: &Path,
        path: &Path,
        prefix: &str,
        destination: &Path,
    ) -> Result<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name();
            let path = entry.path();
            let kind = entry.file_type()?;
            let archive_name = name.to_string_lossy();
            if name == ".git"
                || (archive_name.starts_with("plum-rs-") && archive_name.ends_with(".tar.gz"))
                || (path.parent() == Some(root)
                    && matches!(name.to_str(), Some("target" | "output" | ".plum")))
                || path == destination
            {
                continue;
            }
            if kind.is_symlink() {
                bail!("源码归档不支持符号链接: {}", path.display());
            }
            if kind.is_dir() {
                append(builder, root, &path, prefix, destination)?;
            } else {
                builder.append_path_with_name(
                    &path,
                    Path::new(prefix).join(path.strip_prefix(root)?),
                )?;
            }
        }
        Ok(())
    }
    append(&mut archive, source, source, "plum-rs", destination)?;
    if plum.join("package").is_dir() {
        append(
            &mut archive,
            &plum.join("package"),
            &plum.join("package"),
            "plum-rs/.plum/package",
            destination,
        )?;
    }
    for name in [
        "preset-packages.conf",
        "extra-packages.conf",
        "all-packages.conf",
    ] {
        if plum.join(name).is_file() {
            archive.append_path_with_name(plum.join(name), format!("plum-rs/.plum/{name}"))?;
        }
    }
    let bytes = archive.into_inner()?.finish()?;
    crate::files::atomic_write(destination, &bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minimal_transforms_and_clean_preserves_unrelated_files() {
        let temp = tempfile::tempdir().unwrap();
        let stage = temp.path().join("stage");
        let output = temp.path().join("out");
        fs::create_dir_all(&stage).unwrap();
        fs::create_dir_all(&output).unwrap();
        for (name, body) in [
            ("essay.txt", "common\t500\nrare\t499\n"),
            (
                "luna_pinyin.dict.yaml",
                "version: \"1.0\"\n#以下爲詞組\nremove\n",
            ),
            (
                "luna.schema.yaml",
                "version: 1.0\n  - stroke\n  - reverse_lookup_translator\n",
            ),
            (
                "default.yaml",
                "config_version: 1.0\nschema_list:\n  - schema: luna\n  - schema: missing\n",
            ),
        ] {
            fs::write(stage.join(name), body).unwrap();
        }
        minimal(&stage).unwrap();
        assert_eq!(
            fs::read_to_string(stage.join("essay.txt")).unwrap(),
            "common\t500\n"
        );
        assert!(
            !fs::read_to_string(stage.join("luna_pinyin.dict.yaml"))
                .unwrap()
                .contains("remove")
        );
        assert!(
            !fs::read_to_string(stage.join("default.yaml"))
                .unwrap()
                .contains("missing")
        );
        publish(&stage, &output).unwrap();
        fs::write(output.join("user-file"), "preserve").unwrap();
        clean(&output).unwrap();
        assert!(output.join("user-file").is_file());
        assert!(!output.join("essay.txt").exists());
    }
}
