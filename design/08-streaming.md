# 08 - 统一会话流事件

## 8.1 事实源与协议边界

`pl-core` 是 agent/session/turn UI 事件的唯一生产者和投影者。`pl-trace` 只保存模型、
工具和运行时的内部诊断 trace，不再导出供产品 UI 订阅的 `AgentEvent` broadcast。

跨产品 wire 类型统一位于 `pl-protocol`：

- `SessionEventEnvelope`：一条 session 事件。
- `SessionEventPosition`：`Durable { sequence }` 或 `Transient { revision }`。
- `SessionEventKind`：turn、message、part、interaction、agent、Todo、usage、context、
  capability、skill 和 plan 的结构化变化。
- `SessionViewSnapshot`：指定 durable sequence 时的完整、可归约单 agent session
  projection，并携带唯一 owner 身份。
- `SessionStreamFrame`：`Snapshot | Event | ResyncRequired`。

Studio 与 Mai 不定义第二套 message/part/turn 事件。产品可以拥有 project、task、review、
provider、settings 等低频 product event，但这些事件不得混入 session stream。

大会话与 agent session 是两层身份。大会话 root session 负责 project、标题、模式、task
与 agent 目录；root agent 和每个 child agent 都有自己的 `SessionId`。一个 `SessionId`
只能属于一个 owner agent，`SessionEventEnvelope.sessionId` 必须等于 owner 当前 session，
非空 `sourceAgentId` 必须等于 owner。跨 agent 输入必须同时指定目标 `agentId` 和
`sessionId`，repository 与 reducer 都要拒绝 owner 不匹配，不能把 child event 重新绑定到
root session。

## 8.2 Message/Part 模型

每个 message snapshot 携带 `messageId/sessionId/turnId/role/status/createdAt/updatedAt`。
每个 part snapshot 携带 `partId/messageId/sessionId/turnId/type/order/revision/status` 和
对应内容。消息与 part 首次创建后身份字段和展示顺序不可改变，终态不可回退。

part 类型固定为：

- `text`
- `reasoning`
- `tool`
- `agent`
- `turn`
- `inference`
- `plan`
- `file`

text channel 固定为 `user | commentary | final`。用户输入使用 `{turnId}:user` 与
`{turnId}:user-text` 的稳定 identity；内部 trace 中重复出现的用户输入不得再次投影。

`PartDelta` 只携带目标 part、field、delta、revision 和可选 chunk index。revision 必须相对
当前可见 revision 严格 `+1`；孤儿、重复、倒序、跳号或终态后的 delta 一律触发 resync，
不能静默拼接。

## 8.3 Durable 与 transient

稳定状态变化必须 durable：

- turn/message/part start、完成、失败、取消与审批状态。
- interaction、agent state、subagent activity、Todo replacement。
- usage/context、active skills、MCP/LSP capability、plan lifecycle 与 compaction。

文本、reasoning、plan、tool arguments/result 的中间增量是 transient。transient delta：

- 不占 durable sequence。
- 不进入 session event journal。
- 由 actor 校验 active turn 和 revision 后进入 live channel。
- 同时更新 `SessionEventHub` 的 live overlay，使重连 snapshot 能包含当前可见内容。

terminal snapshot 必须包含最终完整内容，并在 durable transaction 成功后清除 live overlay。
因此丢失 transient delta 不影响最终恢复。

## 8.4 SessionEventHub

`pl-core::SessionEventHub` 按 `SessionId` 懒创建独立的 Tokio broadcast channel，默认容量
1024。`AgentRuntimeHandle::subscribe_session` 接收 `SessionSubscriptionRequest`：

1. 先注册 receiver。
2. 再读取 snapshot 或 durable replay。
3. 返回 bootstrap frame 后进入 live。

这个顺序消除“读取 snapshot 时漏掉新事件”的窗口。session durable sequence 独立递增；
`eventId` 只用于诊断和去重，cursor 只使用 durable sequence。

无 cursor、cursor 早于 journal 下界、缺口超过 1000 条或 reducer 不变量失败时返回完整
snapshot。每个 session 默认保留最近 4096 条 durable event；更早的读取自动回到 snapshot。
receiver lag 时发送 `ResyncRequired` 并终止当前订阅，由调用方重新建立无 cursor 订阅。

Hub 的发布成本必须与当前 batch 规模相关，而不能与全部历史长度相关。journal 使用有界
增量 ring buffer，只追加当前 durable batch；repository 提交的 canonical projection 直接
移交给 hub，禁止 observer/hub 再复制完整 4096 条 journal。实现需记录 batch 大小、
messages/parts/journal 数量、投影耗时、广播积压与 resync 次数；snapshot 可共享只读所有权，
广播只复制引用。

## 8.5 提交与广播顺序

`SessionEventProjector` 在 PL 内把 runtime facts、trace 和 working-set checkpoint 映射为
canonical projection mutation 与事件。`AgentCommit` 在同一个 CAS transaction 内写入：

- agent snapshot、queue、session 与 usage。
- session projection 和 durable event journal。
- turn 与 raw trace。

成功顺序固定为：

1. repository transaction 返回 `Applied`。
2. `SessionEventHub` 广播 durable session events。
3. `AgentCommitObserver` 更新产品 read model 和 product event。

repository 失败或 revision conflict 时不得广播。产品 observer 无失败返回，不能反向回滚
framework transaction。transient delta 不进入 repository，但必须由 actor 完成 turn/revision
校验后广播。

Todo、context、interaction 和 skill activation 不得只发送 turn-local signal。工具或 pipeline
更新 working set 后必须先完成 actor checkpoint；durable ack 后才对 UI 可见。

## 8.6 UI reducer

UI 同时维护 `selectedRootSessionId`、`selectedAgentSessionId`、轻量
`AgentDirectoryProjection` 和当前 `AgentWorkspaceProjection`。Agent Directory 通过 product
stream 持续刷新，不携带 timeline、Todo 或 context；UI 只对当前可见 agent session 建立高频
订阅。切换 agent session 时：

1. 增加本地 generation 并关闭旧 stream。
2. 建立新 subscription。
3. 原子应用 snapshot 或 replay。
4. 归约 generation 匹配的 live event。

durable event sequence 不大于本地 cursor 时忽略。transient delta 按 frame 合批；同一 frame
有完整 snapshot 时，以 snapshot 为准并丢弃旧 delta。发现 revision 缺口、channel lag 或未知的
必要状态变体时立即 resync，不读取产品侧的另一套 timeline。

Studio Flutter 保留现有 generation、frame batching 和 Riverpod reducer 结构；Mai Web 使用同一
协议与 reducer 不变量。两端只能在视觉组件层做不同呈现。

child agent 的 lifecycle/status 变化只更新 Agent Directory，绝不修改
`selectedAgentSessionId`。当前工作区的 timeline、Todo、runtime/context、skills、
interaction、状态栏和 Composer 必须从同一个 agent snapshot/cursor 原子替换；迟到 frame
直接按 generation 丢弃。

## 8.7 产品事件

session stream 之外保留独立低频 product stream：

- Studio：project/session list、task orchestration、handoff、settings 等。
- Mai：environment、project、task、review、provider、settings、resource 等。

product stream 有自己的全局 sequence 和 replay，不承载 message/tool/Todo/context delta，也不能
以“收到任意事件后重拉整个详情”代替 session reducer。

Studio 的 session list 同时承担 `AgentDirectorySnapshot/Event`：每项只包含
`rootSessionId/sessionId/parentSessionId/ownerAgentId/ownerRole/displayName`、稳定创建顺序、
lifecycle/status、最近活动、错误和 attention 状态。目录不得携带其他 agent 的 timeline、
Todo、interaction 内容或 context，也不得再通过单 session 的 `AgentChanged` 聚合 agent tree。

## 8.8 Transport

Flutter Rust Bridge 与 Mai HTTP SSE 都传输 `SessionStreamFrame`：

- FRB 直接把 canonical Rust 类型机械转换为 Dart DTO。
- SSE 首帧为 snapshot 或 replay；只有 durable event 设置 SSE `id`。
- SSE 重连读取 `Last-Event-ID`；无法 replay 时发送 snapshot。
- keepalive、HTTP disconnect 或浏览器重连不能改变 PL session event 语义。

HTTP 与 FRB 必须使用同一 canonical JSON fixture 做契约测试。
