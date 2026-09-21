# 23 - anywork 发布与应用内升级

本文定义 anywork 稳定版的唯一发布渠道、Windows 打包格式和应用内更新信任边界。首版只
支持 Windows x64；RC 构建只作为 CI artifact，不进入稳定更新源。SSH remote helper 不建立
独立发布合同：helper 以压缩资产嵌入 Rust bridge（见 [22](./22-ssh-remote.md)），GitHub
Release、安装目录与本地数据目录均不出现独立 helper 文件。

## 23.1 版本与发布流

Studio 使用 Release Please 的单一根组件管理版本：根版本文件、release manifest 与带版本
注解的 Flutter pubspec 由同一个 Release PR 同步更新；Flutter 版本只允许规范的稳定
`x.y.z`，不使用 `+build`。版本从整个仓库的 Conventional Commits 收集：`fix:` 递增
patch，`feat:` 递增 minor，带 `!` 或 `BREAKING CHANGE:` 的提交递增 major；`ci:`、
`docs:` 与 `chore:` 等非产品修改不单独触发版本。人工审查并合并 Release PR 即批准发版，
不从 Actions 输入或推算版本。

发布 workflow 按 Release Please 官方模式运行，并使用仓库专属 fine-grained PAT 创建或
更新 Release PR，使 PR 自身及其合并提交能触发正常 Actions；workflow 必须先确认 PAT
属于仓库所有者，不得以默认 token 代替。Release PR 维护根 CHANGELOG；合并后，同一次
workflow 从 Release Please 的输出解析不可变 `v{x.y.z}` tag、draft Release 和精确数字
ID，并同步调用可复用的 publisher；调用方必须等待 publisher 完成，构建或发布失败直接使
Release 失败，不通过新的 GitHub 事件或 API dispatch 串联。publisher 再从 GitHub API 和
tag 独立解析仓库、稳定 SemVer、提交 SHA 与三个版本文件，拒绝人工提供的版本或 SHA。
回滚只允许发布更高版本的 forward fix，不覆盖 tag 或既有 Release。

publisher 分为 helper、GUI 构建与发布三阶段。Linux helper job 从 tag 的精确提交交叉编译
两种 musl 架构并生成 SHA-256，只作为同一 workflow run 的内部 artifact；Windows job 消费
这组 helper 并压缩嵌入 Rust bridge，再执行 Rust/Flutter 检查、GUI 构建、签名、安装烟测、
独立 verify 和 provenance；helper 本身不进入正式 Release 文件集。publish job 下载最终
artifact 并按 Release ID 对账：draft 可为空或只含部分资产，但任何已有资产的名称、长度
与 SHA-256 digest 必须和本地文件完全一致；只允许补传缺失资产，不得覆盖不同字节。全部
资产一致后才能取消 draft 并标记 latest。failed job 重跑复用原 artifact 继续补传；完整
draft 与已发布 Release 重跑均幂等成功。若存在尚未完成的稳定 draft，发布 workflow 优先
恢复该 publisher，而不创建下一版 Release PR；publisher 保留带精确 Release ID 的手动
入口，仅用于故障恢复。

权限分离：draft Release 只对具备仓库 push 权限的身份可见，因此负责发现和解析 draft 的
job 使用写权限但只执行读取；构建 job 保持只读，只有最终 publish job 执行资产上传和
Release 状态修改。解析 draft 时，其页面必须是同仓库生成的 untagged URL；取消 draft 后
必须严格切换为对应 tag URL。

稳定 Release 固定包含：Windows 安装器与便携包、各自的 `.minisig` 签名、`latest.json`
更新清单与 `SHA256SUMS.txt`。发布不执行 crate 发布，GitHub Release 是唯一正式分发渠道。
Flutter 与 Rust toolchain 版本由 workflow 固定；第三方 Action 固定到完整 commit SHA，并
使用最小 token 权限、单实例 concurrency 与 build provenance attestation。

## 23.2 Windows 包边界

正式打包入口命令（stage / finalize / verify，参数为规范 `x.y.z`）严格要求版本与 pubspec
完全一致，不接受带 `v`、prerelease 或 build metadata 的形式。stage 复用 GUI 构建生成
per-user Inno Setup 安装器和便携 zip；安装器使用稳定 AppId，默认安装到 LocalAppData，
声明 CloseApplications/RestartApplications。打包输入排除 PDB，包含 LICENSE 与第三方声明。
便携版只供手动分发；便携用户执行应用内升级时进入正式安装版，不对当前运行目录做原地
覆盖。

快捷方式、WER 配置和卸载清理仅管理 anywork；更新资产以 anywork 为前缀，版本序列、
签名信任根与更新清单共同约束发布身份。升级后的用户数据和配置分别遵循
[17](./17-studio-storage.md) 与 [20](./20-config.md) 的版本迁移契约；迁移能力的实现状态
见 [17.7](./17-studio-storage.md#177-迁移实现状态与剩余边界)，安装成功不代表数据迁移已通过验收。

安装器与便携包排除 PDB；Windows 构建必须同时产生独立、带 release version、commit SHA
与 session protocol version 映射的 symbols artifact，收集 runner 和 Rust bridge 的匹配
PDB。symbols artifact 只用于崩溃分析，不作为公开更新资产，也不能被安装器加载。

Authenticode 是可选增强：证书存在时先签主 EXE/自有 DLL，再签最终安装器；缺少证书不
阻塞发布。Minisign/Ed25519 是强制信任根：生产公钥编译进 runtime，私钥与密码只存在于
CI secrets；私钥轮换必须先通过仍受旧密钥信任的应用版本发布新的公钥集合。finalize 只
对最终字节生成 SHA-256、Minisign 签名、校验和文件与更新清单；verify 必须独立复核
文件集、版本、长度、哈希、签名和清单。tag 与 draft Release 由 Release Please 先创建，
但 CI 只有在安装器临时目录静默安装烟测及再次 verify 全部通过后才公开 Release。

## 23.3 更新清单

稳定检查地址固定为仓库 Releases 的 `latest.json`。清单是 camelCase typed JSON：

```json
{
  "schemaVersion": 1,
  "version": "1.2.3",
  "publishedAt": 1770000000,
  "notesUrl": "https://github.com/ZR233/pure-lang/releases/tag/v1.2.3",
  "platforms": {
    "windows-x86_64": {
      "url": "https://github.com/ZR233/pure-lang/releases/download/v1.2.3/anywork-1.2.3-windows-x86_64-setup.exe",
      "size": 123456,
      "sha256": "...",
      "signature": "https://github.com/ZR233/pure-lang/releases/download/v1.2.3/anywork-1.2.3-windows-x86_64-setup.exe.minisig"
    }
  }
}
```

时间戳是 Unix 秒 `i64`。客户端拒绝未知 schema、非稳定 SemVer、同版或降级、非 HTTPS、
非本仓库资源、异常 port/userinfo/query，以及 tag、version、文件名不一致的 URL。清单与
签名下载最多跟随五次重定向，重定向目标仅允许 GitHub Release/CDN HTTPS 主机。

## 23.4 更新状态机与安装

Studio 运行时的 updater owner 保存 canonical UpdaterState：Disabled、Idle、Checking、
UpToDate、Available、Downloading、Verifying、InstallerLaunched、CheckFailed、
InstallFailed；每个状态由独立 payload 承载，update、下载进度和类型化错误不作为平行
可选字段存在。

- `readUpdateState()` 只读 owner cache，不访问网络。
- `checkStudioUpdate()` 使用编译时当前版本并返回 UpToDate 或 Available，Flutter 不传
  currentVersion。
- `installStudioUpdate(expectedRevision, version, eventSink)` 流式下载安装器与签名，校验
  声明长度和 512 MiB 上限，计算 SHA-256，使用内置 Minisign 公钥验签，再启动安装器。

下载使用应用专属缓存目录与 `.partial` 文件：失败删除不完整文件，成功原子重命名；已
验证且与清单完全一致的缓存可复用。并发安装必须拒绝。启动安装器前 bridge 再次确认没有
活动 turn/task；若 runtime 已变忙则保留验证缓存并返回 `runtimeBusy`。空闲时安全关闭
runtime，再使用安装器的 silent/close/restart 参数启动。

检查结果持久化到应用设置键 `observed:studioUpdate:v1`。页面打开只显示 canonical
last-known state，不自动检查。FRB 只公开 typed DTO 和事件（上述三个入口）；安装事件
直接携带完整 canonical updater state；Dart 不接收或解析 raw manifest JSON，也不维护
第二套 install phase。

更新失败不得启动任何二进制：未签名、错误签名、内容篡改、超限、长度不符、URL 越界或
清单降级均属于终止错误；应用保留当前版本并允许用户重试检查或下载。

## 23.5 生产诊断与后台进程

Studio 在用户数据目录的 `anywork/logs` 写入按日滚动 Rust 与 Dart error 日志，panic
marker 和 native dump 写入 `anywork/crashes`。默认 Rust filter 为 `warn`，CLI
`--log-level` 优先于 `RUST_LOG`；启动、每小时与正常关闭清理最后修改时间超过 48 小时的
自有日志和 crash 文件。完整 prompt、context 和工具结果不进入 tracing；日志只记录
root/agent/session 身份、cursor、运行阶段、条目规模、耗时和 outcome（完整合同见
[17](./17-studio-storage.md)）。panic 与 error 使用同步兜底持久化，正常关闭显式 flush。
Windows 构建为当前 exe 配置 WER LocalDumps，并保留 in-process unhandled exception
minidump 兜底；特定栈溢出签名在没有匹配 dump/PDB 时只能报告现象，不能宣称唯一根因。

所有由 GUI 发起的后台 Git、Docker、MCP/LSP、Agent 与终止辅助命令在 Windows 使用
CREATE_NO_WINDOW；其他平台保持既有后台语义（见 [05](./05-conventions.md)）。只有用户
显式打开交互终端或安装器等外部 UI 时允许正常显示窗口。
