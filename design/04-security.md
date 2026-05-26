# 04 - 安全边界

## 4.1 当前安全模型

当前没有 CLI 路径。`CompileMode::Auto` 不是无人值守执行模式，而是“生成自动执行导向方案”的模型提示模式。

桌面端 `pure-studio` 可以启用工具系统，但首版必须使用手动审批策略。任何 `bash`、`subagent` 或会修改文件系统的文件工具调用在执行前都要展示工具名、参数、工作目录和风险提示，由用户批准或拒绝。

安全边界：

- `pure-studio` 只接收用户输入并展示结果。
- `pl-core` 维护配置、会话和模型调用。
- `pl-core` 维护 Studio SQLite 状态。
- `pl-core` 维护工具注册和审批策略。
- `pl-model` 只访问配置的模型 API。
- `pl-protocol` 只承载公共类型。
- `pure-studio` 只把用户批准或拒绝传给 `pl-core`，审批记录由 `pl-core` 保存。

文件工具的安全边界由 `pl-core` 维护：

- 所有路径必须解析到当前 `workspace_root` 内，拒绝路径穿越和逃逸到工作区外的绝对路径。
- 读工具默认只读取 UTF-8 文本；二进制内容返回明确错误，不作为文本发送给模型。
- 写入、删除、复制、移动和 `apply_patch` 属于修改操作，必须经过工具审批策略。
- 文件写入不得通过 shell 间接执行；`apply_patch` 只解析 patch 文本并直接操作文件系统。
- 对符号链接写入保持保守策略：目标不在工作区内或无法确认目标时拒绝。

## 4.2 权限类型

`PermissionLevel` 保留在 `pl-protocol`，用于未来工具、文件编辑和执行策略。

`PermissionLevel` 仍作为长期权限模型保留。当前桌面端的首版审批使用 `ToolApprovalPolicy` 和工具审批事件，不把 `PermissionLevel` 作为执行判定来源。

## 4.3 凭据配置

`~/.pure/config.toml` 允许保存明文 `bearer_token`。这意味着读取该文件的本机用户或进程可以直接获得 API token。

当前 provider 运行时只使用配置中保存的 `bearer_token` 作为 API key。`env_key` 只作为旧配置兼容字段和测试辅助信息保留，不在 `pure-studio` 界面展示，也不作为运行时鉴权来源。

## 4.4 未来执行策略

命令执行或文件编辑必须作为明确的执行策略接入：

- 默认拒绝破坏性操作。
- 文件写入和命令执行必须经过权限策略。
- 执行输出通过 `AgentEvent` 推送。
- 平台细节应保持在专门实现中，不污染 `pl-protocol`。
