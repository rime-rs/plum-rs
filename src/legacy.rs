use anyhow::{Context, Result, bail};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

const SCRIPTS: &[(&str, &str)] = &[
    (
        "bootstrap.sh",
        include_str!("../vendor/plum/scripts/bootstrap.sh"),
    ),
    (
        "styles.sh",
        include_str!("../vendor/plum/scripts/styles.sh"),
    ),
    (
        "resolver.sh",
        include_str!("../vendor/plum/scripts/resolver.sh"),
    ),
    (
        "selector.sh",
        include_str!("../vendor/plum/scripts/selector.sh"),
    ),
    (
        "recipe.sh",
        include_str!("../vendor/plum/scripts/recipe.sh"),
    ),
    (
        "fetch-package.sh",
        include_str!("../vendor/plum/scripts/fetch-package.sh"),
    ),
    (
        "update-package.sh",
        include_str!("../vendor/plum/scripts/update-package.sh"),
    ),
];

pub fn bash() -> PathBuf {
    if let Some(path) = std::env::var_os("PLUM_BASH") {
        return path.into();
    }
    #[cfg(windows)]
    for variable in [
        "ProgramW6432",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "LOCALAPPDATA",
    ] {
        if let Some(base) = std::env::var_os(variable) {
            for suffix in ["Git/bin/bash.exe", "Programs/Git/bin/bash.exe"] {
                let path = Path::new(&base).join(suffix);
                if path.is_file() {
                    return path;
                }
            }
        }
    }
    "bash".into()
}

/// Explicit compatibility mode runs upstream Bash code with the caller's privileges.
/// It is not sandboxed and intentionally has the original scripts' side effects.
pub fn run(
    targets: &[String],
    plum: &Path,
    output: &Path,
    no_update: bool,
    select: bool,
) -> Result<()> {
    std::fs::create_dir_all(plum)?;
    for name in ["preset", "extra", "all"] {
        let filename = format!("{name}-packages.conf");
        let path = plum.join(&filename);
        if !path.exists() {
            std::fs::write(path, crate::conf::builtin(&filename).unwrap())?;
        }
    }
    let runtime = tempfile::tempdir_in(plum)?;
    let scripts = runtime.path().join("scripts");
    std::fs::create_dir_all(&scripts)?;
    for (name, content) in SCRIPTS {
        std::fs::write(scripts.join(name), content.replace("\r\n", "\n"))?;
    }
    std::fs::write(
        scripts.join("install-packages.sh"),
        include_str!("../vendor/plum/scripts/install-packages.sh").replace("\r\n", "\n"),
    )?;
    let launcher = scripts.join("compat.sh");
    std::fs::write(
        &launcher,
        r#"#!/usr/bin/env bash
set -e
export root_dir="$PLUM_DIR"
output_dir="$RIME_DIR"
if command -v cygpath >/dev/null 2>&1; then
  export root_dir="$(cygpath -u "$root_dir")"
  output_dir="$(cygpath -u "$output_dir")"
fi
script_dir="$(dirname "$0")"
export GIT_CEILING_DIRECTORIES="$root_dir"
if [[ "$PLUM_SELECT" == 1 ]]; then
  source "$script_dir/bootstrap.sh"
  require selector
  select_packages "$@"
  set -- "${selected_packages[@]}"
fi
for target in "$@"; do
  bash "$script_dir/install-packages.sh" "$target" "$output_dir"
done
"#,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for entry in std::fs::read_dir(&scripts)? {
            std::fs::set_permissions(entry?.path(), std::fs::Permissions::from_mode(0o755))?;
        }
    }
    let mut command = Command::new(bash());
    command
        .arg(&launcher)
        .args(targets)
        .env("PLUM_DIR", plum)
        .env("RIME_DIR", output);
    command.env("PLUM_SELECT", if select { "1" } else { "0" });
    if no_update {
        command.env("no_update", "1");
    } else {
        command.env_remove("no_update");
    }
    let status = command
        .status()
        .context("Shell 兼容模式需要 Bash、Git、curl 和常用 Unix 工具；可用 PLUM_BASH 指定 Bash")?;
    if !status.success() {
        bail!("上游 Shell 安装失败: {status}");
    }
    Ok(())
}
