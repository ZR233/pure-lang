# 22 - SSH 远程开发

## 21.1 边界

Pure 的 SSH 远程开发是本地 runtime 的宿主能力，不是第二套远端 runtime。Flutter 只调用
typed Studio 功能并展示 canonical snapshot；SSH 服务器管理、连接状态机、helper 安装、协议、
重连和远端工具 backend 位于 `pl-core`。`pl-studio-runtime` 只实现 SQLite、可选系统凭据库与
helper 嵌入资产 adapter；当前密码也可只保存在 core 进程的 secret lease 中。

远端 helper 是随 SSH stdio channel 生存的能力代理，只维护 workspace handle 与进程 handle。
它不包含 Thread/Turn、Tool schema、权限、Git/worktree、Skills、LSP 协议、模型、数据库、
Timeline、重试或会话持久化；不监听端口、不 daemonize，也不支持断线后的进程重附着。SSH EOF、
显式 shutdown 或本地取消必须回收 helper 启动的全部进程组。

## 21.2 最小协议

协议以 request/process id 多路复用长度受限的 typed control frame 与原始二进制 chunk。握手严格
协商协议版本、helper build、远端 OS/架构、实际 shell descriptor 和 capability；未知版本或未知穷尽变体必须失败，不能
降级猜测。

`hello` 返回结构化 shell descriptor（dialect 与已验证的可执行路径）。Linux helper 启动时优先
验证 `/bin/bash`，缺失时验证 `/bin/sh`；不会读取 `$SHELL` 或注入完整环境变量。descriptor 是
远端 helper 的唯一命令启动事实，所有 `exec`、Git 与 LSP 进程都复用它。helper 启动普通远端
进程时继承当前环境，并在请求没有显式覆盖 `PATH` 时，把该用户的 `$HOME/.cargo/bin` 与
`$HOME/.local/bin` 放到继承 `PATH` 前；这只补齐非 login SSH channel 常见的用户工具目录，不读取
shell profile，也不替模型改写命令。调用方显式提供的 `PATH` 始终原样优先。

控制面只提供 `hello`、GUI 专用的 `browseDirectories`、`openWorkspace`、`closeWorkspace` 和
`shutdown`。文件面只提供远端事实所必需的 `stat`、`readBytes`、`writeAtomic`、
`listDirectory`、`createDirectory`、`removePath`、`renamePath` 与 `copyPath`。文本解码、glob、
patch/diff、工具 JSON、图片识别和 Skill 解析留在本地 core。helper 必须用远端文件系统事实完成
canonicalize、链接分类与 workspace 越界拒绝。

进程面提供 `spawn`、`writeStdin`、`closeStdin` 和 `terminate`，并发布 `processOutput` 与
`processExit`。输出事件携带 process id、全局单调 sequence、stdout/stderr 分类与原始 bytes；
helper 同时把完整输出写到 core 指定的 workspace capture path。Core 继续拥有超时、模型输出
截断、tool record、Timeline、artifact 与最终 JSON。v2 不提供 PTY、终端面板、端口转发、调试器
或后台任务恢复。

`processId` 是 opaque token，不允许调用方解析其局部序号。core 进程内所有
`CommandProcessManager` 共用一个原子单调分配序列，因此 manager 重建或多个 Agent 共享同一 helper
连接时也不会复用 id；在单个 helper 连接生命周期内同一 id 最多成功 spawn 一次。helper 在任何异步
进程准备前把 id 原子登记为 `Starting` reservation，成功后转换为 live handle，所有失败路径都释放
reservation。并发同 id 请求不得通过检查后相互覆盖，关闭连接仍回收全部 live 进程组。

## 21.3 本地工具与 SSH 管理

`pl-core::remote::SshManager` 负责服务器校验、系统 OpenSSH/Askpass、架构探测、内嵌 helper
bootstrap、握手 shell descriptor、连接状态与自动重连，并返回带有 `ExecutionEnvironment` 的
`RemoteWorkspaceHost`。host 实现或组合现有
`WorkspaceFileBackend`、`CommandBackend`、Git `ExecutionBackend`、`WorktreeBackend`、
`SkillProvider` 与 LSP process/file backend，再通过同一个 `BuiltinToolInstaller` 注册现有工具。
模型不得看到 `remote_read`、`remote_exec` 等环境专用名字。

文件、Git/worktree、Skills、workspace instructions、图片与 LSP 的环境无关逻辑留在本地。
`apply_patch` 在本地匹配并通过远端原子写提交；Git/worktree 在本地编排命令；LSP client 留在
本地，language server 作为远端可观察进程运行。
workspace instructions 由远端 file backend 读取后以已加载文档集合交给指令组装器，保留远端
来源路径；远程路径不得再次交给本地文件系统做目录或文件检查。

本地与远端 prompt 使用同一份 `ExecutionEnvironment`：Platform developer 段声明 transport、目标
OS、shell dialect 和路径，并按该 dialect 生成命令语法。shell descriptor 只缓存在当前连接和
workspace host；断线自动重连完成新的 hello 后替换旧 descriptor。环境变化会改变动态
`globalDeveloper` 内容及其 prompt cache generation。

SSH 连接、平台探测、helper 上传和协议握手都通过本地后台进程工厂启动系统 OpenSSH：Windows
使用 `CREATE_NO_WINDOW`，不得弹出额外命令行窗口；Unix 使用独立进程组并在丢弃时回收进程。
SSH 通道只承载标准输入输出协议，因此固定关闭伪终端（`-T`）与 X11 转发（`-x`），不打开
交互式终端或图形会话。

连接状态穷尽为 disconnected、connecting、waiting-for-input、ready、reconnecting 与 failed。
断线使当前远端 tool 立即以稳定 `remoteDisconnected` 失败，不透明重放写入或 stdin；core 以
1、2、4、8、15、30 秒退避重连。重连成功后 core 主动重开已知 workspace、重新取得 shell
descriptor；下一次 Turn
重新读取远端 Skills，并在 host identity 变化后重新探测 LSP。

SSH workspace 的 Skill catalog 由一个共享 registry 组合构成：远端 provider 贡献 Project 源，
默认项目目录为远端 workspace 下的 `.agents/skills`；本地配置用户目录、用户主目录
`.agents/skills`、Studio 物化的系统技能目录和显式 external 目录以只读来源并行注册，顺序与本地
workspace 一致。Thread Mode 独立从本地内存注册表捕获，不进入远端或本地 Skill 发现。Turn
执行与 Settings 的显式技能发现共用同一组合，因此设置页技能目录展示远端 Project 技能与本地
系统/用户/external 技能，且激活 fingerprint 包含 Skills 配置指纹；配置变化后的下一次激活会重新
发现。

## 21.4 凭据、路径与持久化

Pure 调用 PATH 中的系统 OpenSSH，复用 ssh config、known_hosts、ProxyJump、ssh-agent 和用户
显式配置的 agent forwarding。Askpass prompt 由本地 core 分类并经宿主 prompt 端口显示。密码只
存在于系统凭据库或当前进程 secret lease。凭据不得进入 SQLite、DTO、
日志、helper 参数、helper 环境或远端协议；Askpass secret 只注入本地 OpenSSH 子进程环境。
Askpass 脚本写入和 chmod 完成后必须关闭可写文件句柄，再以自动清理的路径 lease 覆盖整个
OpenSSH 子进程生命周期，避免 Unix 首次执行返回 `Text file busy`。
provider token 不得转发。远端 Git 只使用服务器原生配置与凭据。

shell descriptor 不是 login shell 配置，也不携带完整环境变量；它只描述 Pure 实际启动命令所用
的解释器与已验证路径。

远端文件 backend 与 `exec.cwd` 始终 confined；`full-access` 不放宽该 backend。冻结为 directory
Profile 的 `writablePaths` 也独立于 Permission Mode：core 在所有远端内置 mutation 路径上先按
workspace-relative POSIX 路径执行同一策略，包括 `write_file`、创建、删除、复制目标、移动源/目标
以及 `apply_patch` 的写入和删除；读取不受该列表限制。helper 继续负责 canonical workspace 与
symlink 越界防护，目录策略仍只是 Pure 内置工具边界，不能宣称为 shell/Git/MCP 的 OS 沙箱。
SSH `exec.cwd` 只接受
workspace-relative POSIX 路径，根目录使用 `.`，不得传远端 canonical root；绝对路径返回
`exec.cwd must be workspace-relative for SSH; use "." for the workspace root`，`..` 仍以 workspace
escape 错误拒绝。runtime 不把绝对路径猜成 `.` 或静默截断前缀。该约束仍是 Pure
策略而非 OS shell 沙箱，命令正文拥有 SSH 用户本身的系统权限。GUI 目录浏览是独立宿主功能，
不注册为模型工具。

Studio schema v16 新增非敏感 `ssh_servers` 表与 nullable `projects.ssh_server_id`。本地项目按
`path` 唯一，远端项目按 `(ssh_server_id, path)` 唯一；远端 path 保存 canonical POSIX path。
Session、Turn、Item、Interaction、working state 与 tool record 的 wire 语义不因远程 host 改变。
Studio 数据库当前 schema v18 采用破坏性重建，不再维护旧 Task/worktree 表或逐版本迁移链；其他
未知 schema 继续 fail closed。远端项目启动时不做本地
canonicalize，服务器离线是连接状态，不是项目损坏。

## 21.5 发布与验收

helper 构建 stripped 静态 `aarch64-unknown-linux-musl` 与 `x86_64-unknown-linux-musl` 资产。
xtask 优先使用 `PURE_REMOTE_HELPER_BUILDER` 指定的构建器；未指定时若 PATH 中存在
`cargo-zigbuild`，且 Zig 位于 PATH 或 `CARGO_ZIGBUILD_ZIG_PATH` 指定的文件，自动使用
`cargo zigbuild`，否则从 PATH 或标准 Cargo target linker 环境发现交叉链接器，不写死机器路径。
两种 helper 在 GUI 构建 Rust bridge 时以 zstd 压缩资产嵌入同一个应用二进制，不作为独立安装文件
或网络资产。
Core 先探测 `uname -s/-m`，再请求宿主 adapter 解压唯一匹配的 helper bytes，并按内容摘要上传到
版本化远端目录；同一摘要已有可执行文件时直接复用，不重复传输。未匹配架构保持压缩状态，也不
产生本地解压文件。远端不需要网络或 Rust 工具链。

`cargo xtask run-gui` 与普通 `cargo xtask build-gui` 都先通过 canonical helper 构建入口交叉编译
全部支持架构，再构建带 `embedded-remote-helpers` feature 的 Rust bridge。正式 publisher 可在
Linux job 构建同一提交的两种 helper，并把 CI 内部构建产物交给 Windows GUI job 嵌入；这些临时
产物不得成为 GitHub Release 资产。安装器和便携包仅通过 bridge 二进制间接携带压缩 helper，SSH
连接期间不访问 GitHub，也不维护额外 helper 下载缓存。

确定性门禁使用 direct-stdio helper contract、fake SSH/Askpass、backend parity 与 Flutter Driver。
`root@192.168.100.12` 是 opt-in aarch64 实机验收主机：测试只在唯一
`/tmp/pure-ssh-validation.XXXXXX` workspace 内写入，结束后按记录的精确路径清理并验证无残留
进程；远端版本化 helper 目录作为正式产品状态保留。

子代理实机验收复用 `cargo xtask verify-subagents --live --gui`，通过
`PURE_SUBAGENTS_SSH_SERVER`、`PURE_SUBAGENTS_SSH_USERNAME` 和当前进程的一次性
`PURE_SUBAGENTS_SSH_PASSWORD` 显式启用。harness 不把 password 放入 argv 或日志；创建远端 fixture
时只把它传给本地 OpenSSH 的 Askpass 环境，启动 GUI 前从 GUI 进程环境移除，再仅向 Driver 进程传递，
由 Driver 通过可见密码框交给产品 secret lease。远端 fixture 位于连接用户 home 下的唯一目录，必须
包含初始 commit；验收后核对 worktree branch/path 已清理、最终 marker 与 Git 状态，再删除该精确目录。
当 password 与任何持久化非 secret 字段都不相同时，artifact 与隔离 Studio home 在结束前扫描
password 字节，发现泄漏立即失败；若 password 与 username 等合法字段相同，字节扫描无法区分来源，
验收必须改用 SQLite typed schema/row 检查，确认只存在非 secret 列且 `auth_json` 只记录认证种类。
