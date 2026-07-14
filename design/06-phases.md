# 06 - 实施阶段

## 6.1 P0：架构收束

| Crate | 范围 |
| --- | --- |
| `pl-protocol` | 公共错误、消息、事件、权限类型 |
| `pl-trace` | 内部 AgentEvent、TraceEvent、TracePart 运行事件 |
| `pl-model` | LLM provider、模型元数据、wire API、SSE |
| `pl-lsp` | LSP client、语言服务器进程与代码智能查询 |
| `pl-core` | turn、session、Studio SQLite、角色化配置、工具审批、核心编译流程编排 |
| `pl-studio-bridge` | Flutter Rust Bridge v2 native crate，转发 Flutter API 与 event stream |
| `pure-studio-flutter` | Flutter Windows 桌面前端，负责 Material 3 UI、FRB 调用、事件订阅和输入回调 |

P0 不包含独立沙箱。`pure-studio-flutter` 接入工具系统时默认使用 `PermissionMode::RequestApproval`：workspace 内访问直接放行，workspace 外访问按权限模式请求用户审批、AI reviewer 审批或在 `full-access` 下放行。后续可以继续补充持久化授权和更强隔离，但不得把默认路径退回无边界的直接执行。

## 6.2 P1：核心能力完善

- 增加可替换的 session/store 抽象。
- 完善 `AgentEvent` 的编译阶段事件。
- 增加更多 provider 配置入口。
- 完成 `pl-core` 的 SeaORM SQLite 会话存储。
- 完成 `pure-studio-flutter` 的设置编辑器和流式 GUI。

## 6.3 P2：工具与执行策略

- 设计工具 trait 和权限策略。
- 接入文件编辑能力。
- 接入命令执行能力。
- 为执行输出建立统一事件流。
- 为桌面端补充更细粒度的审批策略、持久化授权和沙箱实现。

执行能力必须显式建模，不能隐式混入 `pure-studio-flutter` 或 `pl-model`。

## 6.4 验证命令

仓库通过 `.cargo/config.toml` 将 Rust test harness 的默认并发限制为 4。`pl-core`
同时运行大量本地 HTTP/SSE mock server、git 子进程和 SQLite runtime；在 Windows 上使用
CPU 核心数作为无界默认并发会造成临时端口连接失败，并让等待预期连接的异步测试长时间
不退出。`RUST_TEST_THREADS=4` 是当前全量回归通过的基线，外部 CI 可以显式覆盖，但提高前
必须用完整 workspace 测试验证。新增 mock 网络测试应继续使用独立临时目录、
`127.0.0.1:0` 动态端口和有界异步等待。

```powershell
cargo fmt
cargo clippy -- -D warnings
cargo test -p pl-protocol
cargo test -p pl-trace
cargo test -p pl-model
cargo test -p pl-lsp
cargo test -p pl-core
cargo test -p pl-studio-bridge
cd code/pure-studio-flutter
flutter analyze
flutter test
flutter build windows
```
