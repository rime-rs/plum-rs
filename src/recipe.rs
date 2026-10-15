use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;
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
            StringOrVec::String(s) => s.split_whitespace().map(|x| x.to_string()).collect(),
            StringOrVec::Vec(v) => v,
        }
    }
}

/// recipe.yaml 的结构
/// 只实现 install_files 段，download_files 和 patch 后续扩展
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct Recipe {
    pub recipe: Option<RecipeInfo>,
    pub install_files: Option<StringOrVec>,
    pub download_files: Option<StringOrVec>,
    pub patch: Option<Value>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct RecipeInfo {
    #[serde(rename = "Rx")]
    pub rx: Option<String>,
}

pub fn load_recipe(recipe_file: &Path) -> Result<Recipe> {
    let content = std::fs::read_to_string(recipe_file)
        .with_context(|| format!("读取 recipe 失败: {}", recipe_file.display()))?;
    let recipe: Recipe = from_str(&content)
        .with_context(|| format!("解析 recipe.yaml 失败: {}", recipe_file.display()))?;
    Ok(recipe)
}

pub fn install_recipe(recipe_file: &Path, package_dir: &Path, output_dir: &Path) -> Result<usize> {
    let recipe = load_recipe(recipe_file)?;
    let mut count = 0;

    if let Some(files) = recipe.install_files {
        count += install_files(&files.into_vec(), package_dir, output_dir)?;
    } else {
        // 没有 recipe 指定时，装所有符合条件的文件（同 install_files_from_package）
        let files = collect_default_files(package_dir)?;
        count += install_files(&files, package_dir, output_dir)?;
    }

    Ok(count)
}

/// 没有 recipe 时的默认文件收集规则，对应原 install_files_from_package
fn collect_default_files(package_dir: &Path) -> Result<Vec<String>> {
    let mut files = vec![];

    for entry in std::fs::read_dir(package_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_config = name.ends_with(".yaml")
            && !name.ends_with(".custom.yaml")
            && !name.ends_with(".recipe.yaml")
            && name != "recipe.yaml";
        if is_config || name.ends_with(".txt") || name.ends_with(".gram") {
            files.push(name);
        }
    }

    // opencc 子目录
    let opencc_dir = package_dir.join("opencc");
    if opencc_dir.is_dir() {
        for entry in std::fs::read_dir(&opencc_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".json") || name.ends_with(".ocd") || name.ends_with(".txt") {
                files.push(format!("opencc/{}", name));
            }
        }
    }

    Ok(files)
}

fn install_files(files: &[String], package_dir: &Path, output_dir: &Path) -> Result<usize> {
    let mut count = 0;

    let pkg_dir_str = package_dir.to_string_lossy().replace("\\", "/");
    let escaped_dir = glob::Pattern::escape(&pkg_dir_str);

    for pattern in files {
        let glob_pattern = format!("{}/{}", escaped_dir, pattern.replace("\\", "/"));
        let mut matched_any = false;

        if let Ok(paths) = glob::glob(&glob_pattern) {
            for entry in paths.filter_map(Result::ok) {
                if !entry.is_file() {
                    continue;
                }
                matched_any = true;

                let rel_path = match entry.strip_prefix(package_dir) {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                let dst = output_dir.join(rel_path);

                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("创建目录失败: {}", parent.display()))?;
                }

                if dst.exists() {
                    if files_identical(&entry, &dst)? {
                        continue;
                    }
                    println!("更新: {}", rel_path.display());
                } else {
                    println!("安装: {}", rel_path.display());
                }

                std::fs::copy(&entry, &dst).with_context(|| {
                    format!("复制文件失败: {} -> {}", entry.display(), dst.display())
                })?;
                count += 1;
            }
        }

        if !matched_any {
            eprintln!("警告: 匹配不到文件，跳过: {}", pattern);
        }
    }

    Ok(count)
}

fn files_identical(a: &Path, b: &Path) -> Result<bool> {
    let a = std::fs::read(a)?;
    let b = std::fs::read(b)?;
    Ok(a == b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// 每个测试使用独立临时目录，避免并行执行时互相干扰
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            // 建在 target/ 下而非系统临时目录：受限环境下 %TEMP% 未必可写，
            // 而 target/ 已被 .gitignore 忽略，且一定随构建目录一起可写。
            let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("test-tmp")
                .join(format!("recipe-{}-{}-{}", std::process::id(), tag, n));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_file(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn test_into_vec_splits_whitespace() {
        let files = StringOrVec::String("a.yaml b.txt\nc.gram".into()).into_vec();
        assert_eq!(files, vec!["a.yaml", "b.txt", "c.gram"]);
    }

    #[test]
    fn test_into_vec_keeps_list() {
        let files = StringOrVec::Vec(vec!["a.yaml".into(), "b.txt".into()]).into_vec();
        assert_eq!(files, vec!["a.yaml", "b.txt"]);
    }

    #[test]
    fn test_load_recipe_accepts_string_install_files() {
        let dir = TempDir::new("string");
        let path = write_file(dir.path(), "recipe.yaml", "install_files: a.yaml b.txt\n");

        let recipe = load_recipe(&path).unwrap();
        assert_eq!(
            recipe.install_files.unwrap().into_vec(),
            vec!["a.yaml", "b.txt"]
        );
        assert!(recipe.download_files.is_none());
        assert!(recipe.patch.is_none());
    }

    #[test]
    fn test_load_recipe_accepts_list_patch_and_rx() {
        let dir = TempDir::new("list");
        let path = write_file(
            dir.path(),
            "recipe.yaml",
            "\
recipe:
  Rx: luna-pinyin
install_files:
  - a.yaml
  - opencc/*.json
download_files: extra/*.yaml
patch:
  speller/alphabet: zyxwvutsrqponmlkjihgfedcba
",
        );

        let recipe = load_recipe(&path).unwrap();
        assert_eq!(recipe.recipe.unwrap().rx.as_deref(), Some("luna-pinyin"));
        assert_eq!(
            recipe.install_files.unwrap().into_vec(),
            vec!["a.yaml", "opencc/*.json"]
        );
        assert_eq!(
            recipe.download_files.unwrap().into_vec(),
            vec!["extra/*.yaml"]
        );

        // patch 段以通用 Value 解析，确认新 YAML 后端与 serde_yaml 的 Value 行为一致
        let patch = recipe.patch.expect("patch 段应被解析");
        let Value::Mapping(mapping) = &patch else {
            panic!("patch 应为 mapping，实际为 {patch:?}");
        };
        assert_eq!(mapping.len(), 1);
    }

    #[test]
    fn test_load_recipe_reports_parse_error() {
        let dir = TempDir::new("bad");
        let path = write_file(dir.path(), "recipe.yaml", "install_files: [unclosed\n");

        let err = load_recipe(&path).unwrap_err();
        let message = format!("{err:#}");
        assert!(
            message.contains("解析 recipe.yaml 失败"),
            "错误信息缺少上下文: {message}"
        );
    }

    #[test]
    fn test_install_recipe_copies_only_matched_files() {
        let pkg = TempDir::new("pkg");
        let out = TempDir::new("out");

        write_file(pkg.path(), "a.yaml", "alpha");
        write_file(pkg.path(), "b.txt", "beta");
        write_file(pkg.path(), "opencc/t2s.json", "{}");
        let recipe_file = write_file(
            pkg.path(),
            "recipe.yaml",
            "install_files:\n  - a.yaml\n  - opencc/*.json\n",
        );

        let count = install_recipe(&recipe_file, pkg.path(), out.path()).unwrap();
        assert_eq!(count, 2);
        assert_eq!(
            fs::read_to_string(out.path().join("a.yaml")).unwrap(),
            "alpha"
        );
        assert_eq!(
            fs::read_to_string(out.path().join("opencc/t2s.json")).unwrap(),
            "{}"
        );
        assert!(!out.path().join("b.txt").exists());
        assert!(!out.path().join("recipe.yaml").exists());

        // 内容未变化时重复安装不应重复计数
        let count = install_recipe(&recipe_file, pkg.path(), out.path()).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_install_recipe_without_install_files_falls_back_to_defaults() {
        let pkg = TempDir::new("defaults");
        let out = TempDir::new("defaults-out");

        write_file(pkg.path(), "luna.yaml", "x");
        write_file(pkg.path(), "luna.custom.yaml", "x");
        write_file(pkg.path(), "other.recipe.yaml", "x");
        write_file(pkg.path(), "notes.txt", "x");
        let recipe_file = write_file(pkg.path(), "recipe.yaml", "recipe:\n  Rx: luna\n");

        let count = install_recipe(&recipe_file, pkg.path(), out.path()).unwrap();
        assert_eq!(count, 2);
        assert!(out.path().join("luna.yaml").exists());
        assert!(out.path().join("notes.txt").exists());
        assert!(!out.path().join("luna.custom.yaml").exists());
        assert!(!out.path().join("other.recipe.yaml").exists());
        assert!(!out.path().join("recipe.yaml").exists());
    }
}
