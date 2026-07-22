# Pure Studio 发布与应用内升级

本文定义 Pure Studio 稳定版的唯一发布渠道、Windows 打包格式和应用内更新信任边界。
首版只支持 Windows x64。RC 构建只作为 GitHub Actions artifact，不进入稳定更新源。

## 1. 版本与发布入口

`code/pure-studio-flutter/pubspec.yaml` 的 `version: x.y.z+build` 是 Studio 版本唯一事实源。
稳定版只允许在 `main` 上手动运行 `studio-release.yml`。发布表单默认选择修复，对应 patch
递增；功能增加对应 minor 递增并清零 patch；勾选大版本时忽略变更类型，递增 major 并清零
minor 和 patch。每次发布同时将 build number 递增一。CI 自动生成
`chore(studio): prepare v{x.y.z}` 版本提交，不再要求人工计算或输入版本号。

版本提交先推送到固定临时分支 `studio-release/active`，内部发布工作流从该精确提交构建、签名、
烟测并生成 provenance。全部验证成功后才允许将版本提交快进到 `main`，再创建不可变的
`v{x.y.z}` tag 和 GitHub Release。失败时不得推进 `main`，临时分支作为并发锁和重试来源保留；
成功后只能用精确 SHA lease 删除。已有正式 Release 必须拒绝覆盖；同一提交留下的 tag 或草稿
Release 可继续完成。回滚通过更高版本 forward fix 完成。

稳定 Release 固定包含：

- `Pure-Studio-{version}-windows-x86_64-setup.exe`
- `Pure-Studio-{version}-windows-x86_64-portable.zip`
- 上述两个文件各自的 `.minisig`
- `latest.json`
- `SHA256SUMS.txt`

发布不执行 `cargo publish`，GitHub Release 是唯一正式分发渠道。Flutter 与 Rust toolchain
版本由 workflow 固定；第三方 Action 固定到完整 commit SHA，并使用最小 token 权限、
单实例 concurrency 与 GitHub build provenance attestation。

## 2. Windows 包边界

`cargo xtask release-gui prepare --bump patch|minor|major` typed 解析并更新 pubspec，输出包含
`version`、`buildNumber` 和 `pubspecVersion` 的 camelCase JSON。CI 只允许该步骤修改
`pubspec.yaml`。`cargo xtask release-gui stage|finalize|verify --version <semver>` 是正式打包入口。
`stage` 复用 `build-gui`，生成 per-user Inno Setup 安装器和便携 zip；安装器使用稳定 AppId，
默认安装到 LocalAppData，声明 CloseApplications/RestartApplications。打包输入排除 PDB，
包含 LICENSE 与 THIRD_PARTY_NOTICES。便携版只供手动分发；便携用户执行应用内升级时进入
正式安装版，不对当前运行目录做原地覆盖。

Authenticode 是可选增强：证书存在时先签主 EXE/自有 DLL，再签最终安装器；缺少证书不
阻塞首版。Minisign/Ed25519 是强制信任根：生产公钥编译进 runtime，私钥与密码只存在于
GitHub Actions secrets。私钥轮换必须先通过仍受旧密钥信任的应用版本发布新的公钥集合。

`finalize` 只对最终字节生成 SHA-256、Minisign 签名、校验和文件与更新清单；`verify` 必须
独立复核文件集、版本、长度、哈希、签名和清单。CI 在创建 tag/Release 前执行安装器临时
目录静默安装烟测并再次执行 `verify`。

## 3. 更新清单

稳定检查地址固定为：

`https://github.com/ZR233/pure-lang/releases/latest/download/latest.json`

`latest.json` 是 camelCase typed JSON：

```json
{
  "schemaVersion": 1,
  "version": "1.2.3",
  "publishedAt": 1770000000,
  "notesUrl": "https://github.com/ZR233/pure-lang/releases/tag/v1.2.3",
  "platforms": {
    "windows-x86_64": {
      "url": "https://github.com/ZR233/pure-lang/releases/download/v1.2.3/Pure-Studio-1.2.3-windows-x86_64-setup.exe",
      "size": 123456,
      "sha256": "...",
      "signature": "https://github.com/ZR233/pure-lang/releases/download/v1.2.3/Pure-Studio-1.2.3-windows-x86_64-setup.exe.minisig"
    }
  }
}
```

时间戳是 Unix 秒 `i64`。客户端拒绝未知 schema、非稳定 SemVer、同版或降级、非 HTTPS、
非 `ZR233/pure-lang` 资源、异常 port/userinfo/query、以及 tag、version、文件名不一致的
URL。清单与签名下载最多跟随五次重定向，重定向目标仅允许 GitHub Release/CDN HTTPS
主机。

## 4. 更新状态机与安装

`pl-studio-runtime` 的 updater 是独立边界：

- `StudioUpdater::check(current_version)` 返回 `UpToDate | Available`。
- `StudioUpdater::install(update, progress_sender)` 流式下载安装器与签名，校验声明长度和
  512 MiB 上限，计算 SHA-256，使用内置 Minisign 公钥验签，再启动 Inno Setup。

下载使用应用专属缓存目录与 `.partial` 文件。失败删除不完整文件，成功原子重命名；已验证
且与清单完全一致的缓存可复用。并发安装必须拒绝。启动安装器前 Bridge 再次确认没有活动
turn/task；若 runtime 已变忙则保留验证缓存并返回 `runtimeBusy`。空闲时安全关闭 runtime，
再使用 Inno Setup 的 silent/close/restart 参数启动安装器。

FRB 只公开 typed DTO 和事件：`checkStudioUpdate(currentVersion)`、
`installStudioUpdate(update, eventSink)`，以及 `Started`、`Progress`、`Verifying`、
`InstallerLaunched`、`Failed`。Dart 不接收或解析 raw manifest JSON。

更新失败不得启动任何二进制。未签名、错误签名、内容篡改、超限、长度不符、URL 越界或
清单降级均属于终止错误；应用保留当前版本并允许用户重试检查或下载。
