/// 解析 plum 的 target 字符串
///
/// 格式：<user>/<repo>@<branch>:<recipe>:key=value,...
///
/// 例子：
///   luna-pinyin
///   lotem/rime-zhung
///   lotem/rime-zhung@master
///   lotem/rime-forge/lotem-packages.conf
///   lotem/rime-zhung@master:somerecipe:key=val
///   :preset / :extra / :all
///   https://github.com/xxx/raw/master/foo-packages.conf

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Archive(String),
    LocalConfig(String),
    /// :preset / :extra / :all
    BuiltinConfig(String),
    /// 远程 packages.conf 的 URL 或短路径
    PackageList(PackageListRef),
    /// 单个包
    Package(PackageRef),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackageListRef {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackageRef {
    pub identity: String,
    /// GitHub user，默认 "rime"
    pub user: String,
    /// 原始 GitHub 项目名（如果包含 rime- 则保留）
    pub project: String,
    /// 仓库名，不含 rime- 前缀（内部统一去掉前缀）
    pub repo: String,
    /// 分支，None 表示用默认分支
    pub branch: Option<String>,
    /// recipe 名称
    pub recipe: Option<String>,
    /// recipe 选项，key=value 列表
    pub options: Vec<(String, String)>,
}

impl PackageRef {
    /// 完整的 GitHub 仓库路径，如 "rime/rime-luna-pinyin"
    pub fn github_path(&self) -> String {
        format!("{}/{}", self.user, self.project)
    }

    pub fn clone_url(&self) -> String {
        format!("https://github.com/{}.git", self.github_path())
    }
}

#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("无法识别的 target: {0}")]
    Unrecognized(String),
}

pub fn parse_target(s: &str) -> Result<Target, ParseError> {
    let invalid = || ParseError::Unrecognized(s.into());
    if s.is_empty() {
        return Err(invalid());
    }
    if s.starts_with("https://") || s.starts_with("http://") {
        let url = url::Url::parse(s).map_err(|_| invalid())?;
        if url.path().to_ascii_lowercase().ends_with(".zip") {
            return Ok(Target::Archive(s.into()));
        }
        if url.path().ends_with(".conf") {
            return Ok(Target::PackageList(PackageListRef { url: s.into() }));
        }
    }
    if s.to_ascii_lowercase().ends_with(".zip") {
        return Ok(Target::Archive(s.into()));
    }
    // 内置配置 :preset / :extra / :all
    if let Some(name) = s.strip_prefix(':') {
        if !valid_component(name) {
            return Err(invalid());
        }
        return Ok(Target::BuiltinConfig(name.to_string()));
    }

    // 完整 URL 或 packages.conf 短路径
    if s.starts_with("https://github.com/") && !s.ends_with(".conf") {
        let path = s
            .strip_prefix("https://github.com/")
            .unwrap()
            .trim_end_matches('/');
        if let Some((repo, branch)) = path.split_once("/tree/") {
            return Ok(Target::Package(parse_package_ref(&format!(
                "{repo}@{branch}"
            ))?));
        }
        let boundary = path.find(['@', ':']).unwrap_or(path.len());
        let base = path[..boundary]
            .strip_suffix(".git")
            .unwrap_or(&path[..boundary]);
        return Ok(Target::Package(parse_package_ref(&format!(
            "{base}{}",
            &path[boundary..]
        ))?));
    }
    if s.ends_with(".conf") && std::path::Path::new(s).is_file() {
        return Ok(Target::LocalConfig(s.into()));
    }
    if s.starts_with("https://") || s.starts_with("http://") || s.ends_with("-packages.conf") {
        if (s.starts_with("https://") || s.starts_with("http://"))
            && !url::Url::parse(s)
                .map(|u| u.path().ends_with(".conf"))
                .unwrap_or(false)
        {
            return Err(invalid());
        }
        let url = expand_conf_url(s);
        return Ok(Target::PackageList(PackageListRef { url }));
    }

    // 单个包
    Ok(Target::Package(parse_package_ref(s)?))
}

fn expand_conf_url(s: &str) -> String {
    if s.starts_with("https://") || s.starts_with("http://") {
        return s.to_string();
    }
    // 格式：user/repo@branch/filepath 或 user/repo/filepath
    // 例：lotem/rime-forge/lotem-packages.conf
    //     lotem/rime-forge@master/lotem-packages.conf
    let parts: Vec<&str> = s.splitn(3, '/').collect();
    if parts.len() == 3 {
        let user = parts[0];
        let (repo, branch) = if parts[1].contains('@') {
            let mut it = parts[1].splitn(2, '@');
            (it.next().unwrap(), it.next().unwrap())
        } else {
            (parts[1], "master")
        };
        let filepath = parts[2];
        return format!(
            "https://github.com/{}/{}/raw/{}/{}",
            user, repo, branch, filepath
        );
    }
    s.to_string()
}

fn parse_package_ref(s: &str) -> Result<PackageRef, ParseError> {
    // 先分离 options 段（第三个冒号之后）
    // 格式：<pkg>[@branch][:<recipe>[:<options>]]
    let (pkg_and_branch, recipe_and_opts) = split_once_or(s, ':');
    let (recipe_str, opts_str) = split_once_or(recipe_and_opts, ':');

    let recipe = if recipe_str.is_empty() {
        None
    } else {
        Some(recipe_str.to_string())
    };

    let options = parse_options(opts_str);
    let mut seen = std::collections::HashSet::new();
    if !opts_str.is_empty()
        && (opts_str.split(',').any(|entry| !entry.contains('='))
            || options.len() != opts_str.split(',').count()
            || options.iter().any(|(key, _)| {
                !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || !seen.insert(key)
            }))
    {
        return Err(ParseError::Unrecognized(s.into()));
    }

    // 解析 <pkg>[@branch]
    let (pkg_part, branch) = match pkg_and_branch.split_once('@') {
        Some((p, b)) => (p, Some(b.to_string())),
        None => (pkg_and_branch, None),
    };

    // 解析 user/repo 或 shortname
    let (user, repo, project) = if pkg_part.contains('/') {
        let mut it = pkg_part.splitn(2, '/');
        let u = it.next().unwrap().to_string();
        let proj = it.next().unwrap().to_string();
        // 去掉 rime- 前缀作为内部 repo 名
        let r = proj.strip_prefix("rime-").unwrap_or(&proj).to_string();
        (u, r, proj)
    } else {
        // 短名：归属 rime 组织
        let proj = if pkg_part.starts_with("rime-") {
            pkg_part.to_string()
        } else {
            format!("rime-{pkg_part}")
        };
        let r = proj.strip_prefix("rime-").unwrap_or(&proj).to_string();
        ("rime".to_string(), r, proj)
    };

    if !valid_component(&user)
        || !valid_component(&repo)
        || !valid_component(&project)
        || recipe.as_deref().is_some_and(|r| !valid_component(r))
        || branch.as_deref().is_some_and(|b| {
            b.is_empty()
                || b.starts_with('-')
                || b.contains(['\\', ':', ' ', '\n'])
                || b.split('/').any(|p| p == "..")
        })
    {
        return Err(ParseError::Unrecognized(s.into()));
    }
    Ok(PackageRef {
        identity: pkg_part.into(),
        user,
        repo,
        project,
        branch,
        recipe,
        options,
    })
}

fn valid_component(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && !s.ends_with('.')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn split_once_or(s: &str, delim: char) -> (&str, &str) {
    s.split_once(delim).unwrap_or((s, ""))
}

fn parse_options(s: &str) -> Vec<(String, String)> {
    if s.is_empty() {
        return vec![];
    }
    s.split(',')
        .filter_map(|kv| {
            let mut it = kv.splitn(2, '=');
            let k = it.next()?.trim().to_string();
            let v = it.next().unwrap_or("").trim().to_string();
            if k.is_empty() { None } else { Some((k, v)) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_invalid_targets() {
        for raw in [
            "https://github.com/rime/rime-luna-pinyin",
            "https://github.com/rime/rime-luna-pinyin.git",
            "https://github.com/rime/rime-luna-pinyin/",
        ] {
            let Target::Package(pkg) = parse_target(raw).unwrap() else {
                panic!()
            };
            assert_eq!(
                pkg.clone_url(),
                "https://github.com/rime/rime-luna-pinyin.git"
            );
        }
        assert!(matches!(
            parse_target("https://example.com/package.zip?download=1").unwrap(),
            Target::Archive(_)
        ));
        assert!(matches!(
            parse_target("https://example.com/a.conf?raw=1").unwrap(),
            Target::PackageList(_)
        ));
        for raw in [
            "",
            "../bad",
            "user/../../bad",
            ":../bad",
            "luna-pinyin:../bad",
            "luna-pinyin::=oops",
            "luna-pinyin::size=1,size=2",
            "luna-pinyin::flag",
        ] {
            assert!(parse_target(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn test_builtin() {
        assert_eq!(
            parse_target(":preset").unwrap(),
            Target::BuiltinConfig("preset".into())
        );
    }

    #[test]
    fn test_short_name() {
        let t = parse_target("luna-pinyin").unwrap();
        let Target::Package(p) = t else { panic!() };
        assert_eq!(p.user, "rime");
        assert_eq!(p.repo, "luna-pinyin");
        assert_eq!(p.project, "rime-luna-pinyin");
        assert_eq!(p.github_path(), "rime/rime-luna-pinyin");
    }

    #[test]
    fn test_user_repo() {
        let t = parse_target("lotem/rime-zhung@master").unwrap();
        let Target::Package(p) = t else { panic!() };
        assert_eq!(p.user, "lotem");
        assert_eq!(p.repo, "zhung");
        assert_eq!(p.project, "rime-zhung");
        assert_eq!(p.branch, Some("master".into()));
        assert_eq!(p.github_path(), "lotem/rime-zhung");
    }

    #[test]
    fn test_recipe_and_options() {
        let t = parse_target("luna-pinyin:simp:key1=val1,key2=val2").unwrap();
        let Target::Package(p) = t else { panic!() };
        assert_eq!(p.recipe, Some("simp".into()));
        assert_eq!(
            p.options,
            vec![
                ("key1".into(), "val1".into()),
                ("key2".into(), "val2".into()),
            ]
        );
    }

    #[test]
    fn test_conf_short_path() {
        let t = parse_target("lotem/rime-forge/lotem-packages.conf").unwrap();
        let Target::PackageList(pl) = t else { panic!() };
        assert!(pl.url.contains("github.com/lotem/rime-forge/raw/master"));
    }
}
