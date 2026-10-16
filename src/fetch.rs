use crate::target::PackageRef;
use anyhow::{Context, Result, bail};
use git2::{Repository, ResetType};
use std::path::Path;

pub fn fetch_or_update(pkg: &PackageRef, path: &Path, no_update: bool) -> Result<()> {
    if !path.exists() {
        return clone(pkg, path);
    }
    if !path.join(".git").exists() {
        println!("非 Git 目录，跳过更新: {}", path.display());
        return Ok(());
    }
    let repo = Repository::open(path)?;
    let origin = repo.find_remote("origin")?;
    if origin.url()?.trim_end_matches(".git") != pkg.clone_url().trim_end_matches(".git") {
        bail!("缓存 origin 与目标不同: {}", path.display());
    }
    drop(origin);
    update_repository(&repo, pkg.branch.as_deref(), no_update)
}

fn clone(pkg: &PackageRef, path: &Path) -> Result<()> {
    println!("下载: {}", pkg.clone_url());
    let parent = path.parent().context("缓存目录没有父目录")?;
    std::fs::create_dir_all(parent)?;
    let temp = tempfile::tempdir_in(parent)?;
    let checkout = temp.path().join("checkout");
    let mut options = git2::FetchOptions::new();
    options.depth(1);
    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(options);
    if let Some(branch) = &pkg.branch {
        builder.branch(branch);
    }
    let repo = builder.clone(&pkg.clone_url(), &checkout)?;
    if let Ok(head) = repo.head()
        && let Ok(name) = head.name()
        && name.starts_with("refs/heads/")
        && pkg.branch.is_none()
    {
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            &name.replacen("refs/heads/", "refs/remotes/origin/", 1),
            true,
            "remember default branch",
        )?;
    }
    update_submodules(&repo)?;
    drop(repo);
    std::fs::rename(checkout, path)?;
    Ok(())
}

fn update_submodules(repo: &Repository) -> Result<()> {
    for mut submodule in repo.submodules()? {
        let mut fetch = git2::FetchOptions::new();
        if submodule
            .url()?
            .is_some_and(|url| url.starts_with("https://") || url.starts_with("http://"))
        {
            fetch.depth(1);
        }
        let mut options = git2::SubmoduleUpdateOptions::new();
        options.fetch(fetch);
        submodule
            .update(true, Some(&mut options))
            .with_context(|| format!("更新子模块: {:?}", submodule.path()))?;
        update_submodules(&submodule.open()?)?;
    }
    Ok(())
}

fn update_repository(repo: &Repository, requested: Option<&str>, no_update: bool) -> Result<()> {
    let mut remote = repo.find_remote("origin")?;
    let default = repo
        .find_reference("refs/remotes/origin/HEAD")
        .ok()
        .and_then(|r| r.symbolic_target().ok().flatten().map(str::to_string));
    let branch = if let Some(branch) = requested {
        branch.to_string()
    } else if let Some(default) = default {
        default
            .trim_start_matches("refs/remotes/origin/")
            .to_string()
    } else {
        remote.connect(git2::Direction::Fetch)?;
        let name = remote
            .default_branch()?
            .as_str()
            .context("远端没有默认分支")?
            .trim_start_matches("refs/heads/")
            .to_string();
        remote.disconnect()?;
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            &format!("refs/remotes/origin/{name}"),
            true,
            "default branch",
        )?;
        name
    };
    let local_ref = format!("refs/heads/{branch}");
    let current = repo
        .head()
        .ok()
        .and_then(|r| r.name().ok().map(str::to_string));
    let switching = current.as_deref() != Some(&local_ref);
    if !switching && no_update {
        return Ok(());
    }
    let remote_ref = format!("refs/remotes/origin/{branch}");
    let tag_ref = format!("refs/tags/{branch}");
    if requested.is_some()
        && repo.find_reference(&tag_ref).is_ok()
        && repo.find_reference(&local_ref).is_err()
        && repo.find_reference(&remote_ref).is_err()
    {
        let object = repo
            .find_reference(&tag_ref)?
            .peel(git2::ObjectType::Commit)?;
        repo.checkout_tree(&object, None)?;
        repo.set_head_detached(object.id())?;
        if !no_update {
            update_submodules(repo)?;
        }
        return Ok(());
    }
    let must_fetch = !no_update || repo.find_reference(&remote_ref).is_err();
    if must_fetch {
        let mut options = git2::FetchOptions::new();
        // Local remotes used by offline regression tests do not support shallow fetch.
        if remote.url()?.starts_with("https://") || remote.url()?.starts_with("http://") {
            options.depth(1);
        }
        remote.fetch(
            &["+refs/heads/*:refs/remotes/origin/*"],
            Some(&mut options),
            None,
        )?;
    }
    let mut status = git2::StatusOptions::new();
    status.include_untracked(false);
    if !repo.statuses(Some(&mut status))?.is_empty() {
        bail!("缓存含本地修改，拒绝覆盖");
    }
    let remote_commit = repo.find_reference(&remote_ref)?.peel_to_commit()?;
    if switching {
        if repo.find_reference(&local_ref).is_err() {
            repo.branch(&branch, &remote_commit, false)?;
        }
        // Checkout first so a failure does not move HEAD to a branch with the wrong worktree.
        let object = repo.revparse_single(&local_ref)?;
        repo.checkout_tree(&object, None)?;
        repo.set_head(&local_ref)?;
        repo.find_branch(&branch, git2::BranchType::Local)?
            .set_upstream(Some(&format!("origin/{branch}")))?;
    } else {
        repo.reset(remote_commit.as_object(), ResetType::Hard, None)?;
    }
    if !no_update {
        update_submodules(repo)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn commit(repo: &Repository, name: &str, body: &str) -> git2::Oid {
        std::fs::write(repo.workdir().unwrap().join(name), body).unwrap();
        let mut index = repo.index().unwrap();
        index.read(true).unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = git2::Signature::now("test", "test@example.com").unwrap();
        let parent = repo.head().ok().and_then(|r| r.peel_to_commit().ok());
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "fixture",
            &tree,
            &parent.iter().collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn switches_branches_without_moving_other_branches_and_no_update_works() {
        let temp = tempfile::tempdir().unwrap();
        let source = Repository::init(temp.path().join("source")).unwrap();
        let main = commit(&source, "a.yaml", "main");
        if source.find_reference("refs/heads/main").is_err() {
            source
                .branch("main", &source.find_commit(main).unwrap(), false)
                .unwrap();
        }
        source.set_head("refs/heads/main").unwrap();
        source
            .branch("other", &source.find_commit(main).unwrap(), false)
            .unwrap();
        source.set_head("refs/heads/other").unwrap();
        let other = commit(&source, "a.yaml", "other");
        source.set_head("refs/heads/main").unwrap();
        source
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        let repo = Repository::clone(
            source.workdir().unwrap().to_str().unwrap(),
            temp.path().join("cache"),
        )
        .unwrap();
        repo.reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            true,
            "test",
        )
        .unwrap();
        update_repository(&repo, Some("other"), true).unwrap();
        assert_eq!(repo.head().unwrap().name().unwrap(), "refs/heads/other");
        assert_eq!(repo.head().unwrap().target(), Some(other));
        assert_eq!(
            std::fs::read_to_string(repo.workdir().unwrap().join("a.yaml")).unwrap(),
            "other"
        );
        assert_eq!(
            repo.find_reference("refs/heads/main").unwrap().target(),
            Some(main)
        );
        update_repository(&repo, None, true).unwrap();
        assert_eq!(repo.head().unwrap().name().unwrap(), "refs/heads/main");
    }
    #[test]
    fn https_transport_is_built() {
        assert!(git2::Version::get().https());
    }
    #[test]
    fn initializes_nested_submodule_contents() {
        let temp = tempfile::tempdir().unwrap();
        let leaf = Repository::init(temp.path().join("leaf")).unwrap();
        commit(&leaf, "dictionary.txt", "child contents");
        let source = Repository::init(temp.path().join("source")).unwrap();
        commit(&source, "a.yaml", "parent");
        let result = std::process::Command::new("git")
            .arg("-C")
            .arg(source.workdir().unwrap())
            .args(["-c", "protocol.file.allow=always", "submodule", "add"])
            .arg(leaf.workdir().unwrap())
            .arg("dicts/child")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        commit(&source, "a.yaml", "with submodule");
        assert_eq!(source.submodules().unwrap().len(), 1);
        let clone = Repository::clone(
            source.workdir().unwrap().to_str().unwrap(),
            temp.path().join("clone"),
        )
        .unwrap();
        update_submodules(&clone).unwrap();
        assert_eq!(
            std::fs::read_to_string(clone.workdir().unwrap().join("dicts/child/dictionary.txt"))
                .unwrap(),
            "child contents"
        );
    }
    #[test]
    #[ignore = "requires internet"]
    fn online_clone_and_update() {
        let temp = tempfile::tempdir().unwrap();
        let crate::target::Target::Package(pkg) =
            crate::target::parse_target("luna-pinyin").unwrap()
        else {
            panic!()
        };
        let path = temp.path().join("package");
        fetch_or_update(&pkg, &path, false).unwrap();
        assert!(path.join("luna_pinyin.schema.yaml").is_file());
        fetch_or_update(&pkg, &path, false).unwrap();
    }
}
