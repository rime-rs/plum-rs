mod archive;
mod conf;
mod fetch;
mod files;
mod frontend;
mod recipe;
mod target;

use anyhow::{Context, Result, bail};
use clap::Parser;
use std::{
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};
use target::{Target, parse_target};

/// Rime 配置管理器
#[derive(Parser, Debug)]
#[command(name = "rime-install", version, about = "Rime 配置管理器")]
struct Cli {
    /// Rime 用户目录；默认读取 Weasel 注册表或探测前端
    #[arg(long, env = "RIME_DIR")]
    rime_dir: Option<PathBuf>,
    /// 缓存目录；默认使用系统用户缓存目录下的 plum-rs
    #[arg(long, env = "PLUM_DIR")]
    plum_dir: Option<PathBuf>,
    /// 跳过已有包与外部文件的更新
    #[arg(long)]
    no_update: bool,
    /// 无 target 时读取交互输入；重定向输入也可使用
    #[arg(long)]
    interactive: bool,
    /// 安装 target：luna-pinyin、user/repo@branch:recipe:key=value、:preset、ZIP 或清单
    targets: Vec<String>,
}

fn main() -> Result<()> {
    let mut cli = Cli::parse();
    if cli.rime_dir.is_none() {
        cli.rime_dir = std::env::var_os("rime_dir").map(PathBuf::from);
    }
    if cli.plum_dir.is_none() {
        cli.plum_dir = std::env::var_os("plum_dir").map(PathBuf::from);
    }
    cli.no_update |= std::env::var_os("no_update").is_some_and(|s| !s.is_empty());
    let interactive = cli.interactive || (cli.targets.is_empty() && io::stdin().is_terminal());
    if !interactive && cli.targets.is_empty() {
        cli.targets.push(":preset".into());
    }
    let output = std::path::absolute(
        cli.rime_dir
            .clone()
            .or_else(frontend::guess_rime_dir)
            .context("无法自动探测 Rime 用户目录，请用 --rime-dir 指定")?,
    )?;
    let plum = std::path::absolute(
        cli.plum_dir
            .clone()
            .or_else(frontend::default_plum_dir)
            .context("无法探测缓存目录，请用 --plum-dir 指定")?,
    )?;
    println!(
        "Rime 用户目录: {}\n缓存目录: {}",
        output.display(),
        plum.display()
    );
    if !cli.targets.is_empty() {
        install_targets(&cli.targets, &plum, &output, cli.no_update)?;
    }
    if interactive {
        loop {
            print!("输入安装目标（空行安装 :preset，q 退出）: ");
            io::stdout().flush()?;
            let mut line = String::new();
            if io::stdin().read_line(&mut line)? == 0 {
                break;
            }
            let line = line.trim();
            if matches!(line, "q" | "quit" | "exit") {
                break;
            }
            // A whole local path may contain spaces.
            let targets = if line.is_empty() {
                vec![":preset".into()]
            } else if Path::new(line).is_file() {
                vec![line.into()]
            } else {
                line.split_whitespace().map(str::to_string).collect()
            };
            if let Err(e) = install_targets(&targets, &plum, &output, cli.no_update) {
                eprintln!("安装失败: {e:#}");
            }
        }
    }
    Ok(())
}

fn install_targets(raw: &[String], plum: &Path, output: &Path, no_update: bool) -> Result<()> {
    let mut total = 0;
    for raw in raw {
        if raw == "plum" {
            bail!("二进制版本不支持自更新，请下载新版 rime-install");
        }
        let target = parse_target(raw).with_context(|| format!("无法解析 target: {raw}"))?;
        if let Target::Archive(source) = target {
            total += archive::install(&source, plum, output, no_update)?;
            continue;
        }
        for pkg in resolve_target(target, plum)? {
            let package =
                files::safe_join(&plum.join("package"), &format!("{}/{}", pkg.user, pkg.repo))?;
            if !no_update || !package.is_dir() {
                fetch::fetch_or_update(&pkg, &package)?;
            }
            let recipe = if let Some(name) = &pkg.recipe {
                let file = files::safe_join(&package, &format!("{name}.recipe.yaml"))?;
                if !file.is_file() {
                    bail!("recipe 不存在: {}", file.display());
                }
                Some(file)
            } else {
                let file = package.join("recipe.yaml");
                file.is_file().then_some(file)
            };
            total += if let Some(file) = recipe {
                recipe::install_recipe(&file, &package, output, &pkg.options, no_update)?
            } else {
                if !pkg.options.is_empty() {
                    bail!("包没有 recipe，无法消费选项");
                }
                recipe::install_default_files(&package, output)?
            };
        }
    }
    println!("完成：更新 {total} 个文件，输出到 '{}'", output.display());
    Ok(())
}

fn resolve_target(target: Target, plum: &Path) -> Result<Vec<target::PackageRef>> {
    let location = match target {
        Target::Package(pkg) => return Ok(vec![pkg]),
        Target::BuiltinConfig(name) => format!("builtin:{name}-packages.conf"),
        Target::PackageList(list) => list.url,
        Target::LocalConfig(path) => path,
        Target::Archive(_) => bail!("ZIP 不属于包清单"),
    };
    conf::load(&location, plum)?
        .into_iter()
        .map(|raw| match parse_target(&raw)? {
            Target::Package(pkg) => Ok(pkg),
            _ => bail!("conf 中包含非法 target: {raw}"),
        })
        .collect()
}
