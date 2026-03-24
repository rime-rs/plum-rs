use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

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
    pub patch: Option<serde_yaml::Value>,
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
    let recipe: Recipe = serde_yaml::from_str(&content)
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
        if name.ends_with(".yaml")
            && !name.ends_with(".custom.yaml")
            && !name.ends_with(".recipe.yaml")
            && name != "recipe.yaml"
        {
            files.push(name);
        } else if name.ends_with(".txt") || name.ends_with(".gram") {
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
