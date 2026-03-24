use anyhow::{Context, Result};
use git2::{Repository, ResetType};
use std::path::Path;

use crate::target::PackageRef;

/// 把包 clone 或 pull 到 package_dir
pub fn fetch_or_update(pkg: &PackageRef, package_dir: &Path) -> Result<()> {
    if package_dir.exists() {
        update(pkg, package_dir)
    } else {
        clone(pkg, package_dir)
    }
}

fn clone(pkg: &PackageRef, package_dir: &Path) -> Result<()> {
    let url = pkg.clone_url();
    println!("正在下载: {} -> {}", url, package_dir.display());

    let mut builder = git2::build::RepoBuilder::new();
    let mut fetch_opts = git2::FetchOptions::new();
    fetch_opts.depth(1);
    builder.fetch_options(fetch_opts);

    if let Some(branch) = &pkg.branch {
        builder.branch(branch);
    }

    builder
        .clone(&url, package_dir)
        .with_context(|| format!("克隆 {} 失败", url))?;

    Ok(())
}

fn update(pkg: &PackageRef, package_dir: &Path) -> Result<()> {
    println!("正在更新: {}", package_dir.display());

    let repo = Repository::open(package_dir)
        .with_context(|| format!("打开仓库失败: {}", package_dir.display()))?;

    let mut remote = repo.find_remote("origin").context("找不到 origin remote")?;

    let branch = pkg.branch.as_deref().unwrap_or("HEAD");

    let mut fetch_opts = git2::FetchOptions::new();
    fetch_opts.depth(1);

    remote
        .fetch(&[branch], Some(&mut fetch_opts), None)
        .context("fetch 失败")?;

    // fast-forward 到 FETCH_HEAD
    let fetch_head = repo
        .find_reference("FETCH_HEAD")
        .context("找不到 FETCH_HEAD")?;
    let fetch_commit = repo
        .reference_to_annotated_commit(&fetch_head)
        .context("无法解析 FETCH_HEAD")?;
    let target_oid = fetch_commit.id();

    let obj = repo
        .find_object(target_oid, None)
        .context("找不到目标 commit")?;
    repo.reset(&obj, ResetType::Hard, None)
        .context("reset 失败")?;

    Ok(())
}
