mod conf;
mod fetch;
mod frontend;
mod recipe;
mod target;

use anyhow::{bail, Context, Result};
use clap::Parser;
use std::path::PathBuf;
use target::{parse_target, Target};

/// Rime 配置管理器
///
/// target 格式：
///   luna-pinyin                               # 短名，归属 rime 组织
///   lotem/rime-zhung                          # user/repo
///   lotem/rime-zhung@master                   # 指定分支
///   luna-pinyin:simp                          # 指定 recipe
///   luna-pinyin:simp:key=val,key2=val2        # recipe + 选项
///   lotem/rime-forge/lotem-packages.conf      # 远程包列表
///   :preset / :extra / :all                   # 内置配置集
#[derive(Parser, Debug)]
#[command(name = "rime-install", version, about)]
struct Cli {
    /// Rime 用户目录，不指定则自动探测
    #[arg(long, env = "rime_dir")]
    rime_dir: Option<PathBuf>,

    /// plum 工作目录（存放下载的包）
    #[arg(long, env = "plum_dir", default_value = "plum")]
    plum_dir: PathBuf,

    /// 跳过已有包的更新
    #[arg(long)]
    no_update: bool,

    /// 要安装的 target，不指定则默认 :preset
    targets: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let rime_dir = match cli.rime_dir {
        Some(d) => d,
        None => frontend::guess_rime_dir()
            .context("无法自动探测 Rime 用户目录，请用 --rime-dir 手动指定")?,
    };

    println!("Rime 用户目录: {}", rime_dir.display());

    let targets = if cli.targets.is_empty() {
        vec![":preset".to_string()]
    } else {
        cli.targets.clone()
    };

    let mut total_files = 0usize;
    let mut total_packages = 0usize;

    for raw in &targets {
        if raw == "plum" {
            println!("更新 plum 本身暂不支持（二进制版本请重新下载）");
            continue;
        }

        let t = parse_target(raw).with_context(|| format!("无法解析 target: {}", raw))?;

        let packages = resolve_target(t, &cli.plum_dir)?;

        for pkg_ref in &packages {
            let package_dir = cli
                .plum_dir
                .join("package")
                .join(&pkg_ref.user)
                .join(&pkg_ref.repo);

            if !cli.no_update || !package_dir.exists() {
                fetch::fetch_or_update(pkg_ref, &package_dir)?;
            } else {
                println!("已有包: {}/{}", pkg_ref.user, pkg_ref.repo);
            }

            let files = install_package(pkg_ref, &package_dir, &rime_dir)?;
            total_files += files;
            total_packages += 1;
        }
    }

    if total_files == 0 {
        println!("没有文件需要更新。");
    } else {
        println!(
            "完成：共更新 {} 个文件，来自 {} 个包，输出到 '{}'",
            total_files,
            total_packages,
            rime_dir.display()
        );
    }

    Ok(())
}

fn resolve_target(t: Target, plum_dir: &PathBuf) -> Result<Vec<target::PackageRef>> {
    match t {
        Target::Package(p) => Ok(vec![p]),

        Target::BuiltinConfig(name) => {
            let conf_path = plum_dir.join(format!("{}-packages.conf", name));
            if !conf_path.exists() {
                bail!(
                    "内置配置 '{}' 不存在，请确认 plum_dir 正确: {}",
                    name,
                    plum_dir.display()
                );
            }
            let content = std::fs::read_to_string(&conf_path)?;
            parse_package_list(&content)
        }

        Target::PackageList(pl) => {
            let content = conf::fetch_conf_url(&pl.url)?;
            parse_package_list(&content)
        }
    }
}

fn parse_package_list(content: &str) -> Result<Vec<target::PackageRef>> {
    let pkg_list = conf::load_conf_file(content)?;
    pkg_list
        .iter()
        .map(|s| {
            let Target::Package(p) = parse_target(s)? else {
                bail!("conf 文件中包含非法 target: {}", s);
            };
            Ok(p)
        })
        .collect()
}

fn install_package(
    pkg: &target::PackageRef,
    package_dir: &PathBuf,
    output_dir: &PathBuf,
) -> Result<usize> {
    let recipe_file = if let Some(r) = &pkg.recipe {
        let f = package_dir.join(format!("{}.recipe.yaml", r));
        if !f.exists() {
            bail!("recipe 不存在: {}", f.display());
        }
        Some(f)
    } else {
        let f = package_dir.join("recipe.yaml");
        if f.exists() {
            Some(f)
        } else {
            None
        }
    };

    if let Some(rf) = recipe_file {
        recipe::install_recipe(&rf, package_dir, output_dir)
    } else {
        install_default_files(package_dir, output_dir)
    }
}

fn install_default_files(package_dir: &PathBuf, output_dir: &PathBuf) -> Result<usize> {
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

    let mut count = 0;
    for file in &files {
        let src = package_dir.join(file);
        let dst = output_dir.join(file);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let changed = if dst.exists() {
            std::fs::read(&src)? != std::fs::read(&dst)?
        } else {
            true
        };
        if changed {
            if dst.exists() {
                println!("更新: {}", file);
            } else {
                println!("安装: {}", file);
            }
            std::fs::copy(&src, &dst)?;
            count += 1;
        }
    }
    Ok(count)
}
