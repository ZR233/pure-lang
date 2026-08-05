# 08 - Thread 实时流

## 8.1 帧模型

`subscribeThread(threadId)` 返回 `ThreadStreamFrame`：

- `snapshot`：订阅首帧，包含 Thread、当前/最近 Turn、完整 Item、pending Interaction、runtime、
  Todo 与 child directory。
- `notification`：后续 typed 变化。
- `lagged`：只表示 best-effort 事件发生丢弃，客户端必须重新订阅。
- `closed`：Thread 或 runtime 已关闭。

订阅实现先注册 receiver，再读取数据库并合并 ThreadActor live overlay，最后发送 snapshot，避免
snapshot 与 live 之间漏事件。实时流没有 durable cursor、journal replay 或 ResyncRequired
补丁协议；恢复永远重新取得 authoritative snapshot。旧历史通过 `listThreadTurns` 的 opaque
keyset cursor 读取。

ThreadEventBus 只拥有 Turn、Item、Interaction、runtime 与 live overlay 的实时投影，不拥有
Thread directory 元数据。订阅注册完成后，StudioRuntime 必须用同一 `studio.sqlite` 中读取的
Thread 行重绑尚未发送的首帧；不得把 EventBus 中为投影保留的 Thread 副本当成 mode、role、
title 或 status 的事实源。

## 8.2 Notification

通知穷尽为：

- `turnStarted`
- `turnUpdated`
- `turnCompleted`
- `itemStarted`
- `itemDelta`
- `itemCompleted`
- `interactionChanged`
- `threadRuntimeUpdated`

Item delta 只携带 threadId、turnId、itemId、field、revision、delta 和可选 chunkIndex。field
固定为 agent message text、reasoning summary/content、plan text、tool arguments/output。
terminal Item 携带完整 authoritative payload并清除 UI overlay。

## 8.3 背压

每个订阅使用有界 mpsc：

- transcript delta、Item terminal、Turn terminal 与 Interaction request 必须 lossless，发送方
  等待通道容量。
- 普通 progress/runtime 刷新是 best-effort，可用 try_send；丢弃数量在下一条 lossless 通知前
  以 `lagged` 发送。
- 不能丢弃需要客户端回答的 request；无法交付时后端取消 request，不能永久等待。

## 8.4 Flutter reducer

Flutter 为每个 Thread 保存 canonical `ThreadWorkspace`，为本地交互保存独立
`WorkspaceUiState`。snapshot 直接替换 canonical workspace；旧 Turn/Item/runtime 不与新
snapshot 混合。Composer、滚动、展开和 submission revision 不属于 canonical snapshot。

Thread directory 是 Thread 元数据的唯一 canonical cache。snapshot 中携带的 Thread 只用于
校验身份并重绑到当前 directory entry，不能反向覆盖 directory。这样 product stream 与 thread
stream 即使并发到达，也不会用旧 workspace snapshot 回滚刚确认的 mode/role。

切换 Thread 时增加 generation、立即创建新订阅并取消旧订阅；旧 generation 的 frame、error
和 done 全部丢弃。Item delta 只允许命中当前未终态 Item且 revision 严格递增；缺口、未知变体
或 lagged 统一重新订阅。

## 8.5 Product stream

项目、root/child Thread directory、Task、设置、Provider usage 和 MCP/LSP health 使用独立低频
product stream。product stream 不携带 Turn/Item delta；Thread directory 变化只重绑 workspace
中的 Thread 元数据引用，不改变 Turn、Item、Interaction 或 runtime 内容。
