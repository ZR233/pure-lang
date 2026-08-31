# Mode Skill 与可编译工作流

## 16.1 单一会话框架

Studio 不再拥有 Simple/Task 两套运行时。所有 root Thread 使用相同的 Thread、Turn、model、tool、
store 与 collaboration 框架；模式只是预加载的系统指令 Skill。`ModeId` 是动态字符串，内置值为
`mode.simple` 与 `mode.task`，自定义模式为 `mode.<id>`。

工作流只影响模型提示和合法状态边，不关闭文件、命令、Git、Agent 或最终回复能力；是否使用工作流
由 Mode Skill 决定。所有 root Mode 统一通过 `complete` 工具提交完成事实并结束 turn。产品不再拥有
TaskRun、WorkUnit、Completion、ReviewRound、MergeRecord、TaskIssue、worktree、completion gate
或 recovery 状态。

## 16.2 Mode Skill 命名空间

Mode Skill 必须使用 `mode.` 前缀，声明 `mode.display-name`、`mode.order`，并同时设置
`disable-model-invocation: true`、`user-invocable: false`。带 `mode` 元数据但没有前缀，或占用前缀却
缺少元数据的 Skill 都失败关闭。

`mode.simple`、`mode.task` 只接受 Studio 内置 Provider 注册的资源。其他来源的同名候选产生 warning
并在 precedence 之前剔除，不能覆盖预设。其他自定义 ModeId 继续按现有 Skill 来源优先级选 winner。
Mode Skill 不进入普通 `skill_list`/`skill_view`。

没有活动工作流时，Turn 加载当前 winner。第一次成功编译冻结完整 Skill 正文、来源、provider、revision
与 hash；活跃 run 后续只消费冻结快照。正常终态后的新 compile 重新加载最新 winner；supersede 继承
当前快照。模式切换要求 root idle、没有 pending interaction，且当前 workflow 不存在或已终止。

## 16.3 持久化状态

`AgentWorkingState.workflow` 保存 `WorkflowSessionState`：单调 revision、当前 run、最近 16 个归档摘要、
滚动摘要以及最近 32 个成功 operation receipt。当前 run 保存 lineage/run id、完整 definition、
definition hash、Mode snapshot、active/terminal、当前 stage、时间和最近 64 条 transition。

完整 typed state 最大 256 KiB。旧细节由 Thread timeline/trace 审计，热状态只保存有界尾部。
模型上下文不直接暴露完整 JSON；`AgentSession::working_context_snapshot` 派生保留 id `pl.workflow`，
只包含当前阶段指令、完成标准、允许边、最近摘要和下一次 CAS 参数。

## 16.4 图编译

定义包含 title、goal、initialStageId、stages 与 transitions。最多 32 个阶段、96 条边，规范化 JSON
最大 64 KiB。stage id 只允许小写 ASCII、数字、`-`、`_`，长度 1–64。非终态必须有指令、完成标准
与出边；终态禁止出边；允许自环、循环和返工；同一 from/to 只能出现一次。

纯编译器先做 schema/大小/唯一性检查，再校验端点与初态；从初态正向遍历拒绝不可达节点，从所有
终态反向遍历拒绝无法终结的非终态。成功后保留声明顺序用于 UI，以 canonical JSON 计算稳定 hash；
数组顺序是 definition 的一部分，相同规范化定义必然得到相同 hash。编译复杂度为 `O(V + E)`。

## 16.5 `workflow_state`

工具是 root-only tagged union：

- `compile { expectedRevision, expectedRunId?, definition }`：无 run 时 revision 为 0；已有终态时携带其
  最新 id/revision；活跃 run 拒绝。
- `status { view = current | graph | history }`：未编译返回 revision 0 和 compile 指引。
- `transition { expectedRunId, expectedRevision, expectedStageId, toStageId, reason, completion }`：
  表示完成当前阶段并原子进入直接相邻阶段；进入 terminal stage 时 run 终止。
- `supersede { expectedRunId, expectedRevision, expectedStageId, reason, definition }`：先完整编译 replacement，
  再原子归档旧 run 并在同 lineage 创建新 run；不允许原地 patch 图。

`when` 与 completion criteria 是 Agent 的提示约束，Runtime 不推断外部工作是否完成。所有语义拒绝
返回 `accepted: false`、稳定 code、最新 canonical snapshot、constraint prompt 与 recovery actions，
且无副作用。

成功操作以 `(turnId, callId, argumentHash)` 记录。相同身份与参数重放返回 `alreadyApplied`；相同身份
不同参数返回 `operationIdentityConflict`。run id、revision 与 stage id 同时参与 CAS。

`workflow_state` 与 `complete` 使用 `ToolBatchPolicy::Solo`。同一 provider response 若还包含其他调用，整批不执行。
工具只在 working-set clone 上计算；tool call、result 与新 working state 由随后同一个 Thread checkpoint
共同提交，失败则保持旧 canonical revision。

同一 Turn 的 working context 为 prompt cache 保持冻结，切换结果由 tool result 立即提供；下一 Turn
由 `pl.workflow` 注入。若发生 compaction，则在 rebase 边界重新捕获最新 workflow projection。

## 16.6 内置流程 Skill

`mode.simple` 不编译 workflow，也不要求阶段转换；它直接工作、按风险验证，并在完成时调用 `complete`。
它不增加 Git、固定审查轮次或交付门禁。

`mode.task` 默认编译 `planning -> awaiting_confirmation -> editing_documents -> working -> integrating ->
reviewing -> completed`，并提供 stopped 终态。确认修改回到 planning；代码 finding 回到 working，设计
finding 回到 editing_documents；两条返工路径都必须重新经过 integrating 和 reviewing。确认使用通用
`request_user_input`，进入 `completed` 后调用 `complete`。

planning 中 root 优先把相互独立的探索并行交给 fresh-context `explorer`，自己综合依赖图、文件所有权与
验证边界。editing_documents 中 root 亲自更新设计。working 中普通实现必须交给 `executor` 或
`worktree_executor`：单任务和互斥目录并行优先 directory，可能交叉影响共同接口、清单、生成边界或 Git
状态时使用 worktree；真实前后依赖始终顺序执行。每个 child 消息必须详细描述目的、基线、所有权、
禁止范围、有序步骤、完成/失败条件、证据和 workspace/Git 合同。

integrating 中 root 审查 directory 组合 diff、显式采纳 worktree commit、cleanup 并处理冲突。冲突处理中
允许完成相邻必要实现或测试修复，但不能展开无关工作。child 持续不可用时，root 收窄重派一次后可以
最小兜底，并显式记录 `ROOT_IMPLEMENTATION_FALLBACK`。reviewing 必须由新创建的只读 `reviewer` 检查
整合后的主 workspace；root 自审不能替代它，阻塞 finding 修复后必须创建新的 reviewer 复审。

上述职责由冻结 Mode/Profile 指令约束，不新增专用 executor/reviewer runtime 生命周期，也不按阶段裁剪
普通工具能力。

## 16.7 GUI 与 live 验收

GUI 动态展示 ModeCatalog、当前 stage、允许边和 transition history。Driver 使用稳定 ValueKey，不等待
可能瞬间经过的中间 Widget，而是在终态读取 canonical history。

wire replay 对 provider 请求历史中被重试替换的 transition 增量草稿只做形状校验：流式阶段允许
completion 字段尚未完整到达，验收边界仍可继续；运行时执行 `workflow_state` 与 `complete` 时使用
严格 typed schema，并由业务校验拒绝空 summary。

最终入口是 `cargo xtask verify-workflow --live --headless` 与 `cargo xtask verify-workflow --live --gui`。
它们必须使用真实非本地 provider 和真实 Prompt，不得 fallback 到 scripted/demo provider。workflow
harness 在隔离 Rust 项目中分别选择 `mode.simple` 与 `mode.task`：简洁模式直接完成且不产生 workflow
调用，任务模式走完 planning/confirmation/document/working/integrating/review/completed 并调用
`complete`。子代理 GUI harness 另使用带初始 commit 的隔离 Git fixture，验证 explorer、两种 executor、
显式整合和只读 reviewer。两种模式都运行 `cargo test` 与 verifier；GUI 额外重启任务模式验证 run id/
revision/history 持久化。失败 artifacts 保存到 `target/workflow-live-artifacts/` 或
`target/subagents-live-artifacts/`，同时回收 GUI、DTD 与 Driver 进程树。
