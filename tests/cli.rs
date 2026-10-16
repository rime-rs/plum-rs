use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn command(plum: &Path, output: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rime-install"));
    cmd.args(["--plum-dir"])
        .arg(plum)
        .arg("--rime-dir")
        .arg(output)
        .arg("--no-update");
    cmd.env_remove("no_update");
    cmd
}
fn cached(plum: &Path, name: &str) {
    let pkg = plum.join("package/rime").join(name);
    fs::create_dir_all(&pkg).unwrap();
    fs::write(pkg.join(format!("{name}.schema.yaml")), "schema: test").unwrap();
}

#[test]
fn legacy_environment_variables() {
    let temp = tempfile::tempdir().unwrap();
    let plum = temp.path().join("cache");
    let out = temp.path().join("rime");
    cached(&plum, "luna-pinyin");
    let result = Command::new(env!("CARGO_BIN_EXE_rime-install"))
        .env_remove("RIME_DIR")
        .env_remove("PLUM_DIR")
        .env("rime_dir", &out)
        .env("plum_dir", &plum)
        .env("no_update", "0")
        .arg("luna-pinyin")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(out.join("luna-pinyin.schema.yaml").is_file());
}
#[test]
fn original_style_all_and_explicit_preset_in_unicode_paths() {
    let temp = tempfile::tempdir().unwrap();
    let plum = temp.path().join("缓存 plum");
    let output = temp.path().join("输出 Rime");
    fs::create_dir_all(&plum).unwrap();
    fs::write(
        plum.join("preset-packages.conf"),
        "package_list+=(\n luna-pinyin # real comment\n)\n",
    )
    .unwrap();
    fs::write(
        plum.join("extra-packages.conf"),
        "package_list+=( terra-pinyin )\n",
    )
    .unwrap();
    cached(&plum, "luna-pinyin");
    cached(&plum, "terra-pinyin");
    let result = command(&plum, &output).arg(":all").output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.join("luna-pinyin.schema.yaml").is_file());
    assert!(output.join("terra-pinyin.schema.yaml").is_file());
    let other = temp.path().join("default");
    let result = command(&plum, &other)
        .arg(":preset")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(other.join("luna-pinyin.schema.yaml").is_file());
    assert!(!other.join("terra-pinyin.schema.yaml").exists());
}
#[test]
fn no_target_shows_help_without_touching_config_or_cache() {
    let temp = tempfile::tempdir().unwrap();
    let plum = temp.path().join("cache");
    let output = temp.path().join("rime");
    fs::create_dir_all(&output).unwrap();
    let config = output.join("default.custom.yaml");
    fs::write(&config, "patch:\n  menu/page_size: 7\n").unwrap();
    for args in [
        vec![],
        vec!["--no-update"],
        vec!["--shell-compat"],
        vec!["--recursive"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_rime-install"))
            .env("RIME_DIR", &output)
            .env("PLUM_DIR", &plum)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(result.status.success(), "{:?}", result);
        assert!(String::from_utf8_lossy(&result.stdout).contains("--help"));
        assert!(!plum.exists());
        assert_eq!(
            fs::read_to_string(&config).unwrap(),
            "patch:\n  menu/page_size: 7\n"
        );
        assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
    }
    let result = command(&plum, &output).output().unwrap();
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("--help"));
    assert!(!plum.exists());
}
#[test]
fn zip_and_recipe_options_and_failure_status() {
    let temp = tempfile::tempdir().unwrap();
    let plum = temp.path().join("plum");
    let output = temp.path().join("output");
    let zip = temp.path().join("本地 package.zip");
    let mut writer = zip::ZipWriter::new(fs::File::create(&zip).unwrap());
    writer
        .start_file("repo/a.yaml", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"schema: zip").unwrap();
    writer.finish().unwrap();
    assert!(
        command(&plum, &output)
            .arg(&zip)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(output.join("a.yaml").is_file());
    cached(&plum, "luna-pinyin");
    let pkg = plum.join("package/rime/luna-pinyin");
    fs::write(pkg.join("recipe.yaml"), "install_files: luna-pinyin.schema.yaml\npatch_files:\n  default.custom.yaml:\n    menu/page_size: ${size}\n").unwrap();
    let result = command(&plum, &output)
        .arg("luna-pinyin::size=8")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        fs::read_to_string(output.join("default.custom.yaml"))
            .unwrap()
            .contains('8')
    );
    fs::write(pkg.join("recipe.yaml"),"install_files: luna-pinyin.schema.yaml\npatch_files:\n  default.custom.yaml: $(echo unsupported)\n").unwrap();
    let result = command(&plum, &output).arg("luna-pinyin").output().unwrap();
    assert!(!result.status.success());
}
#[test]
fn explicit_interactive_mode_reads_targets_then_exits() {
    let temp = tempfile::tempdir().unwrap();
    let plum = temp.path().join("plum");
    let output = temp.path().join("out");
    cached(&plum, "luna-pinyin");
    let mut child = command(&plum, &output)
        .arg("--interactive")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"luna-pinyin\nq\n")
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.join("luna-pinyin.schema.yaml").is_file());
}
