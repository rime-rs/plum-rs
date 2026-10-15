# 东风破 /plum/

Rime 配置管理器的 Rust 实现。无需 Git、Bash、curl 或 7z。

```powershell
rime-install --rime-dir "D:\我的 Rime" luna-pinyin
rime-install :preset
rime-install :all
rime-install lotem/rime-zhung@master
rime-install https://github.com/rime/rime-luna-pinyin.git
rime-install "D:\下载\方案.zip"
rime-install user/repo:recipe:option=value
```

无参数且连接终端时进入循环输入；空行安装 `:preset`，`q` 退出。
无参数且输入被重定向时默认安装 `:preset`，适用于构建脚本。
`--interactive` 可显式启用输入循环。非交互安装失败返回非零退出码；交互模式显示错误后可继续输入。

## 目录与环境变量

- `--rime-dir` / `RIME_DIR`（兼容 `rime_dir`）：输出目录。Windows 优先读取 HKCU `Software\Rime\Weasel\RimeUserDir`，否则使用 `%APPDATA%\Rime`；目录不存在也可以创建。
- `--plum-dir` / `PLUM_DIR`（兼容 `plum_dir`）：默认系统用户缓存目录中的 `plum-rs`，转换为绝对路径。请与原 plum 的 ZIP 缓存分开。
- `--no-update`：跳过已有缓存包和已下载的 recipe 外部文件。兼容非空 `no_update` 环境变量（包括字符串 `0`，与旧脚本一致）。

`:preset`、`:extra`、`:all` 的清单已内置，离线可解析。`--plum-dir` 下同名的 `*-packages.conf` 可覆盖内置版本。清单只列出包名，首次安装包仍需要网络。
支持本地 `.conf`、远程清单 URL 和 `user/repo@branch/name-packages.conf`；解析 `package_list=(...)`、`package_list+=(...)`、注释、引号，以及递归 `source` / `.`。source 相对清单所在目录解析，识别 `${root_dir:-.}`；循环与无法支持的 Shell 语句报错。

## 配方

```yaml
recipe:
  Rx: demo # 指定 demo.recipe.yaml 时与文件名一致；recipe.yaml 可省略
  description: 示例
install_files:
  - '*.schema.yaml'
```

完整示例：

```yaml
download_files:
  - extra.dict.yaml::https://example.com/extra.dict.yaml
install_files:
  - '*.schema.yaml'
  - extra.dict.yaml
  - lua/**/*.lua
patch_files:
  default.custom.yaml:
    menu/page_size: ${page_size:-9}
```

支持 `download_files`、`install_files` 和 `patch_files`。补丁条目合并到目标文件的 `__patch` 映射；已有其他键保留，同名键更新。目标必须是 YAML 映射。写回使用 YAML 序列化，注释和排版会重新生成。
支持 `$option`、`${option}`、`${option:-default}` 字符串替换，命令行示例：`user/repo:demo:page_size=8`。未定义变量、未使用选项、未知顶层字段、旧 `patch` 字段、命令替换、Shell 脚本补丁均明确报错，不会假装安装成功。任意 Bash `eval`、脚本执行及完整 Shell 兼容不在支持范围；复杂旧配方继续使用原 plum。

显式 glob 无匹配会失败。无 `install_files` 时递归收集 YAML、TXT、GRAM、Lua、JSON、OCD/OCD2，排除隐藏目录、build/target、`*.custom.yaml`、`*.recipe.yaml` 和 `recipe.yaml`。ZIP 支持常见 GitHub 单层仓库目录，拒绝路径穿越和符号链接；下载上限 128 MiB，解压总量上限 512 MiB。

每个包准备完成后才写输出，单文件使用同目录临时文件原子替换；后续文件写入失败时尝试恢复该包先前文件并报告回滚错误。多包之间和进程崩溃/断电不保证整体事务，可能留下空目录。克隆成功后才公布缓存目录；更新拒绝 origin 不一致或包含本地跟踪文件修改的缓存。`--no-update` 可使用手工准备的普通目录缓存。

## Weasel 接入与平台

将 EXE 与 `windows/rime-install.bat` 放在同一目录，原 `rime-install-config.bat` 可保留：适配脚本会调用它，并通过独立 `PLUM_RS_DIR`（默认 `%LOCALAPPDATA%\plum-rs`）隔离缓存。直接运行 EXE 不执行 BAT 文件。已有自定义输出目录通过配置中的 `rime_dir` 或显式参数传入。
Weasel 调用方仍需负责退出后的方案列表刷新和部署；本仓库不修改 Weasel 安装器或构建脚本。

MSVC 的 x64、ARM64、i686 构建静态链接 CRT；发布版本检查 tag 与 `--version` 对应的 Cargo 版本一致，避免旧版本号。CI 在 Windows、Linux、macOS 运行回归及真实 Git HTTPS 克隆/更新。
常规 Rust Windows 目标按 Windows 10 / Server 2016 及以后规划；增加 i686 仅提供 32 位架构，**不代表 Windows 8.1 兼容**。Win8.1 与复杂旧配方仍需旧工具，干净虚拟机和真实 Weasel 安装包测试需由集成方完成。

## 开发验证

```text
cargo fmt --all -- --check
cargo test --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked online_clone_and_update -- --ignored --nocapture
```

最后一项需要网络，默认测试不访问公网。所有安装测试只写临时目录。

内置清单取自 [rime/plum](https://github.com/rime/plum/tree/cab9ed35289f2022b6888503750d2dfb1851dbe0) 的固定提交 `cab9ed35289f2022b6888503750d2dfb1851dbe0`。项目采用 LGPL-3.0-or-later，见 LICENSE；分发和修改需保留许可及对应源码信息。
