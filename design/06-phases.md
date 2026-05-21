# 06 - 实施阶段

## 6.1 P0：架构收束

| Crate | 范围 |
| --- | --- |
| `pl-protocol` | 公共错误、消息、事件、权限类型 |
| `pl-model` | LLM provider、模型元数据、wire API、SSE |
| `pl-core` | turn、session、角色化配置、核心编译流程编排 |
| `purec` | clap 参数解析、配置子命令、调用核心层、渲染结果 |

P0 不包含命令执行、文件编辑、工具系统或沙箱。

## 6.2 P1：核心能力完善

- 增加可替换的 session/store 抽象。
- 完善 `AgentEvent` 的编译阶段事件。
- 增加 `purec` 的流式渲染。
- 增加更多 provider 配置入口。

## 6.3 P2：工具与执行策略

- 设计工具 trait 和权限策略。
- 接入文件编辑能力。
- 接入命令执行能力。
- 为执行输出建立统一事件流。

执行能力必须显式建模，不能隐式混入 `purec` 或 `pl-model`。

## 6.4 验证命令

```powershell
cargo fmt
cargo clippy -- -D warnings
cargo test -p pl-protocol
cargo test -p pl-model
cargo test -p pl-core
cargo test -p purec
cargo run -p purec -- --help
```
