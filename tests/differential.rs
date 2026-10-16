use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn command(cache: &Path, output: &Path, shell: bool) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rime-install"));
    command
        .arg("--plum-dir")
        .arg(cache)
        .arg("--rime-dir")
        .arg(output)
        .arg("--no-update");
    command
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "protocol.https.allow")
        .env("GIT_CONFIG_VALUE_0", "never");
    if shell {
        command.arg("--shell-compat");
    }
    command
}
fn fixture(cache: &Path, name: &str) -> PathBuf {
    let path = cache.join("package/rime").join(name);
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("a.yaml"), "schema: original\n").unwrap();
    let mut options = git2::RepositoryInitOptions::new();
    options.initial_head("master");
    let repo = git2::Repository::init_opts(&path, &options).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("a.yaml")).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = git2::Signature::now("test", "test@example.com").unwrap();
    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, "fixture", &tree, &[])
        .unwrap();
    repo.reference("refs/remotes/origin/master", oid, true, "fixture")
        .unwrap();
    repo.reference_symbolic(
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/master",
        true,
        "fixture",
    )
    .unwrap();
    repo.remote(
        "origin",
        &format!("https://github.com/rime/rime-{name}.git"),
    )
    .unwrap();
    path
}
fn files(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        if !dir.exists() {
            return;
        }
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), out);
            } else {
                out.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut out = Default::default();
    visit(root, root, &mut out);
    out
}
fn run(command: &mut Command) {
    let result = command.output().unwrap();
    assert!(
        result.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn native_matches_upstream_file_selection_patch_sequences_and_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    let pkg = fixture(&cache, "demo");
    let native = temp.path().join("native");
    let original = temp.path().join("original");
    for body in [
        "recipe:\n  Rx: demo\n",
        "patch_files:\n  default.custom.yaml:\n    # retained comment\n    menu/page_size: 9\n    stale: true\n",
        "patch_files:\n  default.custom.yaml:\n    # retained comment\n    menu/page_size: 8\n",
        "patch_files:\n  list.custom.yaml:\n    - foo.yaml:/patch\n    - bar.yaml:/patch\n",
    ] {
        fs::write(pkg.join("recipe.yaml"), body).unwrap();
        run(command(&cache, &native, false).arg("demo"));
        run(command(&cache, &original, true).arg("demo"));
        assert_eq!(files(&native), files(&original), "recipe: {body}");
    }
    assert!(
        !fs::read_to_string(native.join("default.custom.yaml"))
            .unwrap()
            .contains("stale")
    );
    fs::remove_file(pkg.join("recipe.yaml")).unwrap();
    for (name, body) in [
        ("lua/a.lua", "return true"),
        ("dict/a.dict.yaml", "skip"),
        ("opencc/a.ocd2", "skip"),
        ("opencc/a.json", "{}"),
        ("skip.custom.yaml", "skip"),
    ] {
        let path = pkg.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    run(command(&cache, &native, false).arg("demo"));
    run(command(&cache, &original, true).arg("demo"));
    assert_eq!(files(&native), files(&original));
}

#[test]
fn explicit_shell_mode_handles_dynamic_lists_and_command_substitution() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    let pkg = fixture(&cache, "demo");
    let out = temp.path().join("out");
    fs::write(
        pkg.join("recipe.yaml"),
        "patch_files:\n  default.custom.yaml:\n    key: $(printf shell-value)\n",
    )
    .unwrap();
    let list = temp.path().join("custom.conf");
    fs::write(
        &list,
        "package_list=()\nif true; then\n  package_list+=(demo)\nfi\n",
    )
    .unwrap();
    run(command(&cache, &out, true).arg(&list));
    assert!(
        fs::read_to_string(out.join("default.custom.yaml"))
            .unwrap()
            .contains("shell-value")
    );
}

#[test]
fn build_shared_install_clean_and_dist_work_offline() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    let pkg = fixture(&cache, "demo");
    let out = temp.path().join("out");
    fs::write(pkg.join("extra.txt"), "data").unwrap();
    fs::write(cache.join("all-packages.conf"), "package_list=(demo)\n").unwrap();
    run(command(&cache, &out, false).args(["--build", "all"]));
    assert!(out.join("a.yaml").is_file());
    let shared = temp.path().join("shared");
    run(command(&cache, &out, false)
        .arg("--install-shared")
        .arg(&shared));
    assert!(shared.join("a.yaml").is_file());
    fs::write(out.join("unrelated.yaml"), "keep").unwrap();
    run(command(&cache, &out, false).arg("--clean-output"));
    assert!(!out.join("a.yaml").exists());
    assert!(out.join("unrelated.yaml").is_file());
    let source = temp.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("Cargo.toml"), "# fixture").unwrap();
    fs::write(source.join("plum-rs-old.tar.gz"), "previous archive").unwrap();
    let archive = temp.path().join("dist.tar.gz");
    run(command(&cache, &out, false)
        .arg("--source-dir")
        .arg(&source)
        .arg("--dist")
        .arg(&archive));
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(
        fs::File::open(archive).unwrap(),
    ));
    let entries = tar
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(entries.contains(&"plum-rs/Cargo.toml".into()));
    assert!(!entries.contains(&"plum-rs/plum-rs-old.tar.gz".into()));
    assert!(entries.contains(&"plum-rs/.plum/package/rime/demo/a.yaml".into()));
}
#[test]
fn minimal_output_matches_upstream_transformations() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join("cache");
    let essay = fixture(&cache, "essay");
    fs::write(essay.join("essay.txt"), "common\t500\nrare\t499\n").unwrap();
    let luna = fixture(&cache, "luna-pinyin");
    fs::write(
        luna.join("luna_pinyin.dict.yaml"),
        "version: \"1.0\"\n#以下爲詞組\nremove\n",
    )
    .unwrap();
    fs::write(
        luna.join("luna.schema.yaml"),
        "version: 1.0\n  - stroke\n  - reverse_lookup_translator\n",
    )
    .unwrap();
    let prelude = fixture(&cache, "prelude");
    fs::write(
        prelude.join("default.yaml"),
        "config_version: 1.0\nschema_list:\n  - schema: luna\n  - schema: missing\n",
    )
    .unwrap();
    let native = temp.path().join("native");
    let original = temp.path().join("original");
    run(command(&cache, &native, false).args(["--build", "minimal"]));
    run(command(&cache, &original, true).args(["essay", "luna-pinyin", "prelude"]));
    let upstream = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/plum/scripts/minimal-build.sh"),
    )
    .unwrap();
    let transform = upstream
        .split("pushd \"${output_dir}\" > /dev/null")
        .nth(1)
        .unwrap()
        .replace("popd > /dev/null", "");
    let script = temp.path().join("minimal.sh");
    fs::write(&script, format!("set -e\ncd \"$1\"\n{transform}\n")).unwrap();
    let bash = if cfg!(windows) {
        PathBuf::from("C:/Program Files/Git/bin/bash.exe")
    } else {
        "bash".into()
    };
    run(Command::new(bash)
        .arg(&script)
        .arg(original.to_string_lossy().replace('\\', "/")));
    assert_eq!(files(&native), files(&original));
}
