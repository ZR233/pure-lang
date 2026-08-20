# 02 - Crate 边界

## 2.1 总体形态

Studio runtime 仍是模块化单体业务核心，同时提供一个可单独运行的 HTTP 宿主。依赖方向保持
单向，两个 transport 适配器互不依赖：

```text
pure-studio → pl-studio-bridge ─┐
                               ├→ pl-studio-runtime → pl-core
pl-studio-server ───────────────┘          │             │
                                          └→ pl-protocol ←┘
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
- `studio` 命名空间中的 transport-neutral command/query DTO、`StudioOperation`、
  `StudioError` 与产品/SSE 事件。

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

`pl-core` 内部按变化原因拆分编排职责。ThreadActor 的 durable 变更可以在各领域步骤中准备
不同 facts 和 mutation，但 repository CAS、内存状态替换以及提交后事件发布必须经过同一条
commit pipeline，保持“先持久化、后更新内存与广播”的原子边界。Turn 编排入口只保留主流程，
instruction 准备、checkpoint/mailbox 协调、工具结果投影等支线下沉到职责明确的子模块。
Instruction 领域进一步分离 wire/领域类型、宿主 profile、指令组装和模型上下文投影；工具缓存
分离执行编排、single-flight 状态、缓存条目投影、键与 mutation epoch、区间读取和确定性失败。
这些目录入口只导出最终公共类型，内部子模块不作为兼容路径暴露。
工具输出的 secret 遮蔽、生命周期投影和 artifact 捕获是相互独立的职责，由
`tool::output_format` 的对应子模块直接承载，不增加旧名字、别名或转发 façade，也不把产品层
持久化类型引入核心实现。

多个领域结构共享三个及以上稳定字段时，提取具名组合类型并让调用方直接访问该组合；需要保持
既有 JSON 键平铺的 serde 类型使用 `#[serde(flatten)]`。组合只复用同一领域语义，不因字段名
偶然相同而合并；重构后的旧字段和旧调用入口直接删除，不保留 alias 或兼容转发层。
体量较大、克隆频繁且读多写少的进程内领域对象使用内部 `Arc` 写时复制，共享只读状态并在首次
修改时分离；`Arc` 不进入 protocol、repository 或 durable checkpoint，持久化边界始终物化为
owned snapshot。单一所有者的大字段或大 enum 变体使用 `Box` 降低父类型的栈内尺寸；`Vec`、
`String`、map 等自身已持有堆数据的容器不重复装箱。
无失败、上下文无关的一对一领域转换使用 `From`；持久化行恢复为领域类型时，如果需要解析、
范围检查或兼容校验，使用 `TryFrom` 并保留 repository 错误语义。依赖多来源上下文、会丢弃信息
或携带业务默认的映射继续使用具名构造/投影函数，不为追求 `.into()` 形式而隐藏规则。

## 2.5 pl-studio-runtime

`pl-studio-runtime` 是 Studio 产品宿主，拥有：

- 单一 `studio.sqlite` 与 attachment/archive 路径；
- Project、Thread repository、配置、设置和全局 health；
- `TaskService` 及 TaskRun、WorkUnit、Delivery、ReviewRound、MergeRecord、BranchLease；
- worktree、冲突恢复、安全清理和产品事件。

`StudioRuntime` 是唯一业务 façade。聚合状态、Settings CAS/重载/reconcile、active-turn 校验、
MCP/LSP scope 解析、Project recovery 门禁、Skills/Provider 投影和完整生命周期只在这里实现。
transport 不得取得 Store、ConfigRuntime、event owner 或 skill catalog 等底层 accessor。

runtime 在打开 SQLite 或配置前取得 Studio home 下 `runtime.lock` 的跨进程独占 OS 文件锁；
锁由所有 runtime clone 共享，直到完整 shutdown/drop 才释放。同一 Studio home 不能同时由 GUI
与 server 占用。

Task 直接引用 executor/reviewer Thread，不把 Task phase 或 agent outcome 复制进 Thread stream。
Thread runtime 状态从 Thread/Turn 读取，Task 产品状态从 Task 表即时组成 `TaskSnapshot`。

## 2.6 pl-studio-bridge

`pl-studio-bridge` 位于 `code/pure-studio/rust`。它只做 Rust protocol 与 FRB DTO 的机械映射，
不保存状态、不投影 timeline、不拥有 Tokio runtime。

FRB 共享会话 API 统一为：

- `listThreadsPage`
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

只有 FRB 无法直接表达的 union、stream sink 和桌面宿主参数保留在 `frb_wire`，并使用 `Frb*`
命名。初始化、远程关机、关机进度、Driver fixture 与更新安装是桌面宿主专属入口，不进入共享
operation 清单。

## 2.7 pl-studio-server

`pl-studio-server` 是独立二进制与可测试库。`main.rs` 只解析 CLI、初始化 tracing、绑定 listener、
处理系统信号并驱动 shutdown；router、handler、SSE、OpenAPI 和错误映射位于库模块。业务路由
固定在 `/api/v1`，健康检查、OpenAPI 3.1 与 Swagger UI 分别位于 `/health`、`/openapi.json`、
`/docs`。spec 由 canonical DTO 和同一批路由生成，不提交生成 JSON。

server 只允许 loopback bind，不启用 CORS；Host 必须是 loopback/localhost，带 Origin 的请求
必须与 Host 同源。普通请求与 SSE 各有独立的 64 并发上限，JSON body 上限 4 MiB。它不提供
runtime 远程 shutdown 或桌面更新安装，也不进入打包、Release Please 或产物上传流程。

## 2.8 pure-studio

Flutter 数据路径为：

```text
FRB service → repository/controller → ThreadWorkspace → selectors → widgets
```

canonical state 只保存 Thread directory 与 `workspacesByThread`。Composer、滚动位置、展开项、
submission revision 和 stream generation 位于独立 `WorkspaceUiState`。Widget 不访问 SQLite，
不解析 raw event，也不把 UI 临时状态写回 canonical snapshot。

选中身份只有 `selectedThreadId`；root 通过 Thread 的 `rootThreadId` 派生。timeline、Todo、状态栏、
interaction 和 Composer 必须从同一个 workspace 原子切换。

## 2.9 pl-xtask

`pl-xtask` 只提供开发、生成、构建和运行命令：

- `cargo flutter <args...>`
- `cargo dart <args...>`
- `cargo xtask generate-gui`
- `cargo xtask verify-gui`
- `cargo xtask run-gui [--demo] [--driver]`
- `cargo xtask build-gui [--demo] [--no-clean]`

`cargo flutter` 与 `cargo dart` 是仓库级透传入口：它们把后续参数原样交给对应工具，
并把工作目录固定为 `code/pure-studio`。Windows GUI 构建和运行仍使用专用 xtask 命令，
不通过通用透传入口执行。

Windows 上 FRB 生成、GUI 构建和运行都必须通过 xtask。xtask 负责让 FRB
2.12 的 Rust root、生成输出与 canonical crate path 使用同一种 Windows 路径表示，
并在生成期间局部处理已锁定 Freezed 版本的兼容性。FRB 生成文件提交到仓库
但禁止手改。

## 2.10 数据版本

新 `studio.sqlite` 使用 `PRAGMA user_version = 1`。首次启动新架构时先生成只读 manifest，再把
`studio_state.sqlite`、`studio_history.sqlite`、`studio_2.sqlite`、它们的 WAL/SHM 和旧
attachments 一起移入时间戳 archive；全部归档成功后才创建新库。

不导入旧会话或 Task，不保留运行期 migration dispatcher。任何锁定、损坏或归档失败都
fail closed 并保留原文件。`config.toml` 使用 schema 14；provider API token 由系统凭据库保存，
TOML 不持久化明文 secret。配置内容不兼容时不运行迁移器，直接原子替换为当前初始配置；
文件 IO 或凭据库失败仍 fail closed。
