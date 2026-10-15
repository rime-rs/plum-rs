use anyhow::{Context, Result, bail};
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

    let parent = package_dir.parent().context("缓存目录没有父目录")?;
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::tempdir_in(parent)?;
    let checkout = staging.path().join("checkout");
    let repo = builder
        .clone(&url, &checkout)
        .with_context(|| format!("克隆 {} 失败", url))?;
    drop(repo);
    std::fs::rename(checkout, package_dir)?;

    Ok(())
}

fn update(pkg: &PackageRef, package_dir: &Path) -> Result<()> {
    println!("正在更新: {}", package_dir.display());

    let repo = Repository::open(package_dir)
        .with_context(|| format!("打开仓库失败: {}", package_dir.display()))?;

    let mut remote = repo.find_remote("origin").context("找不到 origin remote")?;
    if remote.url()? != pkg.clone_url() {
        bail!(
            "缓存 origin 与请求不一致，请使用新的 --plum-dir: {}",
            package_dir.display()
        );
    }
    let mut status_options = git2::StatusOptions::new();
    status_options.include_untracked(false);
    if !repo.statuses(Some(&mut status_options))?.is_empty() {
        bail!("缓存含有本地修改，拒绝硬重置: {}", package_dir.display());
    }

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

#[cfg(test)]
mod tests {
    #[test]
    fn https_transport_is_built() {
        assert!(git2::Version::get().https(), "libgit2 必须启用 HTTPS");
    }

    #[test]
    #[ignore = "requires internet; writes only to temporary directories"]
    fn online_clone_and_update() {
        let dir = tempfile::tempdir().unwrap();
        let crate::target::Target::Package(pkg) =
            crate::target::parse_target("luna-pinyin").unwrap()
        else {
            panic!()
        };
        let path = dir.path().join("package");
        super::fetch_or_update(&pkg, &path).unwrap();
        assert!(path.join("luna_pinyin.schema.yaml").is_file());
        super::fetch_or_update(&pkg, &path).unwrap();
    }
}
