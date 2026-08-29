# 22 - SSH 远程开发

## 21.1 边界

Pure 的 SSH 远程开发是本地 runtime 的宿主能力，不是第二套远端 runtime。Flutter 只调用
typed Studio 功能并展示 canonical snapshot；SSH 服务器管理、连接状态机、helper 安装、协议、
重连和远端工具 backend 位于 `pl-core`。`pl-studio-runtime` 只实现 SQLite、可选系统凭据库与
helper 资产路径/缓存等宿主 adapter；当前密码也可只保存在 core 进程的 secret lease 中。

远端 helper 是随 SSH stdio channel 生存的能力代理，只维护 workspace handle 与进程 handle。
它不包含 Thread/Turn、Tool schema、权限、Git/worktree、Skills、LSP 协议、模型、数据库、
Timeline、重试或会话持久化；不监听端口、不 daemonize，也不支持断线后的进程重附着。SSH EOF、
显式 shutdown 或本地取消必须回收 helper 启动的全部进程组。

## 21.2 最小协议

协议以 request/process id 多路复用长度受限的 typed control frame 与原始二进制 chunk。握手严格
协商协议版本、helper build、远端 OS/架构和 capability；未知版本或未知穷尽变体必须失败，不能
降级猜测。

控制面只提供 `hello`、GUI 专用的 `browseDirectories`、`openWorkspace`、`closeWorkspace` 和
`shutdown`。文件面只提供远端事实所必需的 `stat`、`readBytes`、`writeAtomic`、
`listDirectory`、`createDirectory`、`removePath`、`renamePath` 与 `copyPath`。文本解码、glob、
patch/diff、工具 JSON、图片识别和 Skill 解析留在本地 core。helper 必须用远端文件系统事实完成
canonicalize、链接分类与 workspace 越界拒绝。

进程面提供 `spawn`、`writeStdin`、`closeStdin` 和 `terminate`，并发布 `processOutput` 与
`processExit`。输出事件携带 process id、全局单调 sequence、stdout/stderr 分类与原始 bytes；
helper 同时把完整输出写到 core 指定的 workspace capture path。Core 继续拥有超时、模型输出
截断、tool record、Timeline、artifact 与最终 JSON。v1 不提供 PTY、终端面板、端口转发、调试器
或后台任务恢复。

## 21.3 本地工具与 SSH 管理

`pl-core::remote::SshManager` 负责服务器校验、系统 OpenSSH/Askpass、架构探测、已签名 helper
bootstrap、连接状态与自动重连，并返回 `RemoteWorkspaceHost`。host 实现或组合现有
`WorkspaceFileBackend`、`CommandBackend`、Git `ExecutionBackend`、`WorktreeBackend`、
`SkillProvider` 与 LSP process/file backend，再通过同一个 `BuiltinToolInstaller` 注册现有工具。
模型不得看到 `remote_read`、`remote_exec` 等环境专用名字。

文件、Git/worktree、Skills、workspace instructions、图片与 LSP 的环境无关逻辑留在本地。
`apply_patch` 在本地匹配并通过远端原子写提交；Git/worktree 在本地编排命令；LSP client 留在
本地，language server 作为远端可观察进程运行。

连接状态穷尽为 disconnected、connecting、waiting-for-input、ready、reconnecting 与 failed。
断线使当前远端 tool 立即以稳定 `remoteDisconnected` 失败，不透明重放写入或 stdin；core 以
1、2、4、8、15、30 秒退避重连。重连成功后 core 主动重开已知 workspace；下一次 Turn
重新读取远端 Skills，并在 host identity 变化后重新探测 LSP。

## 21.4 凭据、路径与持久化

Pure 调用 PATH 中的系统 OpenSSH，复用 ssh config、known_hosts、ProxyJump、ssh-agent 和用户
显式配置的 agent forwarding。Askpass prompt 由本地 core 分类并经宿主 prompt 端口显示。密码只
存在于系统凭据库或当前进程 secret lease。凭据不得进入 SQLite、DTO、
日志、helper 参数、helper 环境或远端协议；Askpass secret 只注入本地 OpenSSH 子进程环境。
provider token 不得转发。远端 Git 只使用服务器原生配置与凭据。

远端文件 backend 与 `exec.cwd` 始终 confined；`full-access` 不放宽该 backend。该约束仍是 Pure
策略而非 OS shell 沙箱，命令正文拥有 SSH 用户本身的系统权限。GUI 目录浏览是独立宿主功能，
不注册为模型工具。

Studio schema v16 新增非敏感 `ssh_servers` 表与 nullable `projects.ssh_server_id`。本地项目按
`path` 唯一，远端项目按 `(ssh_server_id, path)` 唯一；远端 path 保存 canonical POSIX path。
Session、Turn、Item、Interaction、Task、worktree path 与 tool record 不变。只允许精确 v14
fingerprint 通过单事务从 v15 迁移到 v16；v13/v14 先沿 canonical 链逐级升级，其他未知 schema
继续 fail closed。远端项目启动时不做本地
canonicalize，服务器离线是连接状态，不是项目损坏。

## 21.5 发布与验收

helper 发布 stripped 静态 `aarch64-unknown-linux-musl` 与 `x86_64-unknown-linux-musl` 资产。
xtask 只从 PATH 或标准 Cargo target linker 环境发现交叉链接器，不写死机器路径。Core 先探测
`uname -s/-m`，在本地验证 SHA-256；生产配置还必须提供 Minisign 公钥和相邻签名，验签成功后
才原子上传到版本化远端目录。开发构建生成相邻 SHA-256，并可由显式环境变量、Studio 缓存目录
或仓库 `dist/remote-helper` 提供；远端不需要网络或 Rust 工具链。

`cargo xtask run-gui` 每次先通过 canonical helper 构建入口交叉编译全部支持架构，并把本地
`dist/remote-helper` 路径显式注入 Studio 进程；这条开发路径只信任相邻 SHA-256。
`cargo xtask build-gui` 不复用本地构建产物，而是下载与 Studio 版本一致的 GitHub Release
helper、SHA-256 和 Minisign 签名，验签后复制到 GUI 的 `remote-helper/<target>/` 目录。
正式 publisher 先在 Linux job 构建并签名两种 helper，再将相同字节注入 Windows GUI 打包并
与 GUI 资产一起原子发布，避免构建尚未公开的同版本 Release 时产生循环依赖。runtime 优先从
可执行文件相邻的打包目录发现 helper；带签名的任一生产资产都强制使用编译时内置公钥验签。

确定性门禁使用 direct-stdio helper contract、fake SSH/Askpass、backend parity 与 Flutter Driver。
`root@192.168.100.12` 是 opt-in aarch64 实机验收主机：测试只在唯一
`/tmp/pure-ssh-validation.XXXXXX` workspace 内写入，结束后按记录的精确路径清理并验证无残留
进程；版本化 helper 缓存作为正式产品状态保留。
