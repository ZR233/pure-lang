# 12 - 方案乙实施规范

## 1. 目标与范围

本规范用于约束方案乙 A-G 全量改造，属于一次性破坏性升级：

- 不保留旧命令兼容层
- 不保留旧 DTO 兼容解析
- 不保留旧 SQLite / config 运行期双栈读取

## 2. 命名与模块边界

`pl-core` 固定四层语义：

- `application`：use case 编排
- `domain`：领域类型与状态
- `interfaces`：端口 trait
- `infrastructure`：适配器实现

Tauri 固定结构：

- `commands/*`
- `dto/*`
- `events/*`
- `approvals/*`
- `state/*`
- `main.rs`（壳层）

前端固定结构：

- reducer + actions + selectors
- `App.tsx` 只做编排与组件装配

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

SQLite：

1. 检测旧库
2. 生成时间戳备份
3. 创建新 schema（v2+agent）
4. 不读取旧表
5. `subagent_events` 被 `agent_events` 替代；新 schema 不再创建旧表，运行期不读写旧表
6. `trace_events` 不再作为 Studio 对话流读取源；新 schema 使用 `studio_events`、`studio_messages`、`message_parts`、`turns` 和 `interactions` 作为 durable snapshot 与 projection
7. 旧 `timeline_events` 只作为破坏性迁移清理对象保留；运行期不再提供写入、读取或 cursor API

config：

1. 检测旧结构
2. 备份旧文件
3. 生成新结构模板
4. 仅迁入必要字段（provider/model/token/role）

## 5. 安全默认

- 默认审批策略固定 `ToolApprovalPolicy::AutoAllow`
- Tauri CSP 使用显式最小策略，禁止 `null`
- token 不在 UI 和日志明文扩散

## 6. 发布工程化

新增 CI：

- PR 质量门：fmt / clippy / test / web typecheck / web build
- RC 打包：Linux/macOS/Windows 构建产物

## 7. 验收口径

后端：

1. 高频事件 `Lagged` 不导致 drain 退出
2. `message.updated`、`message.part.updated`、turn 和 interaction snapshot 采用同一事务写入 `studio_events` 并更新 projection
3. 新 schema 启动切换可重复执行且有备份
4. wall-clock 预算耗尽时必须写入 `TurnBudgetLimited`，并保留观测用量
5. 用户显式要求子代理分工时，核心提示必须要求先用 `spawn_agent` 调度子代理，再由父会话汇总
6. `spawn_agent`、`wait_agent`、`list_agents`、`send_message`、`followup_task`、`close_agent` 形成完整协作闭环
7. agent 运行时状态采用 `queued | running | waiting | completed | errored | interrupted | shutdown | notFound`；预算限制不再作为 agent 状态，而是 turn abort reason
8. `close_agent` 拒绝 root，并级联 shutdown 目标 agent 的 live descendants；父 agent 因中断、错误或预算限制停止时，也必须关闭残留子树
9. `wait_agent` 返回 `{ message, timedOut, agents, recoverableFailures }`，其中 `agents` 只包含状态、摘要、错误和预算信息，不向主 chat 泄漏子 agent 内部工具输出
10. `Done`、turn final、agent final、terminal `message.part.updated` 作为 lossless snapshot 处理，不因普通 live delta 背压丢失
11. 工具并行执行时，实际执行可并发，写回模型上下文的 tool result 顺序必须保持模型发出顺序

桥接：

1. `main.rs` 降为壳层
2. 命令与 DTO 分层明确

前端：

1. reducer 接管业务状态
2. `App.tsx` 显著收敛
3. 停止路径稳定 `interrupted` 收尾
