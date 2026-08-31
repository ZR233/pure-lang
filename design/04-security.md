# 04 - 安全边界（方案乙）

## 4.1 执行默认

方案乙默认权限模式固定为：

- `PermissionMode::RequestApproval`

这是破坏性升级后的默认行为，不再根据旧 UI 选择分支切换默认值，也不保留独立的
`ToolApprovalPolicy`。手动审批能力由 `PermissionMode::RequestApproval` 在 workspace
外访问时触发，不是第二套控制面。

Pure v1 的权限模式是本地策略层，不是 OS 沙箱、网络沙箱或系统级进程隔离。策略层只决定 Pure 已注册工具是否放行、请求用户审批、请求 AI reviewer 审批或拒绝；直接放行也不会绕过工具自身 schema 校验、工作区写锁、超时、输出截断和 timeline 记录。

权限模式：

- `request-approval`：默认模式。workspace 内文件读写、`apply_patch`、项目 skill 写入和 workspace cwd 的 `exec` 直接放行；工具请求访问 workspace 外路径或 workspace 外 cwd 时请求用户批准。
- `auto-review`：workspace 内行为同 `request-approval`；工具请求访问 workspace 外路径或 workspace 外 cwd 时交给 reviewer 模型审批。reviewer 只返回是否批准，不执行工具。
- `full-access`：所有已注册工具在策略层直接放行；本地文件 backend 可解析 workspace 外路径，`exec.cwd` 可指向 workspace 外已存在目录。

execution profile 的工具 effect 白名单优先于权限模式。当前 Studio root 与 child policy 允许其普通
effect，再由 Permission Mode、workspace assignment 和各工具 schema 共同约束实际调用。Task Mode 中
“root 只亲自修改设计与整合代码”、explorer/reviewer 只读等属于 Mode/Profile 提示词的合作式角色合同，
不是按 workflow stage 动态切换的硬权限，也不能对抗 shell、Git 或 MCP 的命令正文。directory child 的
`writablePaths` 只约束 Pure 内置 mutation；worktree child 的 `Confined` boundary 则始终不能被权限模式
放宽。GUI、工具描述和固定上下文必须如实区分这两类边界。

只读 reviewer 可以调用 `report_progress` 追加协作层的结构化审查报告；这不会修改项目 workspace、Git
或外部系统，不属于实现写入。验收与 root 编排只能把绑定到 reviewer `agentId` 的 canonical
submission 作为 approval/finding 证据，不能把 root 转述、任意 session 文本或空 submission 当作授权。

## 4.2 分层边界

安全边界按端口-适配器落位：

- `pure-studio`：输入收集、事件展示、命令调用
- `pl-studio-runtime::StudioRuntime` / `StudioHost`：产品策略编译与资源约束
- `pl-core::AgentRuntime` / `TurnEngine`：执行策略校验、actor 状态与通用 turn 约束
- `pl-core::agent_runtime` host traits：repository、turn factory、lifecycle 与 event 端口
- `pl-core::tool`、`pl-core::mcp`：通用工具执行与协议能力
- `pl-studio-runtime`：Studio 配置文件、SQLite、事件落盘和产品资源生命周期
- `pl-model`：仅访问已配置 API

`pl-protocol` 只承载类型，不持有策略实现。

## 4.3 文件与工具约束

文件工具默认遵守工作区边界：

- 工具输入可以是 workspace-relative 路径或绝对路径；相对路径按 `workspace_root` 解析，不依赖进程 cwd
- 执行前统一解析为规范化绝对路径，并复用同一解析结果做审批预判和实际执行
- 解析后路径必须位于 `workspace_root` 内
- `WorkspaceOnly` 拒绝 `..`、Windows drive-relative 路径、越界绝对路径、越界 UNC / verbatim 路径和符号链接越界
- 二进制读取返回明确错误
- `apply_patch` 直接改文件，不经 shell 转发
- 符号链接目标不可确认或越界时拒绝

当用户显式选择 `full-access` 时，Pure 放宽本地文件 backend 和 `exec.cwd` 的 workspace 边界：绝对路径和 `..` 可以解析到 workspace 外。该模式仍要求目标自身或其最近存在父目录可解析，只影响 Pure 工具的本地策略，不代表系统级完全隔离或提权；容器或远程 backend 可以继续拒绝越界路径。

SSH 远端 backend 始终拒绝 workspace 越界和符号链接入口，`full-access` 不放宽它。远端 helper
依据远端文件系统事实做 canonicalize；但 `exec` 仍只约束 cwd，不分析命令正文，也不是 OS
沙箱。远端命令拥有 SSH 用户权限，这一事实必须在连接与权限 UI 中可见。

## 4.4 凭据暴露面

`config.toml` 仍为本地凭据来源，但方案乙收紧暴露面：

- UI 默认不回显完整 token
- 日志与事件 payload 不输出 token
- 错误信息禁止拼接敏感字段

SSH password 与 Askpass 回答只存在于系统凭据库或当前进程 secret lease，不进入
SQLite、transport DTO、日志、helper argv/env 或远端协议；Askpass secret 只进入本地 OpenSSH
子进程环境。系统 OpenSSH 继续使用用户的
known_hosts、ssh config 与 agent；本地 provider token 不得通过 SSH 转发，远端 Git 使用远端
原生凭据或用户显式配置的 agent forwarding。

## 4.5 桌面 WebView 边界

Flutter 桌面端的安全边界集中在本地工具策略、配置凭据和 Flutter/FRB 桥接。

桌面 UI 目标：

- 不引入远程脚本执行入口。
- 只通过 `pl-studio-bridge` 调用本地 runtime。
- 文件选择、路径访问和工具执行仍由 `pl-core` 策略校验。

## 4.6 本机 HTTP 边界

`pl-studio-server` 不提供远程鉴权能力，因此只允许绑定 loopback 地址。请求 Host 必须解析为
loopback IP 或 `localhost`；带 `Origin` 的请求必须与当前 Host 同源，其他 Origin 一律拒绝。
server 不发送 CORS 许可头，不接受把 wildcard、LAN 或公网地址作为 listen 参数。

HTTP 与 FRB 统一返回脱敏 `StudioError { code, message, retryable, correlationId, details }`；日志可
按 correlation ID 记录内部诊断，但响应不得包含 token、配置正文、绝对私有路径、provider 原始
错误或数据库语句。OpenAPI 与 Swagger UI 是静态协议展示，不启动第二个 runtime。

## 4.7 数据切换安全

Studio 运行期只读写 `studio.sqlite`；配置只接受 `config.toml` schema 17，provider API token
保存在系统凭据库。启动发现不兼容配置时不迁移、不导入其中的凭据，先逐字备份到配置目录中的
唯一 `.rejected.<timestamp>.bak` 文件，再原子替换为当前初始配置；系统凭据库保持独立，替换后
只按初始 provider id 注入已有凭据。配置文件、备份或凭据库 IO 失败不得触发替换，运行期显式
重载也不得自动恢复。
数据库版本、结构 fingerprint 或完整性不兼容时，不迁移、不归档、不导入：关闭检查连接后只
删除精确 canonical 数据库及其 `-wal/-shm`，再创建空库。删除或重建失败必须停止启动；不得
扫描或修改用户 workspace、Git repository 或配置目录中的其他数据。破坏性升级可以删除精确
canonical 数据库及 Studio 自有 `attachments/`，但必须拒绝符号链接/reparse point 且不得跟随到
目录外。完整合同见
`19-studio-storage-and-diagnostics.md`。
