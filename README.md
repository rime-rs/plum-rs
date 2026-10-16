# 东风破 /plum/

Rime 配置管理器的 Rust 实现。普通安装无需 Git、Bash、curl 或 7z。
安装源文件后，仍由输入法前端部署和启用方案。

```powershell
rime-install --rime-dir "D:\我的 Rime" luna-pinyin
rime-install :preset
rime-install :all
rime-install lotem/rime-zhung@master
rime-install https://github.com/rime/rime-luna-pinyin.git
rime-install https://github.com/rime/rime-luna-pinyin/tree/master
rime-install "D:\下载\方案.zip"
rime-install user/repo:recipe:option=value
rime-install --select :all
```

无参数运行仅显示帮助并退出，不联网、不下载、不修改配置。只指定目录、`--no-update` 或 `--shell-compat` 等选项而未指定目标或操作时，也只显示帮助。安装预设包需显式运行 `rime-install :preset`。

`--select` 显示编号菜单（未指定候选目标时使用 `:preset`），可输入包名、`clear` 重选、`.` 完成、`quit` 取消。`--interactive` 是额外的循环输入模式；空行安装 `:preset`，`q` 退出。

## 目录与缓存

- `--rime-dir` / `RIME_DIR`（兼容 `rime_dir`）指定输出目录。
- `--rime-frontend` / `rime_frontend` 支持 `ibus-rime`、`fcitx-rime`、`fcitx5-rime`、`squirrel`、`weasel` 及原版组织前缀形式。默认 Linux 使用 ibus-rime、macOS 使用 Squirrel，目录无需事先存在。Windows 保留注册表自定义目录探测。
- `--plum-dir` / `PLUM_DIR`（兼容 `plum_dir`）指定下载缓存，默认系统用户缓存目录中的 `plum-rs`。与原版 ZIP 缓存分开使用。
- `--no-update` 或非空 `no_update` 跳过当前分支的更新；指定其他分支时仍会切换，缓存不足时会获取所需分支。非 Git 目录可作为手动准备的离线包。

Git 克隆和更新递归处理子模块，切换分支不会将当前分支引用改写成其他分支。保留拒绝覆盖缓存中本地跟踪文件修改、校验 origin 的保护。与原版一样，不指定分支时回到缓存记录的远端默认分支。

`:preset`、`:extra`、`:all` 清单内置于程序，固定为上游 `b1be1969f914cc005add4090631b855db00c2591`；`quick` 位于 preset。缓存目录下同名 `*-packages.conf` 优先。首次获取包仍需网络。

本地/远程 `.conf` 支持 `package_list=(...)`、`package_list+=(...)`、注释、引号、`source` / `.`。`${root_dir}` 和 `${root_dir:-.}` 指向缓存根目录；普通 source 相对于调用进程工作目录，与 Bash 一致。动态 Shell 清单使用下文兼容模式。

## 文件选择与配方

默认文件集合恢复原版规则：根目录 YAML、TXT、GRAM，以及 `opencc` 下 JSON/OCD/TXT；排除根目录 `*.custom.yaml`、`*.recipe.yaml`、`recipe.yaml`。`--recursive` 可额外递归收集 Lua、嵌套词库、JSON、OCD2 等，排除隐藏目录和 build/target。

存在 recipe 时，仅复制 `install_files` 显式指定的文件；省略该字段不会回退到默认文件集合。

```yaml
recipe:
  Rx: demo # demo.recipe.yaml 的 Rx 必须匹配；recipe.yaml 可省略
install_files:
  - '*.schema.yaml'
download_files:
  - extra.dict.yaml::https://example.com/extra.dict.yaml
patch_files:
  default.custom.yaml:
    menu/page_size: ${page_size:-9}
  schema.custom.yaml:
    - common.yaml:/patch
```

- `download_files` 支持 URL 和 `文件名::URL`，需要安装的下载文件应再列入 `install_files`。已有文件发送 `If-Modified-Since`，接受 304；即使 no_update 也检查外部文件，与原版配方一致。
- `patch_files` 支持映射和序列，按原文写入 `__patch`。使用原版 `# Rx: package:recipe:options {` / `# }` 标记，同一 recipe 和选项再次应用时替换整个旧块，清除已删除的旧键；其他配方块、注释、引号和排版保留。
- 原生模式支持命名变量、环境变量和 `${name:-default}`。未定义变量展开为空，与未启用 nounset 的 Bash 一致。未使用的选项允许保留。复杂参数展开、命令替换和可执行清单可切换 Shell 兼容模式。
- 原生模式对未知 recipe 顶层字段、无匹配 glob、非法相对输出路径报错；不模拟原脚本静默忽略或继续执行的错误行为。

每个原生包先准备再提交输出，单文件同目录原子替换，写入失败尝试恢复该包已写文件。多包与崩溃/断电不保证整体事务。ZIP 拒绝路径穿越和符号链接；下载上限 128 MiB，ZIP 解压总量上限 512 MiB。

## 完整 Shell 兼容模式

```text
rime-install --shell-compat user/repo:recipe:key=value
rime-install --shell-compat ./dynamic-packages.conf
rime-install --shell-compat --select :all
```

此模式嵌入并运行固定上游版本的安装、清单和配方脚本，支持 Bash eval、命令替换、Shell 条件与函数等原始语义。需要 Bash、Git、curl、sed 等 Unix 工具；Windows 自动查找 Git for Windows，或通过 `PLUM_BASH` 指定 Bash。没有这些工具时明确报错，不自动安装系统软件。

兼容模式会以当前用户权限执行清单和配方中的 Shell 代码，采用原版文件写入和更新行为，不提供原生模式的回滚和路径限制。仅用于信任的 Git 包和清单。ZIP 安装使用原生入口。

## 数据构建、共享安装与源码分发

```text
rime-install --build preset --rime-dir ./output
rime-install --build minimal --rime-dir ./output
rime-install --build all --compile --rime-dir ./output
rime-install --compile --rime-dir ./output
rime-install --install-shared ./dest/usr/share/rime-data --rime-dir ./output
rime-install --source-dir . --dist ./plum-rs-data.tar.gz
rime-install --clean-output --rime-dir ./output
```

构建模式默认输出到缓存目录/output，独立暂存后再发布。minimal 按原版规则裁剪 essay 低频词条、截断朙月词组、关闭反查组件并收敛默认 schema_list。`--compile` 调用外部 `rime_deployer --build`，需要安装 librime；成功后移除生成的 user.yaml。

Windows 的 rime_deployer 及其依赖 DLL 都需能被系统找到；采用 librime 的 dist 布局时，将 dist/bin 和 dist/lib 一起加入当前进程 PATH。

共享安装选择根目录、build/opencc 的文件，Unix 权限设为 0644。`--dist` 先准备 all 数据，再打包源码和包缓存为 tar.gz（排除 .git、target、output）。源码包可用缓存离线复现。clean 和构建更新只清理构建清单记录的旧产物，保留用户自行放入的其他文件；与原版直接删除整个输出目录不同。

提供 Makefile 的 `preset`、`extra`、`all`、`minimal`、`preset-bin`、`all-bin`、`minimal-bin`、`build`、`install`、`clean`、`dist` 目标，支持 OUTPUT、PLUM_DIR、PREFIX、DESTDIR、RIME_DATA_DIR、VERSION 和 RIME_INSTALL 覆盖。

## 自更新、分发与验证

`rime-install plum` 查询本项目 GitHub 最新稳定发行版，选择当前目标架构的资产，校验官方 SHA256SUMS.txt，备份旧程序后替换，失败恢复。仅在较新版本且当前平台提供资产时更新。Windows 正在运行的旧文件若无法删除会保留备份路径，退出后可清理。

MSVC x64、ARM64、i686 使用静态 CRT 和 asInvoker manifest。常规 Windows 目标按 Win10 / Server 2016 及以后规划，i686 不意味着 Win8.1 兼容。`windows/rime-install.bat` 是可选的原 BAT 配置适配层。

```text
cargo fmt --all -- --check
cargo test --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked online_clone_and_update -- --ignored --nocapture
```

差分测试使用 vendor 中固定版本的上游脚本，要求 Bash/Git/sed 等工具；覆盖实际文件集合、序列补丁、整块更新、动态清单和 Shell 配方。普通测试不访问公网；最后一项需要网络，写入临时目录。

原版来源：[rime/plum](https://github.com/rime/plum/tree/b1be1969f914cc005add4090631b855db00c2591)。上游脚本未修改，来源和许可证保存在 vendor/plum；本项目采用 LGPL-3.0-or-later，分发时需保留许可及对应源码信息。数据包各自的许可证也需要保留。
