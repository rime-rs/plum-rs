use crate::files::{commit, safe_join};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};
use yaml_serde::{Value, from_str};

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum StringOrVec {
    String(String),
    Vec(Vec<String>),
}
impl StringOrVec {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            Self::String(s) => s.split_whitespace().map(str::to_string).collect(),
            Self::Vec(v) => v,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub recipe: Option<Value>,
    pub install_files: Option<StringOrVec>,
    pub download_files: Option<StringOrVec>,
    pub patch_files: Option<BTreeMap<String, Value>>,
    // Explicitly diagnose this previously silently ignored field.
    pub patch: Option<Value>,
}

pub fn load_recipe(file: &Path) -> Result<Recipe> {
    from_str(&std::fs::read_to_string(file)?)
        .with_context(|| format!("解析 recipe.yaml 失败: {}", file.display()))
}

/// Expand named options only. Shell commands and unknown variables fail before writes.
fn expand(
    text: &str,
    options: &HashMap<String, String>,
    used: &mut HashSet<String>,
) -> Result<String> {
    if text.contains('`') || text.contains("$(") {
        bail!("不支持 recipe 中的 Shell 命令执行");
    }
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        // Regex anchors and literal dollar signs are common in Rime patches.
        if !chars
            .peek()
            .is_some_and(|c| *c == '{' || c.is_ascii_alphanumeric() || *c == '_')
        {
            out.push('$');
            continue;
        }
        let braced = chars.peek() == Some(&'{');
        let mut expr = String::new();
        if braced {
            chars.next();
            loop {
                match chars.next() {
                    Some('}') => break,
                    Some(c) => expr.push(c),
                    None => bail!("recipe 变量未闭合"),
                }
            }
        } else {
            while let Some(c) = chars.peek() {
                if c.is_ascii_alphanumeric() || *c == '_' {
                    expr.push(*c);
                    chars.next();
                } else {
                    break;
                }
            }
        }
        let (key, default) = expr
            .split_once(":-")
            .map_or((expr.as_str(), None), |(k, v)| (k, Some(v)));
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            bail!("不支持 recipe 变量: {expr}");
        }
        let value = options
            .get(key)
            .filter(|v| default.is_none() || !v.is_empty())
            .map(String::as_str)
            .or(default)
            .with_context(|| format!("recipe 选项未提供: {key}"))?;
        used.insert(key.into());
        out.push_str(value);
    }
    Ok(out)
}

fn expand_value(
    value: Value,
    options: &HashMap<String, String>,
    used: &mut HashSet<String>,
) -> Result<Value> {
    Ok(match value {
        Value::String(s) => {
            let expanded = expand(&s, options, used)?;
            if s.starts_with('$') && !s[1..].contains('$') && expanded != s {
                match from_str::<Value>(&expanded) {
                    Ok(value @ (Value::Number(_) | Value::Bool(_) | Value::Null)) => value,
                    _ => Value::String(expanded),
                }
            } else {
                Value::String(expanded)
            }
        }
        Value::Sequence(v) => Value::Sequence(
            v.into_iter()
                .map(|v| expand_value(v, options, used))
                .collect::<Result<_>>()?,
        ),
        Value::Mapping(m) => {
            let mut output = yaml_serde::Mapping::new();
            for (k, v) in m {
                output.insert(
                    expand_value(k, options, used)?,
                    expand_value(v, options, used)?,
                );
            }
            Value::Mapping(output)
        }
        Value::Tagged(_) => bail!("不支持 recipe 补丁中的 YAML 标签"),
        v => v,
    })
}

pub fn install_recipe(
    file: &Path,
    package: &Path,
    output: &Path,
    options: &[(String, String)],
    no_update: bool,
) -> Result<usize> {
    let recipe = load_recipe(file)?;
    if recipe.patch.is_some() {
        bail!("不支持 recipe.patch；请使用 patch_files: 文件名: 补丁映射");
    }
    if let Some(info) = &recipe.recipe
        && let Some(rx) = info.get("Rx").and_then(Value::as_str)
        && let Some(stem) = file
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_suffix(".recipe.yaml"))
        && rx != stem
    {
        bail!("recipe.Rx '{rx}' 与文件名 '{stem}' 不一致");
    }
    let vars: HashMap<_, _> = options.iter().cloned().collect();
    let mut used = HashSet::new();
    let mut downloads = Vec::new();
    for spec in recipe
        .download_files
        .map(StringOrVec::into_vec)
        .unwrap_or_default()
    {
        let spec = expand(&spec, &vars, &mut used)?;
        let (name, url) = if let Some(pair) = spec.split_once("::") {
            (pair.0.to_string(), pair.1.to_string())
        } else {
            let url = url::Url::parse(&spec)?;
            let name = url
                .path_segments()
                .and_then(|mut s| s.next_back())
                .filter(|s| !s.is_empty())
                .context("下载 URL 缺少文件名")?
                .to_string();
            (name, spec)
        };
        let parsed = url::Url::parse(&url)?;
        if !matches!(parsed.scheme(), "http" | "https") {
            bail!("不支持下载协议: {url}");
        }
        safe_join(package, &name)?;
        downloads.push((name, url));
    }
    let patterns = recipe
        .install_files
        .map(StringOrVec::into_vec)
        .map(|patterns| {
            patterns
                .into_iter()
                .map(|p| expand(&p, &vars, &mut used))
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?;
    let mut patches = BTreeMap::new();
    for (name, patch) in recipe.patch_files.unwrap_or_default() {
        let name = expand(&name, &vars, &mut used)?;
        safe_join(output, &name)?;
        let patch = expand_value(patch, &vars, &mut used)?;
        if !patch.is_mapping() {
            bail!("patch_files.{name} 必须是 YAML 映射；不支持 Shell 脚本");
        }
        patches.insert(name, patch);
    }
    for (key, _) in options {
        if !used.contains(key) {
            bail!("未使用的 recipe 选项: {key}");
        }
    }
    // Validate all declarations before network or installation side effects.
    if let Some(patterns) = &patterns {
        for pattern in patterns {
            safe_join(package, pattern)?;
            glob::Pattern::new(pattern)?;
        }
    }
    for (name, url) in downloads {
        let path = safe_join(package, &name)?;
        if no_update && path.is_file() {
            continue;
        }
        println!("下载文件: {url}");
        let bytes = ureq::get(&url)
            .call()?
            .into_body()
            .with_config()
            .limit(128 * 1024 * 1024)
            .read_to_vec()?;
        crate::files::atomic_write(&path, &bytes)?;
    }
    let patterns = match patterns {
        Some(p) => p,
        None => collect_default_files(package)?,
    };
    let mut files = collect_files(&patterns, package)?;
    for (name, patch) in patches {
        let existing = if let Some(bytes) = files.get(&name) {
            Some(bytes.clone())
        } else {
            match std::fs::read(safe_join(output, &name)?) {
                Ok(b) => Some(b),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
            }
        };
        let mut document: Value = match existing {
            Some(b) => {
                yaml_serde::from_slice(&b).with_context(|| format!("补丁目标不是 YAML: {name}"))?
            }
            None => Value::Mapping(Default::default()),
        };
        let mapping = document
            .as_mapping_mut()
            .context("补丁目标必须是 YAML 映射")?;
        let key = Value::String("__patch".into());
        if !mapping.contains_key(&key) {
            mapping.insert(key.clone(), Value::Mapping(Default::default()));
        }
        let target = mapping
            .get_mut(&key)
            .unwrap()
            .as_mapping_mut()
            .context("现有 __patch 必须是 YAML 映射")?;
        for (k, v) in patch.as_mapping().unwrap() {
            target.insert(k.clone(), v.clone());
        }
        files.insert(name, yaml_serde::to_string(&document)?.into_bytes());
    }
    commit(output, files)
}

pub fn install_default_files(package: &Path, output: &Path) -> Result<usize> {
    commit(
        output,
        collect_files(&collect_default_files(package)?, package)?,
    )
}

fn collect_default_files(package: &Path) -> Result<Vec<String>> {
    fn visit(root: &Path, dir: &Path, files: &mut Vec<String>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if ty.is_symlink()
                || name.starts_with('.')
                || matches!(name.as_str(), "build" | "target")
            {
                continue;
            }
            if ty.is_dir() {
                visit(root, &entry.path(), files)?;
            } else if ty.is_file() {
                let ext = entry
                    .path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if matches!(
                    ext.as_str(),
                    "yaml" | "txt" | "gram" | "lua" | "json" | "ocd" | "ocd2"
                ) && !name.ends_with(".custom.yaml")
                    && !name.ends_with(".recipe.yaml")
                    && name != "recipe.yaml"
                {
                    files.push(
                        entry
                            .path()
                            .strip_prefix(root)?
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(package, package, &mut files)?;
    Ok(files)
}

fn collect_files(patterns: &[String], package: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut files = BTreeMap::new();
    let root = glob::Pattern::escape(&package.to_string_lossy().replace('\\', "/"));
    for pattern in patterns {
        safe_join(package, pattern)?;
        let mut matched = false;
        for entry in glob::glob(&format!("{root}/{}", pattern.replace('\\', "/")))? {
            let entry = entry?;
            if !entry.is_file() {
                continue;
            }
            let relative = entry
                .strip_prefix(package)?
                .to_string_lossy()
                .replace('\\', "/");
            let checked = safe_join(package, &relative)?;
            files.insert(relative, std::fs::read(checked)?);
            matched = true;
        }
        if !matched {
            bail!("匹配不到 install_files 文件: {pattern}");
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn downloads_named_file_then_installs_and_respects_no_update() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request).unwrap();
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\nschema: test").unwrap();
        });
        let pkg = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let file = pkg.path().join("recipe.yaml");
        std::fs::write(&file,format!("download_files:\n  - nested/a.yaml::http://{address}/remote\ninstall_files: nested/a.yaml\n")).unwrap();
        assert_eq!(
            install_recipe(&file, pkg.path(), out.path(), &[], false).unwrap(),
            1
        );
        server.join().unwrap();
        assert_eq!(
            std::fs::read_to_string(out.path().join("nested/a.yaml")).unwrap(),
            "schema: test"
        );
        assert_eq!(
            install_recipe(&file, pkg.path(), out.path(), &[], true).unwrap(),
            0
        );
    }
    #[test]
    fn patch_options_and_idempotence() {
        let pkg = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::write(pkg.path().join("a.yaml"), "schema: original\n").unwrap();
        let file = pkg.path().join("recipe.yaml");
        std::fs::write(&file, "install_files: a.yaml\npatch_files:\n  a.yaml:\n    schema/name: ${name}\n  default.custom.yaml:\n    menu/page_size: 9\n").unwrap();
        let options = vec![("name".into(), "中文名字".into())];
        assert_eq!(
            install_recipe(&file, pkg.path(), out.path(), &options, false).unwrap(),
            2
        );
        let value: Value =
            from_str(&std::fs::read_to_string(out.path().join("a.yaml")).unwrap()).unwrap();
        assert_eq!(
            value["__patch"]["schema/name"],
            Value::String("中文名字".into())
        );
        assert_eq!(
            install_recipe(&file, pkg.path(), out.path(), &options, false).unwrap(),
            0
        );
    }
    #[test]
    fn rejects_unsupported_before_install() {
        let pkg = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let file = pkg.path().join("recipe.yaml");
        for body in [
            "patch: {}",
            "unknown: true",
            "patch_files:\n  ../escape.yaml: {}",
            "patch_files:\n  a.yaml:\n    key: $(touch bad)",
        ] {
            std::fs::write(&file, body).unwrap();
            assert!(install_recipe(&file, pkg.path(), out.path(), &[], false).is_err());
            assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
        }
    }
    #[test]
    fn recursive_defaults_and_missing_pattern() {
        let pkg = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        for name in [
            "a.yaml",
            "lua/a.lua",
            "dict/a.dict.yaml",
            "opencc/a.ocd2",
            "skip.custom.yaml",
        ] {
            let p = pkg.path().join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "test").unwrap();
        }
        assert_eq!(install_default_files(pkg.path(), out.path()).unwrap(), 4);
        assert!(collect_files(&["absent.yaml".into()], pkg.path()).is_err());
    }
    #[test]
    fn expansions() {
        let vars = HashMap::from([("name".into(), "Alice".into())]);
        let mut used = HashSet::new();
        assert_eq!(
            expand("$name ${other:-default}", &vars, &mut used).unwrap(),
            "Alice default"
        );
        assert!(expand("${missing}", &vars, &mut used).is_err());
        assert_eq!(
            expand("derive/x$/y/", &vars, &mut used).unwrap(),
            "derive/x$/y/"
        );
        assert_eq!(
            expand_value(Value::String("${size:-9}".into()), &vars, &mut used).unwrap(),
            Value::Number(9.into())
        );
    }
}
