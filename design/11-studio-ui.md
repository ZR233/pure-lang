# Pure Studio Timeline UI

Pure Studio 的主对话区采用明亮主题的连续 timeline。界面以正在执行的 turn 为中心展示：用户消息、思考、工具调用、assistant 文本和子代理活动都投影到同一条纵向时间线上，避免分散在独立卡片区。

## Timeline

- 主内容区保留顶部会话标题和项目路径。
- 消息流使用一条连续纵线和节点表达 turn 内事件顺序。
- 用户消息、assistant 文本、Thought、工具调用、子代理活动使用统一 timeline 行。
- 工具调用默认保持紧凑行高，展示工具名、状态和摘要；完成态使用弱提示，避免绿色状态胶囊抢占 timeline 视觉重心。
- 子代理活动在主 timeline 中按 `Agent: role` 日志行展示，不作为嵌套大卡片；状态详情放在行内 badge 和底部子代理弹层。
- 不渲染空 assistant 块；只有 content 或 reasoning 存在时才生成对应 timeline 行。
- trace 事件是 timeline 的持久化来源，前端可将 `TimelineItem` 和消息投影合并展示，但不能因为 trace 被二次 drain 而丢失。

## 底部状态栏

状态栏位于 composer 上方，左侧显示当前 turn 状态，右侧收拢运行上下文入口。

- 左侧 `TurnStatusIndicator` 显示 `idle | running | thinking | tool | subagent | approval | stopping | completed | interrupted | failed`。
- 运行中状态显示简短文案和 elapsed time。
- 右侧依次展示模型、上下文、能力、子代理数量。
- 模型只显示图标和模型名；费用信息不在常驻状态栏显示。
- 能力入口合并 skills 和 MCP，悬浮或点击时显示两类明细。
- 子代理入口显示当前会话去重后的数量；展开列表按运行中、等待中优先，其次按更新时间倒序展示 role、task、status dot/badge。

## Composer

- `isBusy=false` 时按钮显示“发送”，点击提交当前 prompt。
- `isBusy=true` 时按钮显示“停止”，点击后进入 `stopping`，按钮禁用以避免重复停止。
- 发送按钮不承载 turn 状态；turn 状态只由状态栏左侧表达。
- 运行中输入框暂不支持排队输入，保持禁用。

## 后台语义

- turn lifecycle 使用 `started | completed | failed | interrupted` 作为持久化和前端收尾语义。
- 用户停止是真实 interrupt：模型 streaming、工具执行和审批等待都要响应取消。
- 中断通过 finished response 返回 `turnStatus=interrupted`，不走 prompt failed 通道。
- Bash 工具收到取消时必须尽力终止子进程，并让 turn 以 interrupted 收尾。
- 浏览器预览模式也要模拟停止闭环，避免停止后被延迟完成结果覆盖为 completed。

## 验收

验收设计图固定为：

`C:\Users\zrufo\.codex\generated_images\019e6862-d8ea-7cf3-a960-1ab9c80828f5\ig_0fe8baa72b0dc9de016a16a4c71edc8195b125d6be7510ab63.png`

检查项：

- timeline 纵线节点、Thought 和工具行密度接近设计图。
- 状态栏左侧为 turn 状态，现存模型、上下文、能力等元素整体右对齐。
- 子代理数量入口可展开，列表能区分运行中、等待中、完成和失败状态。
- 发送按钮空闲时为“发送”，运行中为“停止”，停止后以 interrupted 正常收尾。
- 实际界面验收优先使用 Browser/IAB 截图；不可用时记录原因，再用 Playwright 或系统截图兜底。
