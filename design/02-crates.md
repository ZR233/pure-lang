# 02 - Crate 边界

## 2.1 总体形态

本仓库是模块化单体，不新增常驻进程。依赖方向保持单向：

```text
pure-studio → pl-studio-bridge → pl-studio-runtime → pl-core
                                      │               │
                                      └── pl-protocol ←┘
                                              ↑
                              pl-model / pl-trace / pl-lsp
```

Studio 的会话主线统一使用 `Thread → Turn → Item`。`Session`、`Message/Part`、durable event
journal 和双库 projection 不再是公共或内部架构边界。

## 2.2 pl-protocol

`pl-protocol` 定义跨 crate 的稳定 wire 类型，不包含行为和存储实现：

- `Thread`、`Turn`、`ThreadItem`、`Interaction`；
- `ThreadSnapshot`、`ThreadRuntimeSnapshot`、历史分页结果；
- `ThreadNotification`：Turn、Item、Interaction 和 runtime 的 typed 通知；
- provider catalog、capability、MCP/LSP health、usage 与公共错误。

`ThreadItem` 是穷尽 union。协议中未知 union 变体必须失败，不能降级成文本。只有工具参数、
工具结果 artifact 等明确开放的叶子允许动态 JSON。

## 2.3 pl-trace、pl-model 与 pl-lsp

- `pl-trace` 保存 provider/工具内部诊断事件。trace 可被 `TurnEngine` 解释成 Item，但不直接
  广播给 UI，也不是持久事实源。
- `pl-model` 适配 provider、模型目录与物理 transport。transport session 不等于 Thread。
- `pl-lsp` 管理 LSP JSON-RPC 和语言服务器生命周期，不参与会话状态机。

现役 OpenAI-compatible、Zhipu-compatible provider 都属于正式适配器，不作为 legacy 删除。

## 2.4 pl-core

`pl-core` 是产品无关的 Thread runtime：

- `ThreadManager`：registry、spawn/close 和 Thread directory watch；
- `ThreadActor`：单 Thread 输入队列、活动 Turn、取消、steer 与 live Item overlay；
- `TurnEngine`：模型采样、工具执行、interaction 和上下文压缩；
- agent control tools：以 `agentPath` 定位 Thread；
- 通用工具、effect、执行策略、MCP 与 Web Search runtime。

核心 host 端口只有 `ThreadRepository`、`TurnFactory` 和 `ChildLifecycle`。普通 turn 命令由
`ThreadHandle` 直接发送给 `ThreadActor`，不经过第二层 coordinator/loop 路由。

`pl-core` 不依赖 SeaORM，不知道 Studio 路径、schema 或 Task 表；`TurnFactory` 直接返回可执行
的 engine、request 和 policy，不保留只做转发的 kernel façade。

## 2.5 pl-studio-runtime

`pl-studio-runtime` 是 Studio 产品宿主，拥有：

- 单一 `studio.sqlite` 与 attachment/archive 路径；
- Project、Thread repository、配置、设置和全局 health；
- `TaskService` 及 TaskRun、WorkUnit、Delivery、ReviewRound、MergeRecord、BranchLease；
- worktree、冲突恢复、安全清理和产品事件。

Task 直接引用 executor/reviewer Thread，不把 Task phase 或 agent outcome 复制进 Thread stream。
Thread runtime 状态从 Thread/Turn 读取，Task 产品状态从 Task 表即时组成 `TaskSnapshot`。

## 2.6 pl-studio-bridge

`pl-studio-bridge` 位于 `code/pure-studio/rust`。它只做 Rust protocol 与 FRB DTO 的机械映射，
不保存状态、不投影 timeline、不拥有 Tokio runtime。

会话 API 统一为：

- `listThreads`
- `readThread`
- `listThreadTurns`
- `startTurn`
- `steerTurn`
- `interruptTurn`
- `respondInteraction`
- `subscribeThread`

项目、Task、设置、provider catalog、usage、health、应用更新和清理 API 保持 typed。会话 API
不再暴露 Session、Message、Part、cursor replay 或整帧 JSON。

`subscribeThread` 先注册监听，再返回 authoritative `ThreadSnapshot`；后续只发送 typed
notification。订阅 handle 显式取消，runtime shutdown 会等待所有 handle 退出。

## 2.7 pure-studio

Flutter 数据路径为：

```text
FRB service → repository/controller → ThreadWorkspace → selectors → widgets
```

canonical state 只保存 Thread directory 与 `workspacesByThread`。Composer、滚动位置、展开项、
submission revision 和 stream generation 位于独立 `WorkspaceUiState`。Widget 不访问 SQLite，
不解析 raw event，也不把 UI 临时状态写回 canonical snapshot。

选中身份只有 `selectedThreadId`；root 通过 Thread 的 `rootThreadId` 派生。timeline、Todo、状态栏、
interaction 和 Composer 必须从同一个 workspace 原子切换。

## 2.8 pl-xtask

`pl-xtask` 只提供开发、生成、构建和运行命令：

- `cargo xtask generate-gui`
- `cargo xtask verify-gui`
- `cargo xtask run-gui [--demo] [--driver]`
- `cargo xtask build-gui [--demo] [--no-clean]`

Windows 上 FRB 生成、GUI 构建和运行都必须通过 xtask。xtask 负责让 FRB
2.12 的 Rust root、生成输出与 canonical crate path 使用同一种 Windows 路径表示，
并在生成期间局部处理已锁定 Freezed 版本的兼容性。FRB 生成文件提交到仓库
但禁止手改。

## 2.9 数据版本

新 `studio.sqlite` 使用 `PRAGMA user_version = 1`。首次启动新架构时先生成只读 manifest，再把
`studio_state.sqlite`、`studio_history.sqlite`、`studio_2.sqlite`、它们的 WAL/SHM 和旧
attachments 一起移入时间戳 archive；全部归档成功后才创建新库。

不导入旧会话或 Task，不保留运行期 migration dispatcher。任何锁定、损坏或归档失败都
fail closed 并保留原文件。`config.toml` 继续使用 schema 12。
