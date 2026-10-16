mod archive;
mod build_data;
mod conf;
mod download;
mod fetch;
mod files;
mod frontend;
mod legacy;
mod patch;
mod recipe;
mod selector;
mod self_update;
mod target;

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};
use target::{Target, parse_target};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum BuildSet {
    Preset,
    Extra,
    All,
    Minimal,
}

#[derive(Parser, Debug)]
#[command(name = "rime-install", version, about = "Rime 配置管理器")]
struct Cli {
    #[arg(long, env = "RIME_DIR")]
    rime_dir: Option<PathBuf>,
    #[arg(long, env = "PLUM_DIR")]
    plum_dir: Option<PathBuf>,
    #[arg(long, env = "rime_frontend")]
    rime_frontend: Option<String>,
    #[arg(long)]
    no_update: bool,
    /// 按原版菜单选择要安装的包
    #[arg(long, conflicts_with = "interactive")]
    select: bool,
    /// 循环读取安装目标（无参数默认直接安装 :preset）
    #[arg(long)]
    interactive: bool,
    /// 扩展默认规则，递归安装 Lua、嵌套词库等；配方中的 install_files 仍优先
    #[arg(long, conflicts_with = "shell_compat")]
    recursive: bool,
    /// 使用内置上游 Bash 脚本，执行复杂清单与配方；需要 Bash/Git/curl
    #[arg(long)]
    shell_compat: bool,
    /// 生成 preset/extra/all/minimal 数据到 --rime-dir（默认缓存目录/output）
    #[arg(long,value_enum,conflicts_with_all=["select","interactive","clean_output"])]
    build: Option<BuildSet>,
    /// 使用 rime_deployer 编译数据；可与 --build 配合
    #[arg(long)]
    compile: bool,
    /// 将已有输出安装为共享数据
    #[arg(long)]
    install_shared: Option<PathBuf>,
    /// 打包源码和下载缓存为 tar.gz；默认先生成 all 数据
    #[arg(long)]
    dist: Option<PathBuf>,
    #[arg(long)]
    source_dir: Option<PathBuf>,
    /// 只清理本工具构建清单记录的文件，保留其他文件
    #[arg(long)]
    clean_output: bool,
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
    if cli.targets == ["plum"] {
        return self_update::update();
    }
    let plum = std::path::absolute(
        cli.plum_dir
            .clone()
            .or_else(frontend::default_plum_dir)
            .context("请用 --plum-dir 指定缓存")?,
    )?;
    let data_mode = cli.build.is_some()
        || cli.compile
        || cli.install_shared.is_some()
        || cli.dist.is_some()
        || cli.clean_output;
    let output = std::path::absolute(match cli.rime_dir.clone() {
        Some(path) => path,
        None if data_mode => plum.join("output"),
        None => frontend::guess_rime_dir(cli.rime_frontend.as_deref())?
            .context("无法探测输出目录，请用 --rime-dir 指定")?,
    })?;
    println!(
        "Rime 目录: {}\n缓存目录: {}",
        output.display(),
        plum.display()
    );
    if cli.clean_output {
        return build_data::clean(&output);
    }
    if cli.dist.is_some() && cli.build.is_none() {
        cli.build = Some(BuildSet::All);
    }
    if let Some(set) = cli.build {
        if !cli.targets.is_empty() {
            bail!("--build 请勿同时指定安装目标");
        }
        std::fs::create_dir_all(&plum)?;
        let stage = tempfile::tempdir_in(&plum)?;
        let targets = match set {
            BuildSet::Preset => vec![":preset".into()],
            BuildSet::Extra => vec![":extra".into()],
            BuildSet::All => vec![":all".into()],
            BuildSet::Minimal => vec!["essay".into(), "luna-pinyin".into(), "prelude".into()],
        };
        install_targets(&targets, &plum, stage.path(), &cli)?;
        if matches!(set, BuildSet::Minimal) {
            build_data::minimal(stage.path())?;
        }
        if cli.compile {
            build_data::compile(stage.path())?;
        }
        build_data::publish(stage.path(), &output)?;
    } else if cli.compile {
        build_data::compile(&output)?;
    } else if cli.install_shared.is_none() && cli.dist.is_none() {
        let mut targets = cli.targets.clone();
        if cli.select && !cli.shell_compat {
            if targets.is_empty() {
                targets.push(":preset".into());
            }
            let mut candidates = Vec::new();
            for raw in &targets {
                for pkg in resolve_target(parse_target(raw)?, &plum)? {
                    let mut name = pkg.identity.clone();
                    if let Some(branch) = pkg.branch {
                        name.push('@');
                        name.push_str(&branch);
                    }
                    if let Some(recipe) = pkg.recipe {
                        name.push(':');
                        name.push_str(&recipe);
                    }
                    if !pkg.options.is_empty() {
                        if !name.contains(':') {
                            name.push(':');
                        }
                        name.push(':');
                        name.push_str(
                            &pkg.options
                                .into_iter()
                                .map(|(k, v)| format!("{k}={v}"))
                                .collect::<Vec<_>>()
                                .join(","),
                        );
                    }
                    if !candidates.contains(&name) {
                        candidates.push(name);
                    }
                }
            }
            targets = selector::select(
                &candidates,
                &targets,
                &mut io::stdin().lock(),
                &mut io::stdout().lock(),
            )?;
            if targets.is_empty() {
                return Ok(());
            }
        }
        if !cli.interactive || !targets.is_empty() {
            if targets.is_empty() {
                targets.push(":preset".into());
            }
            install_targets(&targets, &plum, &output, &cli)?;
        }
        if cli.interactive {
            loop {
                print!("输入目标（空行 :preset，q 退出）: ");
                io::stdout().flush()?;
                let mut line = String::new();
                if io::stdin().read_line(&mut line)? == 0 {
                    break;
                }
                let line = line.trim();
                if matches!(line, "q" | "quit" | "exit") {
                    break;
                }
                let targets = if line.is_empty() {
                    vec![":preset".into()]
                } else if Path::new(line).is_file() {
                    vec![line.into()]
                } else {
                    line.split_whitespace().map(str::to_string).collect()
                };
                if let Err(error) = install_targets(&targets, &plum, &output, &cli) {
                    eprintln!("安装失败: {error:#}");
                }
            }
        }
    }
    if let Some(destination) = &cli.install_shared {
        build_data::install_shared(&output, &std::path::absolute(destination)?)?;
    }
    if let Some(destination) = &cli.dist {
        build_data::dist(
            &std::path::absolute(cli.source_dir.clone().unwrap_or(std::env::current_dir()?))?,
            &plum,
            &std::path::absolute(destination)?,
        )?;
    }
    Ok(())
}

fn install_targets(targets: &[String], plum: &Path, output: &Path, cli: &Cli) -> Result<()> {
    if cli.shell_compat {
        if cli.select {
            return legacy::run(targets, plum, output, cli.no_update, true);
        }
        for raw in targets {
            if raw == "plum" {
                self_update::update()?;
            } else if let Ok(Target::Archive(source)) = parse_target(raw) {
                archive::install(&source, plum, output, cli.no_update, false)?;
            } else {
                legacy::run(
                    std::slice::from_ref(raw),
                    plum,
                    output,
                    cli.no_update,
                    false,
                )?;
            }
        }
        return Ok(());
    }
    let mut total = 0;
    for raw in targets {
        if raw == "plum" {
            self_update::update()?;
            continue;
        }
        let target = parse_target(raw)?;
        if let Target::Archive(source) = target {
            total += archive::install(&source, plum, output, cli.no_update, cli.recursive)?;
            continue;
        }
        for pkg in resolve_target(target, plum)? {
            let package =
                files::safe_join(&plum.join("package"), &format!("{}/{}", pkg.user, pkg.repo))?;
            fetch::fetch_or_update(&pkg, &package, cli.no_update)?;
            let recipe = if let Some(name) = &pkg.recipe {
                let path = files::safe_join(&package, &format!("{name}.recipe.yaml"))?;
                if !path.is_file() {
                    bail!("recipe 不存在: {}", path.display());
                }
                Some(path)
            } else {
                let path = package.join("recipe.yaml");
                path.is_file().then_some(path)
            };
            total += if let Some(file) = recipe {
                recipe::install_named_recipe(&file, &package, output, &pkg.options, &pkg.identity)?
            } else if cli.recursive {
                recipe::install_recursive_files(&package, output)?
            } else {
                recipe::install_default_files(&package, output)?
            };
        }
    }
    println!("完成：更新 {total} 个文件");
    Ok(())
}

fn resolve_target(target: Target, plum: &Path) -> Result<Vec<target::PackageRef>> {
    let location = match target {
        Target::Package(pkg) => return Ok(vec![pkg]),
        Target::BuiltinConfig(name) => format!("builtin:{name}-packages.conf"),
        Target::PackageList(list) => list.url,
        Target::LocalConfig(path) => path,
        Target::Archive(_) => bail!("ZIP 无法展开为包列表"),
    };
    conf::load(&location, plum)?
        .into_iter()
        .map(|raw| match parse_target(&raw)? {
            Target::Package(pkg) => Ok(pkg),
            _ => bail!("清单目标非法: {raw}"),
        })
        .collect()
}
