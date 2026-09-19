# 04 - 安全边界

## 4.1 权限模式

权限模式是本地策略层，不是 OS 沙箱、网络沙箱或系统级进程隔离。策略层只决定已注册工具是否放行、
请求用户审批、请求 AI reviewer 审批或拒绝；直接放行也不会绕过工具自身 schema 校验、工作区写锁、
超时、输出截断与 timeline 记录。本文是三值权限语义的唯一权威源。

- `request-approval`：默认模式。workspace 内文件读写、`apply_patch`、项目 skill 写入和 workspace
  cwd 的 `exec` 直接放行；访问 workspace 外路径或 workspace 外 cwd 时请求用户批准。
- `auto-review`：workspace 内行为同 request-approval；访问 workspace 外时交给 reviewer 模型审批。
  reviewer 只返回是否批准，不执行工具。
- `full-access`：所有已注册工具在策略层直接放行；本地文件 backend 可解析 workspace 外路径，
  `exec.cwd` 可指向 workspace 外已存在目录。

不保留独立的工具审批策略开关：手动审批由 request-approval 在越界访问时触发，不是第二套控制面。

execution profile 的工具 effect 白名单优先于权限模式。Studio root 与 child policy 允许其普通
effect，再由权限模式、workspace assignment 与各工具 schema 共同约束实际调用。Task Mode 中
"root 只亲自修改设计与整合代码"、explorer/reviewer 只读等属于 Mode/Profile 提示词的合作式角色
合同，不是按 workflow 状态动态切换的硬权限，也不能对抗 shell、Git 或 MCP 的命令正文。directory
child 的 writablePaths 只约束内置 mutation 工具；worktree child 与会话自身 worktree
（`ThreadWorkspaceMode = worktree`）的 confined 边界始终不能被权限模式放宽，`full-access`
也不会为该会话放开 worktree 之外的 host 路径或命令 cwd。GUI、工具描述与固定上下文必须
如实区分这些边界。

只读 reviewer 通过自然 final 或 `finish_turn({message})` 结束本轮并汇报审查结论；这不修改
项目 workspace、Git 或外部系统。root 阅读绑定到真实 reviewer、Turn 与 journal 终态水位的
完整报告，结合产物与验证判断是否接受，不从字符串口令推断批准。

## 4.2 分层边界

安全边界按端口-适配器落位：

- anywork（Flutter UI）：输入收集、事件展示、命令调用。
- pl-studio-runtime：产品策略编译与资源约束、配置文件、SQLite、事件落盘与产品资源生命周期；
  Studio 线程装配层负责配置、资源、生命周期与产品事件装配。
- pl-core：执行策略校验、actor 状态与通用 turn 约束。
- pl-tool：通用工具执行与 MCP 协议能力。
- pl-model：仅访问已配置 API。
- pl-protocol：只承载类型，不持有策略实现。

## 4.3 文件与工具约束

文件工具默认遵守工作区边界：

- 本地文件、patch、LSP 与 `exec.cwd` 输入可以是 workspace-relative 或绝对路径；相对路径按
  workspace root 解析，不依赖进程 cwd。SSH `exec.cwd` 是例外，只接受 workspace-relative 路径，
  workspace 根目录使用 `.`。
- 执行前统一解析为规范化绝对路径，并复用同一解析结果做审批预判和实际执行。
- 解析后路径必须位于 workspace root 内。
- 越界拒绝覆盖 `..`、Windows drive-relative 路径、越界绝对路径、越界 UNC / verbatim 路径与符号
  链接越界；符号链接目标不可确认或越界时拒绝。
- 二进制读取返回明确错误。
- `apply_patch` 直接改文件，不经 shell 转发。

用户显式选择 full-access 时，本地文件 backend 与 `exec.cwd` 的 workspace 边界放宽：绝对路径与
`..` 可以解析到 workspace 外，但仍要求目标自身或其最近存在父目录可解析。该模式只影响本地工具
策略，不代表系统级完全隔离或提权；容器或远程 backend 可以继续拒绝越界路径。

SSH 远端 backend 始终拒绝 workspace 越界与符号链接入口，full-access 不放宽它。远端 helper 依据
远端文件系统事实做 canonicalize；模型不得把远端 canonical root 传给 `exec.cwd`，runtime 也不
猜测或自动截断绝对路径，绝对路径与 `..` 分别返回可操作且不同的稳定错误。`exec` 仍只约束 cwd，
不分析命令正文；远端命令拥有 SSH 用户权限，这一事实必须在连接与权限 UI 中可见。

## 4.4 凭据暴露面

- provider API token 保存在系统凭据库，配置只声明非敏感参数与环境变量名（见 [20](./20-config.md)）；
  UI 默认不回显完整 token，日志与事件 payload 不输出 token，错误
  信息禁止拼接敏感字段。
- SSH password 与 Askpass 回答只存在于系统凭据库或当前进程 secret lease，不进入 SQLite、
  transport DTO、日志、helper argv/env 或远端协议；Askpass secret 只进入本地 OpenSSH 子进程
  环境。
- 系统 OpenSSH 继续使用用户的 known_hosts、ssh config 与 agent；本地 provider token 不得通过
  SSH 转发，远端 Git 使用远端原生凭据或用户显式配置的 agent forwarding。

## 4.5 桌面 WebView 边界

- 不引入远程脚本执行入口。
- 只通过 pl-studio-bridge 调用本地 runtime。
- 文件选择、路径访问和工具执行仍由 core 策略校验。

## 4.6 本机 HTTP 边界

pl-studio-server 不提供远程鉴权能力，只允许绑定 loopback 地址。请求 Host 必须解析为 loopback
IP 或 localhost；带 Origin 的请求必须与当前 Host 同源，其他 Origin 一律拒绝。server 不发送 CORS
许可头，不接受把 wildcard、LAN 或公网地址作为监听参数。

HTTP 与 FRB 统一返回脱敏错误信封：稳定错误码、可读消息、可重试标志、关联 ID 与结构化细节。日志
可按关联 ID 记录内部诊断，但响应不得包含 token、配置正文、绝对私有路径、provider 原始错误或
数据库语句。OpenAPI 与 Swagger UI 是静态协议展示，不启动第二个 runtime。

## 4.7 数据切换安全

数据与配置升级分别遵循 [17](./17-studio-storage.md) 与 [20](./20-config.md)，当前实现缺口
集中记录在 [17.6](./17-studio-storage.md#176-迁移契约的实现缺口)。迁移失败不能授权清空数据库、
删除附件或替换为默认配置；备份与迁移产物具有与原数据相同的私密性，诊断不得泄露凭据。

迁移只操作已确认归属的 anywork 数据，拒绝符号链接 / reparse point 越界，不扫描或修改用户
workspace、Git repository 与无法确认归属的资源。迁移需要保留附件内容、资源引用与凭据关联；
更改存储位置不授予执行历史工具或删除外部资源的权限。
