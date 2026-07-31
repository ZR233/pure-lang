# 12 - 方案乙实施规范

## 1. 目标与范围

本规范用于约束方案乙 A-G 全量改造，属于一次性破坏性升级：

- 不保留旧命令兼容层
- 不保留旧 DTO 兼容解析
- 不保留旧 SQLite / config 运行期双栈读取

## 2. 命名与模块边界

`pl-core` 固定端口-适配器边界，但不再保留只做 re-export 的分层包装模块。公开 API 由 crate root 直接导出稳定入口，内部实现按业务命名空间组织：

- `interfaces`：端口 trait
- `studio`：Studio runtime、store、事件投影和 UI-facing records
- `core`：turn pipeline、权限和工具调度
- `tool`、`config`、`mcp`：具体适配器与运行时能力

新增端口抽象放入 `interfaces`；新增实现优先进入对应业务命名空间。不得新增 `application`、`domain`、`infrastructure` 这类只转发类型的兼容包装层；调用方应直接引用 crate root 导出的稳定类型，或在 crate 内部引用真实模块路径。

Flutter/FRB 固定结构：

- `pl-studio-bridge` 暴露 typed bridge command 和 event stream
- Flutter data repository 负责 bridge 调用与 JSON/DTO 适配
- Riverpod reducer/controller 负责归一化状态、actions 和 selectors
- `MaterialApp.router` 只做路由、主题和顶层页面装配

## 3. 端口规范

新增 trait 必须：

- 使用原生 RPITIT 异步签名
- 显式 `+ Send`
- 带文档注释说明职责和实现约束

示例：

```rust
pub trait SessionRepository: Send + Sync {
    fn list_sessions(
        &self,
        project_id: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<Vec<SessionRecord>>> + Send;
}
```

## 4. 数据迁移策略

SQLite（一次性破坏性升级已完成，运行期不再保留迁移入口与兼容读取路径）：

1. v1→v2 切换的 `studio_1.sqlite` 检测/备份/重建逻辑已删除；运行期只识别当前 `studio_2.sqlite`
2. 新 schema（v2+agent）为唯一支持结构
3. `subagent_events` 被 `agent_events` 替代；当前最终 schema 不再包含旧 `agent_messages` / `agent_turns` 表。历史 append-only migration 可以先创建或变更旧表，但后续 migration 必须显式 drop，运行期不读写，也不在关闭项目时清理旧残留行
4. `trace_events` 不再作为 Studio 对话流读取源；新 schema 使用 `studio_events`、`studio_messages`、`message_parts`、`turns` 和 `interactions` 作为 durable snapshot 与 projection
5. 旧 `session_handoffs` handoff/child session projection 已退出当前 schema。Plan 实施只在当前 session 内启动新 turn；历史 migration 可用于迁移旧 parent/child 关系，但最终 schema 需要 drop `session_handoffs`
6. 旧 `timeline_events` 的 entity、运行期写入、读取、cursor API 与项目关闭清理路径均已删除；其 drop 与 create 语句作为 append-only 迁移历史保留，确保已部署库的版本号不漂移，但运行期不再有代码读写该表

config：

1. 检测旧结构
2. 备份旧文件
3. 生成新结构模板
4. 仅迁入必要字段（provider/model/token/role）

## 5. 安全默认

- 默认权限模式固定 `PermissionMode::RequestApproval`
- 工具权限只由 `PermissionMode`、execution policy 和访问分类决定
- Flutter/FRB 桥接层不暴露 raw provider 私有结构
- token 不在 UI 和日志明文扩散

## 6. 发布工程化

新增 CI：

- PR 质量门：fmt / clippy / test / Flutter analyze / Flutter test
- RC 打包：通过 `cargo xtask build-gui` 生成当前 OS 的 Flutter 桌面构建产物

## 7. 验收口径

后端：

1. 高频事件 `Lagged` 不导致 drain 退出
2. `message.updated`、`message.part.updated`、turn 和 interaction snapshot 采用同一事务写入 `studio_events` 并更新 projection
3. 新 schema 启动切换可重复执行且有备份
4. wall-clock 预算耗尽时必须写入 `TurnBudgetLimited`，并保留观测用量
5. 用户显式要求子代理分工时，核心提示必须要求先用 `spawn_agent` 调度子代理，再由父会话汇总
6. `spawn_agent`、`send_input`、`list_agents`、`close_agent` 与 direct-child 订阅 continuation 形成通用协作闭环；工具层只持有 `AgentRuntimeHandle` 并提交命令，`AgentRuntime` 统一管理 actor、队列、活动 turn、取消、容量、订阅唤醒和 inactivity timeout；产品 harness 可以通过严格类型化工具调用同一 runtime。Task 根的通用 spawn 只允许 explorer，executor/reviewer 分别由 `task_spawn_executor` / `task_request_review` 创建；后续输入使用 `InputDelivery`，不保留 `resume_agent` 或模型可见等待工具
7. agent 状态正交拆为 lifecycle（`Active | Closing | Closed | Faulted`）与 activity（`Idle | Queued | Running | WaitingTool | WaitingInteraction | WaitingAgents`）；完成、失败、取消和预算限制属于 turn outcome，不污染 agent 生命周期
8. `close_agent` 按产品层 `AgentAccessPolicy` 校验目标，并由 runtime 与 host lifecycle saga 级联收束 live descendants；普通 turn 中断、失败或预算限制不会隐式关闭仍可继续工作的 agent
9. 父代理活跃时 child 更新只能合并，不能抢占；无其他工作时进入 `WaitingAgents`，由更新或每 child 独立 inactivity timeout 提交 typed wake batch。内部 `wait_until_idle` 使用 snapshot subscription predicate，不占用 actor waiter；完整树 snapshot 通过 `list_agents` 读取
10. `Done`、turn final、agent final、terminal `message.part.updated` 作为 lossless snapshot 处理，不因普通 live delta 背压丢失
11. 工具并行执行时，实际执行可并发，写回模型上下文的 tool result 顺序必须保持模型发出顺序

桥接：

1. `pl-studio-bridge` 只暴露 typed command、bootstrap snapshot 和事件 stream
2. 命令与 DTO 分层明确

前端：

1. reducer 接管业务状态
2. 顶层 app widget 显著收敛
3. 停止路径稳定 `interrupted` 收尾
