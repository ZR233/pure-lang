# 13 - 工具调用运行时

## 13.1 身份与执行

每个工具调用携带 typed `ToolCallIdentity`，由当前 TurnId、provider item/call identity 组成。工具目录、
参数 schema、审批策略和 `ToolBatchPolicy` 在 inference 开始时冻结为代际快照。未知工具、参数解析、
审批拒绝与执行失败都形成 typed tool result，不伪造成功。

`Coexist` 工具可按普通 batch 执行；包含 `Solo` 工具的 response 必须恰好只有该一个调用，否则整批在
任何工具执行前拒绝。该规则通用，不依赖工具名；`workflow_state` 与 `complete` 都使用 `Solo`。

## 13.2 workflow_state

当活动 Mode Skill 选择工作流时，状态工具支持：

- `compile`：在无 workflow 或前一 run 已 terminal 时编译新定义并冻结 Mode Skill；
- `status`：读取 current、graph 或 history，不修改 revision；
- `transition`：以 run/revision/current stage CAS 沿一条直接边前进；
- `supersede`：先完整编译 replacement，再原子归档 active run 并创建同 lineage 新 run。

Runtime 校验 schema、图、CAS、合法直接边、大小限制与 operation identity；`when`、阶段完成标准和证据
真实性由 Agent 判断。拒绝结果总是返回 canonical snapshot、合法路径与 recovery actions。

稳定拒绝码为 `invalidDefinition`、`workflowNotCompiled`、`activeWorkflowExists`、
`terminalWorkflow`、`runMismatch`、`staleRevision`、`stageMismatch`、`unknownTargetStage`、
`transitionNotAllowed`、`operationIdentityConflict`。

幂等键是 `(turnId, callId, argumentHash)`：完全重放返回 `alreadyApplied`，同一 turn/call 使用不同参数
返回 identity conflict。成功 operation receipt 最近保留 32 条。

## 13.3 原子 checkpoint

状态调用先在 working-state clone 上运行。一个 checkpoint 同时持久化 assistant tool-call Item、tool
result Item 和更新后的 `AgentWorkingState`；提交失败不发布任何一个新事实，也不消费 revision。

transition result 内含最新 stage constraint，供同一 Turn 后续 inference 使用。下一 Turn 从 working
state 派生 `pl.workflow`；context compaction 后重新捕获最新 projection，不复用压缩前阶段。

## 13.4 普通能力

Workflow 阶段不能收缩文件、命令、Git、MCP、Agent 或最终回复工具目录。工具权限继续使用既有通用
approval/sandbox 策略。协作 `spawn_agent` 接受 `profileId` 并冻结 Profile；child 不注册 root 的
`workflow_state`，因此不能替 root 改写 run。

`spawn_agent` 对 directory Profile 接受 `writablePaths`；省略、空数组和非空数组分别表示项目内全可写、
只读和目录前缀白名单。所有 Pure 内置文件 mutation 通过中央策略检查，读取与项目外访问不受该字段
扩张或收缩。该策略不是 OS 沙箱，exec、Git 和 MCP 的 schema/固定 prompt 必须明确其可绕过目录限制，
并要求 child 不借这些工具修改白名单外项目文件。

## 13.5 统一完成

所有 root Mode 在完成请求后调用 `complete`，提交非空 `summary` 和可选、有界的 `evidence` 列表。
工具返回结构化完成事实并结束当前 turn；普通文本不能替代该调用。未选择 workflow 的 Mode 不需要
调用 `workflow_state`，但仍必须通过 `complete` 结束。child（包括 `worktree_executor`）保持直接结束，
不得要求其调用 root 专用的 `complete`。
