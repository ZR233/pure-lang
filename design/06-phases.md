# 06 - 实施阶段

## 6.1 P0：架构收束

| Crate | 范围 |
| --- | --- |
| `pl-protocol` | 公共错误、消息、事件、权限类型 |
| `pl-model` | LLM provider、模型元数据、wire API、SSE |
| `pl-core` | turn、session、Studio SQLite、角色化配置、工具审批、核心编译流程编排 |
| `pure-studio` | Tauri 2 桌面前端，负责 React UI、命令桥接、事件推送和输入回调 |

P0 不包含独立沙箱。`pure-studio` 可以接入工具系统；当前 Studio 运行路径暂时使用 `AutoAllow`，后续再补充更细粒度的手动审批、持久化授权和沙箱实现。

## 6.2 P1：核心能力完善

- 增加可替换的 session/store 抽象。
- 完善 `AgentEvent` 的编译阶段事件。
- 增加更多 provider 配置入口。
- 完成 `pl-core` 的 SeaORM SQLite 会话存储。
- 完成 `pure-studio` 的设置编辑器和流式 GUI。

## 6.3 P2：工具与执行策略

- 设计工具 trait 和权限策略。
- 接入文件编辑能力。
- 接入命令执行能力。
- 为执行输出建立统一事件流。
- 为桌面端补充更细粒度的审批策略、持久化授权和沙箱实现。

执行能力必须显式建模，不能隐式混入 `pure-studio` 或 `pl-model`。

## 6.4 验证命令

```powershell
cargo fmt
cargo clippy -- -D warnings
cargo test -p pl-protocol
cargo test -p pl-model
cargo test -p pl-core
cargo test -p pure-studio
cargo build -p pure-studio
npm --prefix code/pure-studio run typecheck
npm --prefix code/pure-studio run build
npm --prefix code/pure-studio run tauri:build
```
