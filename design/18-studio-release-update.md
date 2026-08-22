# Pure Studio 发布与应用内升级

本文定义 Pure Studio 稳定版的唯一发布渠道、Windows 打包格式和应用内更新信任边界。
首版只支持 Windows x64。RC 构建只作为 GitHub Actions artifact，不进入稳定更新源。

## 1. 版本与发布入口

Studio 使用 Release Please 的单一根组件管理版本。根 `studio-version.txt`、
`.release-please-manifest.json` 与带 `x-release-please-version` 注解的 Flutter
`pubspec.yaml` 由同一个 Release PR 同步更新；Flutter 版本只允许规范的稳定 `x.y.z`，不使用
`+build`。Release Please 从整个仓库收集 Conventional Commits：`fix:` 递增 patch，`feat:`
递增 minor，带 `!` 或 `BREAKING CHANGE:` 的提交递增 major。`ci:`、`docs:` 与 `chore:` 等
非产品修改不单独触发版本。人工审查并合并 Release PR 即批准发版，不再从 Actions 输入或推算版本。

`studio-release.yml` 按 Release Please 官方模式在 `main` push 或手动刷新时运行固定提交的
Release Please v5，并使用仓库专属 fine-grained PAT 创建或更新 Release PR，使 PR 自身及其
合并提交能触发正常 Actions。workflow 必须先通过 GitHub `/user` API 确认 PAT 属于仓库所有者，
不得以 `GITHUB_TOKEN` 代替。Release PR 维护根 `CHANGELOG.md`；合并后，同一次 workflow 从
Release Please 的 `release_created`、`upload_url`、`tag_name`、`version` 与 `sha` 输出解析
不可变 `v{x.y.z}` tag、draft Release 和精确数字 ID，并同步调用 reusable publisher。调用方
必须等待 publisher 完成，构建或发布失败直接使 Studio Release 失败，不通过新的 GitHub 事件或
API dispatch 串联。publisher 再从 GitHub API 和 tag 独立解析仓库、稳定 SemVer、提交 SHA 与
三个版本文件，拒绝人工提供的版本或 SHA。回滚只允许发布更高版本的 forward fix，不覆盖 tag 或
既有 Release。

publisher 分为构建与发布两阶段。Windows job 从 tag 的精确提交执行 Rust/Flutter 检查、构建、
签名、安装烟测、独立 verify 和 provenance，再把六个正式文件保存为同一 workflow run 的不可变
Actions artifact。publish job 下载该 artifact 并按 Release ID 对账：draft 可为空或只含部分资产，
但任何已有资产的名称、长度与 GitHub SHA-256 digest 必须和本地文件完全一致；只允许补传缺失资产，
不得覆盖不同字节。六项全部一致后才能取消 draft 并标记 latest。failed job 重跑复用原 artifact
继续补传；完整 draft 与已发布 Release 重跑均幂等成功。若存在尚未完成的稳定 draft，
`studio-release.yml` 优先通过同一个 reusable workflow 恢复 publisher，而不创建下一版
Release PR。Publisher 保留带精确 Release ID 的手动入口，仅用于故障恢复，不属于正常发版步骤。

GitHub draft Release 只对具备仓库 push 权限的身份可见，因此负责发现和解析 draft 的 job
使用 `contents: write`，但只执行读取；构建 job 仍保持 `contents: read`，只有最终 publish job
执行资产上传和 Release 状态修改。
解析 draft 时，其 GitHub 页面必须是同仓库生成的 `untagged-*` URL；取消 draft 后则必须严格
切换为对应 `v{x.y.z}` tag URL。

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

`cargo xtask release-gui stage|finalize|verify --version <semver>` 是正式打包入口。三个命令都
严格接受不带 `v`、prerelease 或 build metadata 的规范 `x.y.z`，并要求它与 pubspec 完全一致。
`stage` 复用 `build-gui`，生成 per-user Inno Setup 安装器和便携 zip；安装器使用稳定 AppId，
默认安装到 LocalAppData，声明 CloseApplications/RestartApplications。打包输入排除 PDB，
包含 LICENSE 与 THIRD_PARTY_NOTICES。便携版只供手动分发；便携用户执行应用内升级时进入
正式安装版，不对当前运行目录做原地覆盖。

采用 `pure_studio.exe` 新文件名的首个版本要求用户先手动卸载旧版再安装。安装器继续使用
同一 AppId、安装目录和用户数据目录，但允许直接覆盖安装，不检测也不删除旧 EXE 或旧 WER
配置；跳过手动卸载时，旧文件可能残留。新版快捷方式、卸载项、WER 配置和 Authenticode
检查只指向 `pure_studio.exe`。

安装器与便携包继续排除 PDB；Windows 构建必须同时产生独立、带 release version、commit
SHA 与 session protocol version 映射的 symbols artifact，收集 runner 和 Rust bridge 的匹配
PDB。symbols artifact 只用于崩溃分析，不作为公开更新资产，也不能被安装器加载。

Authenticode 是可选增强：证书存在时先签主 EXE/自有 DLL，再签最终安装器；缺少证书不
阻塞首版。Minisign/Ed25519 是强制信任根：生产公钥编译进 runtime，私钥与密码只存在于
GitHub Actions secrets。私钥轮换必须先通过仍受旧密钥信任的应用版本发布新的公钥集合。

`finalize` 只对最终字节生成 SHA-256、Minisign 签名、校验和文件与更新清单；`verify` 必须
独立复核文件集、版本、长度、哈希、签名和清单。tag 与 draft Release 由 Release Please 先创建，
但 CI 只有在安装器临时目录静默安装烟测及再次 `verify` 全部通过后才公开 Release。

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

`pl-studio-runtime` 的 updater owner 保存 canonical `UpdaterState`：Disabled、Idle、Checking、
UpToDate、Available、Downloading、Verifying、InstallerLaunched、CheckFailed、InstallFailed。
每个 variant 由独立 state struct 承载合法 payload；update、下载进度和 typed error 不再作为平行
optional 字段存在：

- `readUpdateState()` 只读 owner cache，不访问网络。
- `checkStudioUpdate()` 使用编译时当前版本并返回 `UpToDate | Available`，Flutter 不传
  `currentVersion`。
- `StudioUpdater::install(update, progress_sender)` 流式下载安装器与签名，校验声明长度和
  512 MiB 上限，计算 SHA-256，使用内置 Minisign 公钥验签，再启动 Inno Setup。

下载使用应用专属缓存目录与 `.partial` 文件。失败删除不完整文件，成功原子重命名；已验证
且与清单完全一致的缓存可复用。并发安装必须拒绝。启动安装器前 Bridge 再次确认没有活动
turn/task；若 runtime 已变忙则保留验证缓存并返回 `runtimeBusy`。空闲时安全关闭 runtime，
再使用 Inno Setup 的 silent/close/restart 参数启动安装器。

检查结果持久化到现有 `app_settings` 键 `observed:studioUpdate:v1`。页面打开只显示 canonical last-known state，
不自动检查。FRB 只公开 typed DTO 和事件：`readUpdateState()`、`checkStudioUpdate()`、
`installStudioUpdate(expectedRevision, version, eventSink)`；安装事件直接携带完整 canonical updater
state。Dart 不接收或解析 raw manifest JSON，也不维护第二套 install phase。

更新失败不得启动任何二进制。未签名、错误签名、内容篡改、超限、长度不符、URL 越界或
清单降级均属于终止错误；应用保留当前版本并允许用户重试检查或下载。

## 5. 生产诊断与后台进程

Studio 在 LocalAppData 的 `Pure Studio/logs` 写入按日滚动 Rust 与 Dart error 日志，panic marker
和 native dump 写入 `Pure Studio/crashes`。默认 Rust filter 为 `warn`，CLI `--log-level` 优先于
`RUST_LOG`；启动、每小时与正常关闭清理最后修改时间超过 48 小时的自有日志和 crash 文件。
完整 prompt、context 和工具结果不进入 tracing；日志只记录 root/agent/session 身份、cursor、
运行阶段、条目规模、耗时和 outcome。panic 与 error 使用同步兜底持久化，正常关闭显式 flush。
详细合同见 `19-studio-storage-and-diagnostics.md`。Windows runner 为当前 exe 配置 WER
LocalDumps，并保留 in-process unhandled exception minidump 兜底；`0xc0000409/BEX64`
在没有匹配 dump/PDB 时只能报告现象，不能宣称唯一根因。

所有由 GUI 发起的后台 Git、worktree、task/review/merge、Docker、MCP/LSP 与终止辅助命令
在 Windows 使用 `CREATE_NO_WINDOW`；其他平台保持既有后台语义。只有用户显式打开交互终端
或安装器等外部 UI 时允许正常显示窗口。
