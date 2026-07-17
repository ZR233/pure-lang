# 04 - 安全边界（方案乙）

## 4.1 执行默认

方案乙默认权限模式固定为：

- `PermissionMode::RequestApproval`

这是破坏性升级后的默认行为，不再根据旧 UI 选择分支切换默认值。旧 `ToolApprovalPolicy::AutoAllow | Manual | DenyAll` 保留为兼容构造，但核心执行前统一以 `PermissionMode` 做策略判断。手动审批能力保留为可选能力，不是默认控制面。

Pure v1 的权限模式是本地策略层，不是 OS 沙箱、网络沙箱或系统级进程隔离。策略层只决定 Pure 已注册工具是否放行、请求用户审批、请求 AI reviewer 审批或拒绝；直接放行也不会绕过工具自身 schema 校验、工作区写锁、超时、输出截断和 timeline 记录。

权限模式：

- `request-approval`：默认模式。workspace 内文件读写、`apply_patch`、项目 skill 写入和 workspace cwd 的 `bash` 直接放行；工具请求访问 workspace 外路径或 workspace 外 cwd 时请求用户批准。
- `auto-review`：workspace 内行为同 `request-approval`；工具请求访问 workspace 外路径或 workspace 外 cwd 时交给 reviewer 模型审批。reviewer 只返回是否批准，不执行工具。
- `full-access`：所有已注册工具在策略层直接放行；文件工具可解析 workspace 外路径，`bash.workingDirectory` 可指向 workspace 外已存在目录。

execution profile 的工具 effect 白名单优先于权限模式。即使当前权限模式是 `full-access`，task planner、explorer 和 reviewer 也不能获得未声明或越权的写入工具。planner 仅在 design 更新和 merge 冲突阶段获得路径受限写权限。

## 4.2 分层边界

安全边界按端口-适配器落位：

- `pure-studio-flutter`：输入收集、事件展示、命令调用
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

当用户显式选择 `full-access` 时，Pure 放宽文件工具和 `bash.workingDirectory` 的 workspace 边界：绝对路径和 `..` 可以解析到 workspace 外。该模式仍要求目标自身或其最近存在父目录可解析，只影响 Pure 工具的本地策略，不代表系统级完全隔离或提权。

## 4.4 凭据暴露面

`config.toml` 仍为本地凭据来源，但方案乙收紧暴露面：

- UI 默认不回显完整 token
- 日志与事件 payload 不输出 token
- 错误信息禁止拼接敏感字段

## 4.5 桌面 WebView 边界

Flutter 桌面端的安全边界集中在本地工具策略、配置凭据和 Flutter/FRB 桥接。

桌面 UI 目标：

- 不引入远程脚本执行入口。
- 只通过 `pl-studio-bridge` 调用本地 runtime。
- 文件选择、路径访问和工具执行仍由 `pl-core` 策略校验。

## 4.6 数据切换安全

破坏性升级已完成，运行期不再保留迁移与兼容读取路径：

1. 当前版本只识别新 SQLite（`studio_2.sqlite`）与新 config（v2）
2. v1→v2 检测/备份/重建逻辑已删除；旧库文件直接忽略
3. 不做旧结构运行期兼容读取

恢复路径只通过用户在升级前生成的时间戳备份手动回滚，不通过应用内双栈兼容。
