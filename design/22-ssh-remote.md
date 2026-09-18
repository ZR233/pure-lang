# 22 - SSH 远程开发

## 22.1 边界

Pure 的 SSH 远程开发是本地 runtime 的宿主能力，不是第二套远端 runtime。Flutter 只调用
typed Studio 功能并展示 canonical snapshot；SSH 服务器管理、连接状态机、helper 安装、
协议、重连和远端工具 backend 位于 pl-tool；pl-studio-runtime 只实现 SQLite、可选系统
凭据库与 helper 嵌入资产 adapter。SSH 服务器配置不是产品数据，唯一事实源是用户
`~/.ssh/config`（Windows 为 `%USERPROFILE%\.ssh\config`）。

远端 helper 是随 SSH stdio channel 生存的能力代理，只维护 workspace handle 与进程 handle。
它不包含 Thread/Turn、Tool schema、权限、Git/worktree、Skills、LSP 协议、模型、数据库、
Timeline、重试或会话持久化；不监听端口、不 daemonize，也不支持断线后的进程重附着。SSH
EOF、显式 shutdown 或本地取消必须回收 helper 启动的全部进程组。

## 22.2 最小协议

协议以 request/process id 多路复用长度受限的 typed control frame 与原始二进制 chunk。
握手严格协商协议版本、helper build、远端 OS/架构、实际 shell descriptor 和 capability；
未知版本或未知穷尽变体必须失败，不能降级猜测。

`hello` 返回结构化 shell descriptor（dialect 与已验证的可执行路径）。Linux helper 启动时
优先验证 `/bin/bash`，缺失时验证 `/bin/sh`；descriptor 只描述实际命令解释器，与用户环境
加载 shell 分开。每次 SSH 连接先按用户账户信息、有效 `$SHELL`、`/bin/sh` 的顺序选择环境
加载 shell，未知解释器明确失败。Bash 以交互非登录模式读取 `.bashrc`，Zsh/Fish 使用交互
登录模式，sh 使用登录模式读取 profile。配置仅由对应解释器执行，不跨 shell source，不
要求 PTY；只继承 exported 环境，不继承 alias、函数或依赖真实终端的状态。采集受物理进程
监督器管理，stdin 为 `/dev/null`，启动输出与 SSH 协议隔离；15 秒超时、提前退出或无效
快照导致连接失败并回收进程树，诊断不包含环境值。

完整环境快照仅在远端连接内存中存在，通过私有 Unix socket 采集，helper 在该环境下启动
服务，不依赖远端 Python 或 `env -0`。普通命令、Git 和 LSP 统一继承此环境（包括 unset），
请求显式 env 最后覆盖；普通进程不重复执行继承的启动钩子，显式请求可覆盖；PATH 完全
尊重远端配置，不额外补入固定目录；同一连接不重复加载配置，重连重新采集。本地 process
worker 的环境策略不受影响。

控制面只提供 `hello`、GUI 专用的 `browseDirectories`、`openWorkspace`、`closeWorkspace`
和 `shutdown`。文件面只提供远端事实所必需的 `stat`、`readBytes`、`writeAtomic`、
`listDirectory`、`createDirectory`、`removePath`、`renamePath` 与 `copyPath`；文本解码、
glob、patch/diff、工具 JSON、图片识别和 Skill 解析留在本地 core。helper 必须用远端文件
系统事实完成 canonicalize、链接分类与 workspace 越界拒绝。进程面提供 `spawn`、
`writeStdin`、`closeStdin` 和 `terminate`，并发布 `processOutput` 与 `processExit`：输出
事件携带 process id、全局单调 sequence、stdout/stderr 分类与原始 bytes；helper 同时把
完整输出写到 core 指定的 workspace capture path。core 继续拥有超时、模型输出截断、tool
record、Timeline、artifact 与最终 JSON。不提供 PTY、终端面板、端口转发、调试器或后台
任务恢复。

`processId` 是 opaque token，不允许调用方解析其局部序号。core 进程内所有命令进程管理器
共用一个原子单调分配序列，因此 manager 重建或多个 Agent 共享同一 helper 连接时也不会
复用 id；在单个 helper 连接生命周期内同一 id 最多成功 spawn 一次。helper 在任何异步进程
准备前把 id 原子登记为 Starting reservation，成功后转换为 live handle，所有失败路径都
释放 reservation；并发同 id 请求不得通过检查后相互覆盖，关闭连接仍回收全部 live 进程组。

## 22.3 本地工具与 SSH 管理

pl-tool 的 SSH 管理器负责服务器校验、系统 OpenSSH、架构探测、内嵌 helper
bootstrap、握手 shell descriptor、连接状态与自动重连，并返回带执行环境的远端 workspace
host；host 实现或组合现有文件、命令、Git、worktree、Skill 与 LSP backend，再经统一安装
入口注册现有工具。模型不得看到 `remote_read`、`remote_exec` 等环境专用名字。

SSH 服务器列表读取解析 `~/.ssh/config`，保存与删除以带标记的 anywork 管理块原子写回该
文件，未触及内容逐字节保留（含行尾风格）。管理块使用
`# BEGIN anywork server: <alias>` / `# END anywork server: <alias>` 包裹
Host/HostName/Port/User/IdentityFile；Host 别名即服务器身份，创建后不可改名，别名必须是
非空单 token 且不含通配或否定字符。解析遵循 ssh 的首次匹配语义（每个关键字取块内首次
出现值，HostName 缺省回退别名），含通配、多模式或 Include 的条目不进入可选列表；用户
手写条目只读展示，编辑与删除仅对管理块开放。新建别名与文件中既有 Host 冲突时分配不冲突
后缀；同别名管理块与待写内容一致时视为同一事实，重试安全。内部 ssh 调用只传 Host 别名，
端口、用户、私钥与代理全部由 ssh config 解析，运行时不维护第二份连接参数。

文件、Git/worktree、Skills、workspace instructions、图片与 LSP 的环境无关逻辑留在本地。
工具读取的远端图片由本地媒体宿主归档；Timeline 的文字入口与行内展开只读取该 Thread
持久化引用的归档资源，不在展开时重新连接 SSH 或读取原始远端文件；源文件变更、删除或
SSH 断线不改变已归档图片，未知或未被该 Thread 引用的资源必须拒绝。`apply_patch` 在本地
匹配并通过远端原子写提交；Git/worktree 在本地编排命令；LSP client 留在本地，language
server 作为远端可观察进程运行。workspace instructions 由远端 file backend 读取后以已加载
文档集合交给指令组装器，保留远端来源路径；远程路径不得再次交给本地文件系统做目录或
文件检查。

SSH 项目与本地项目一样提供会话工作区模式：`local` 使用远端 canonical Project 目录，
`worktree` 在远端仓库按创建时解析的 `HEAD` 创建
`<repo>/.anywork/worktrees/<root-thread-id>/session`，lease 记录 `ssh_alias`，激活时按
identity 匹配的 `active` lease 把该远端路径绑定为会话工作区根。远端 workspace handle 根即
会话工作区根，工具层接受会话工作区根与 canonical Project 目录分离，并按 workspace-relative
POSIX 路径执行同一内置目录写策略。远端 worktree backend 以解析出的仓库根为基准，Project
目录是仓库子目录时同样成立。远端 worktree 的恢复、preview 与显式清理走同一 lease 状态机，
连接可用时经远端 backend；SSH 离线时按既有语义保留现场并给出诊断，不推断 ownership。启动期
「未注册 worktree」审计只覆盖本地文件系统与本地 git，不为远端项目打开 SSH 连接；远端资源由
durable lease 覆盖。

远端路径在任何跨端边界上都是 POSIX 字符串：远端 lease 的 `repository_root` 与 `path`、会话
工作区根、workspace 打开参数与身份比较，以及传给远端 git/helper 的路径参数都以 POSIX 形式
表示。客户端宿主形态（包括 Windows 的路径分隔符与驱动器前缀）只在本地文件系统上使用，不得
跨端出现；跨端前统一归一化，并保证同一个远端路径在目录写操作与 git 路径参数上使用同一结果，
避免物理 worktree 落在与 lease 记录不一致的位置。

本地与远端 prompt 使用同一份执行环境：Platform developer 段声明 transport、目标 OS、
shell dialect 和路径，并按该 dialect 生成命令语法。shell descriptor 只缓存在当前连接和
workspace host；断线自动重连完成新的 hello 后替换旧 descriptor；环境变化会改变动态
developer 内容及其 prompt cache generation。

SSH 连接、平台探测、helper 上传和协议握手都通过统一后台进程工厂启动系统 OpenSSH：Windows
不弹出额外命令行窗口；Unix 使用独立进程组并在丢弃时回收进程。SSH 通道只承载标准输入
输出协议，因此固定关闭伪终端与 X11 转发，不打开交互式终端或图形会话；SSH 以 BatchMode
运行，只接受 ssh-agent 与密钥等非交互认证，不注入密码或 askpass。

连接状态穷尽为 disconnected、connecting、ready、reconnecting 与 failed。SSH 建连超时为 15 秒，存活探测每 15 秒一次、连续三次无响应后断开；平台与资产
探测最多等待 30 秒，资产上传最多等待 120 秒，helper 握手最多等待 25 秒，目录重开最多
等待 15 秒。超时关闭并回收所属 SSH 进程；关闭旧连接先等待 5 秒、再终止并最多等待 5
秒；失败保留回收责任。传输保存首次失败阶段与有界诊断，持续排空进程错误输出，密码不
进入诊断。单个本地输出接收端关闭只丢弃该端的迟到输出，不使同一连接上的其他进程和
文件操作断线；真实协议错误仍关闭连接。断线使当前远端工具立即以稳定
`remoteDisconnected` 失败，不透明重放写入或 stdin；core 以 1、2、4、8、15、30 秒退避
重连。重连成功后 core 主动重开已知 workspace、重新取得 shell descriptor；手动及自动
重连成功均通知原有 root/child 会话刷新工具绑定，无需新建会话。刷新依据连接身份判断
租约是否仍有效，不以服务器名称、目录或可重复的 workspace id 判断。原工作目录重新打开
失败时报告失败，禁止继续使用旧连接或切换到其他目录；下一次 Turn 重新读取远端 Skills，
并在 host identity 变化后重新探测 LSP。

SSH workspace 的 Skill catalog 由一个共享 registry 组合构成：远端 provider 贡献
Project 源，默认项目目录为远端 workspace 下的 `.agents/skills`；本地配置用户目录、用户
主目录 `.agents/skills`、Studio 物化的系统技能目录和显式 external 目录以只读来源并行
注册，顺序与本地 workspace 一致（目录合同见 [10](./10-skills.md)）。Thread Mode 独立
从本地内存注册表捕获，不进入远端或本地 Skill 发现。Turn 执行与 Settings 的显式技能
发现共用同一组合，因此设置页技能目录展示远端 Project 技能与本地系统/用户/external
技能，且激活 fingerprint 包含 Skills 配置指纹；配置变化后的下一次激活会重新发现。

## 22.4 凭据、路径与持久化

Pure 调用 PATH 中的系统 OpenSSH，复用 ssh config、known_hosts、ProxyJump、ssh-agent 和
用户显式配置的 agent forwarding。认证仅支持 ssh-agent 与密钥；凭据不得进入 SQLite、DTO、
日志、helper 参数、helper 环境或远端协议，也不维护进程内密码 lease 或 askpass 注入。
首次主机身份核验沿用用户 ssh 的 known_hosts 语义，不自动信任新主机或已变更的主机密钥。
provider token 不得转发；远端 Git 只使用服务器原生配置与凭据。shell descriptor 不是
login shell 配置，也不携带完整环境变量。

远端文件 backend 与 `exec.cwd` 始终 confined，`full-access` 不放宽该 backend（见
[04](./04-security.md)）。冻结为 directory Profile 的 `writablePaths` 也独立于权限模式：
core 在所有远端内置 mutation 路径上先按 workspace-relative POSIX 路径执行同一策略，包括
`write_file`、创建、删除、复制目标、移动源/目标以及 `apply_patch` 的写入和删除；读取
不受该列表限制。helper 继续负责 canonical workspace 与 symlink 越界防护；目录策略仍只是
Pure 内置工具边界，不能宣称为 shell/Git/MCP 的 OS 沙箱。SSH `exec.cwd` 只接受
workspace-relative POSIX 路径，根目录使用 `.`，不得传远端 canonical root；绝对路径与
`..` 分别返回可操作且不同的稳定错误，runtime 不把绝对路径猜成 `.` 或静默截断前缀。该
约束仍是 Pure 策略而非 OS shell 沙箱，命令正文拥有 SSH 用户本身的系统权限。GUI 目录
浏览是独立宿主功能，不注册为模型工具。

Studio 的"打开远程项目"窗口绑定 `~/.ssh/config` 中的一个 Host 别名，并同时提供目录浏览与
路径输入；输入只接受该服务器上的绝对 POSIX 目录路径，不解析 `~`、相对路径、`ssh://` URI
或 `user@host:path`，也不隐式创建服务器配置。初次进入浏览远端默认目录，之后的向上导航、
子目录导航和手工路径都先通过 Studio 的 `browseRemoteDirectories` 取得 canonical listing；
只有与当前输入一致、已经验证的 canonical path 才能提交给 `openRemoteProject`，再由远端
协议的 `openWorkspace` 打开。后端 product snapshot 只拥有 canonical Project 目录，不拥有
Flutter 当前选择；Studio controller 只有在重新读取的目录中找到与打开结果同 id、同 SSH
别名、同 canonical path 的 Project，并由显式 selection intent 采用它之后才报告成功；
拒绝新工作或 canonical Project 身份未被采用都不是成功，窗口不得因此关闭。浏览与打开
操作必须串行化：pending 期间所有调用入口都拒绝重复或冲突请求，不能只依赖下一帧的按钮
禁用状态；打开期间窗口不可通过取消、遮罩或系统返回动作关闭；浏览或打开失败必须保留
窗口与用户输入，让用户在原上下文中修正并重试。目录没有子目录时显示明确空态，但仍
允许打开已验证的当前目录。窗口在窄视口与放大文本下仍须保持路径输入和主要操作可达；
图标导航必须提供可本地化的可访问名称。

Studio schema 不保存 SSH 服务器表；远端项目以可空 `projects.ssh_alias` 引用
`~/.ssh/config` 的 Host 别名：本地项目按 `path` 唯一，远端项目按 `(ssh_alias, path)`
唯一，远端 path 保存 canonical POSIX path。数据库版本演进遵循统一迁移契约（见
[17](./17-studio-storage.md)）：v20 及更早的 `ssh_servers` 行在升级为 v21 时由启动
协调器先备份产品库，再把每行迁移为 `~/.ssh/config` 管理块（别名取原 name，冲突时分配
后缀；密码认证行不携带 IdentityFile，迁移后需用户自行补充密钥或 agent），同一事务内把
`projects.ssh_server_id` 重写为最终别名并删除旧表；文件写入与别名分配幂等，中断后可安全
重试。Session、Turn、Item、Interaction、working state 与 tool record 的 wire 语义不因
远程 host 改变。远端项目启动时不做本地 canonicalize，服务器离线是连接状态，不是项目损坏。

## 22.5 helper 资产与嵌入

helper 构建为 stripped 静态 musl 资产（aarch64 与 x86_64 两种 Linux 架构），在 GUI 构建
Rust bridge 时以 zstd 压缩资产嵌入同一个应用二进制，不作为独立安装文件或网络资产；
helper target 由 `uname -s/-m` 穷尽映射，未知平台明确失败。core 先探测架构，再请求宿主
adapter 解压唯一匹配的 helper bytes，并按内容摘要上传到版本化远端目录；同一摘要已有
可执行文件时直接复用，不重复传输；未匹配架构保持压缩状态，也不产生本地解压文件；远端
不需要网络或 Rust 工具链。交叉编译入口由仓库构建工具统一提供（构建器选择与环境发现以
工具实现为准），正式发布流程可在 Linux 侧构建同一提交的两种 helper 并作为 CI 内部产物
交给 Windows GUI job 嵌入，不得进入正式 Release 文件集。

预编译 helper 除 SHA-256 外必须携带目标架构与 worker 协议版本元数据，缺失或不匹配时
拒绝并提示重建。宿主架构复用 worker Ready 协议做实际握手，其他架构仅校验静态元数据；
探针拥有并回收子进程，退出、版本不符与超时分别报告，有界保留 stderr 和执行路径。同一
物化 helper 的启动失败在当前 runtime 内保持不可用，重启后重新准入，不延长原有超时。
