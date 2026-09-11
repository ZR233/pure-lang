# Pure Studio 功能结果测试

测试从用户动作或公开状态边界出发，断言实际内容、保存结果、错误恢复与生命周期。
组件测试不是按私有方法逐个配套；不约束 widget 包装层、装饰参数、对象身份或内部 projection 数组。
稳定 ValueKey 用于定位用户操作，不要求实现必须由某一种 Material 控件组成。

## 本轮清理

| 原覆盖 | 处理 | 保留的结果证明 |
| --- | --- | --- |
| JSON helper 的重复归一化断言 | 删除 | 工具参数/结果、搜索链接、图片结果的真实渲染测试 |
| 相邻工具的内部 row 数组及类型 | 删除重复项 | 工具组摘要、展开后的工具结果及顺序 |
| 消息 ordinal、频道、推理分组、文件和重复技能的中间数组 | 改写 | 屏幕上的顺序、各消息内容、推理展开、文件不混入正文及两次技能记录 |
| 设置外框类型、详情圆角、进度条固定高度 | 删除 | 配置保存、额度值与比例、详情打开/关闭、窄窗口可操作性 |
| 同一缺失价格 UI 的两份测试 | 合并 | 无费用和全部未定价两组输入均显示不可用费用 |
| 仅初始化却名为“保存后元数据”的测试 | 删除 | 供应商编辑/保存后配置、模型元数据与选择器的结果测试 |
| snapshot / controller 的对象身份断言 | 改写 | 旧 revision 不覆盖当前权限/服务、workspace 内容不被设置更新替换 |

保留目录分页、过期响应、并发保存、取消和关闭的结果测试：这些保护数据与生命周期，不是中间步骤。
保留无障碍、键盘、滚动恢复和 reduced-motion 测试：这些是用户可观察的行为。

新增/扩展结果场景包括三类指令独立保存、供应商草稿跨页面保留、技能展开后过滤与返回目录、
供应商完整用量与两类刷新、计划确认与代理状态栏的布局归属。纯 Dart/demo 不代表真实 provider
联网或真实 SSH/MCP/LSP 验收；对应生产后端能力继续由原有契约和 opt-in live harness 保护。

## 真实 timeline 验收回归

- 成功工具展开后可读取完整参数和长输出，不再丢失返回内容。
- 历史中的失败 Turn 显示于所属轮次末尾，不依赖 activeTurn 或 Driver 缓存。
- Rust TurnFinished 为缺失 trace 的终态失败发布持久化 Item，前端通过原有 typed 协议消费。
- 正文结束不提前追加“本轮已完成”，最终完成以校验后的 Turn 终态为准。

以上缺陷均记录了修复前失败与修复后通过；真实 GUI 另用隔离临时项目、Full 权限和
真实模型完成文件读取、修改、执行测试、complete 收尾，以及失败提示与历史恢复验收。

## 原生会话重启续聊

保留隔离的 Studio home 与项目，在 GUI 关闭后通过 `cargo xtask run-gui --driver` 重启，
再从仓库根目录运行（参数依次为 VM URL、原 Thread ID、续聊 prompt 文件、预期工具、证据前缀）：

```sh
cargo dart run test_driver/thread_recovery_acceptance_driver.dart "$VM_URL" "$THREAD_ID" "$PROMPT_FILE" read_file "$OUTPUT_PREFIX"
```

prompt 应要求读取原项目中的已知文件。Driver 必须观察到同一 Thread 的新 Turn 完成，且新增的
工具回执包含预期工具成功；旧 Turn 完成、旧工具回执或仅能加载历史都不能使验收通过。
Driver 保存快照和截图，并等待 runtime shutdown；启动 GUI 的宿主仍负责回收 Flutter/DTD/GUI 进程树。
SSH 项目使用相同入口，保留服务器配置与远端测试目录。通用简洁模式允许普通回复完成，
任务模式的 workflow/complete 专项断言仍要求成功的 `complete` 回执。
