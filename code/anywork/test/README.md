# anywork 功能结果测试

测试从用户动作或公开状态边界出发，断言实际内容、保存结果、错误恢复与生命周期。
组件测试不是按私有方法逐个配套；不约束 widget 包装层、装饰参数、对象身份或内部 projection 数组。
稳定 ValueKey 用于定位用户操作，不要求实现必须由某一种 Material 控件组成。

## 全仓测试保留口径

自有 Rust、Flutter、xtask 与 Driver 测试按业务流程、异常终态和不可替代的真实集成结果审计。
字段回读、静态模板或翻译清单、主题装饰、命令参数镜像及重复层级的检查不作为独立测试；
配置保存、权限拒绝、数据恢复、并发取消、进程回收和协议失败仍须有可观察结果。
预置上游技能的 Python 测试随源码同步，不在此清理范围内。

提交门禁使用仓库根目录的 `cargo test --workspace` 和 `cargo xtask verify-gui`；
原生 GUI 用户旅程通过 `cargo xtask verify-gui --integration` 及按需运行的 Driver 验收补充。
纯 Dart demo、模拟 API 或测试文件所在的 `integration_test/` 目录本身不能证明真实 Rust bridge、
SSH、MCP、数据库及 provider 边界已通过验收。

| 判定 | 被保护的行为与可观察结果 | 重复性与处置 |
| --- | --- | --- |
| 删除 | 主题、翻译、内置目录、提示词、参数数组、字段回读、静态提示及布局坐标 | 仅复述源码或装饰值，不证明用户动作后的终态；删除独立用例。 |
| 并入 | 模型切换后的 effort 选项 Driver key、错误来源链序列化、请求标题解析 | 将独有断言放进实际选择、序列化或解析流程，删除重复的单点用例。 |
| 保留 | 提交/停止、配置保存、历史恢复、附件读取、远端项目打开 | 观察动作后的持久状态、可见反馈或跨组件结果；按行为与受测边界保留。 |
| 保留 | 权限与路径拒绝、迁移碰撞不丢数据、并发取消、进程回收、协议失败 | 即使正常流程相似，错误终态和保全证据无法由成功用例替代。 |

## 启动动画原生验收

启动 `cargo xtask run-gui --demo --driver` 后，从仓库根目录执行：

```sh
cargo dart run test_driver/startup_acceptance_driver.dart "$VM_URL" "$OUTPUT_DIR"
```

Driver demo 暂时显示真实启动页，记录动画区域坐标、连续截图及 render tree，检查布局溢出，
再验证回到主界面并等待 runtime shutdown。核对截图中猫爪的变化，避免将下方旋转进度条
误判为猫咪动画；生产启动流程不增加等待。启动 GUI 的宿主仍负责回收全部子进程。

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

## 最新主线复核

| 处理 | 依据与保留的证明 |
| --- | --- |
| 删除字段回读与重复解析 | `ToolCall` 身份回读、Skill frontmatter 二次解析、无动作的空会话 key 检查和 root/child fixture 自测不再独立占用用例；原始工具参数保全、Skill 目录发现、会话提交和实际 Thread 切换继续验证结果。 |
| 并入完整流程 | schema 排序方向、billing wire 版本、兼容模型的 chat endpoint、loopback 请求头与未初始化的 Skill 搜索拒绝并入各自行为测试，不放松原有失败断言。 |
| 删除跨层重复 | worktree 选择/标记在 widget 流程已覆盖，demo integration 不再重复执行这两条路径；原生集成仍保留提交、重定向、停止、恢复和 provider 设置旅程。 |
| 保留独特失败 | checkpoint schema、迁移归档与碰撞、权限拒绝、HTTP/FRB 契约、进程回收、LSP 错误及 Provider 工具策略仍保留；纯 Dart 测试不能替代真实外部边界。 |

新增/扩展结果场景包括三类指令独立保存、供应商草稿跨页面保留、技能展开后过滤与返回目录、
供应商完整用量与两类刷新、计划确认与代理状态栏的布局归属。纯 Dart/demo 不代表真实 provider
联网或真实 SSH/MCP/LSP 验收；对应生产后端能力继续由原有契约和 opt-in live harness 保护。

## 真实 timeline 验收回归

Timeline 展示的窗口/尾部/行投影专项自动化用例已移除；原生验收为按需手动检查，
不是 `verify-gui` 自动门禁。从仓库根目录运行：

```sh
python3 code/anywork/tool/timeline_native_harness.py --output /tmp/anywork-timeline-acceptance
```

完整阶段失败后保留同一隔离 home 并单独复核冷启动时，可再次运行上述命令并追加
`--reopen-only`；工具只在自己的临时 fixture 工作目录缺失时重建该目录。

此入口使用隔离 Studio home、真实 Rust/FRB/SQLite 与本地脚本 provider，启动 Linux 原生
GUI 的 full/reopen 两阶段。逐阶段人工核对 `*.png`、`*.tree.txt`、`snapshots.jsonl`、
`gui*.log`、`driver*.log`、查询与滚动 trace；检查回看双向翻页、切会话恢复锚点、
跳最新、实时完整正文、运行中 GUI 输入、失败/取消及关闭重开后的历史。
Driver 退出码或 `result.json` 不替代视觉和历史终态判断。

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
真实协作验收采集完整 Turn、inbox 和实际产物并人工观察；自然 final 与 finish_turn 都是合法结束，不要求固定结束工具或交付口令。
