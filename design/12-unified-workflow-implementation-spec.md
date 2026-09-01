# 12 - 统一模式与工作流实施规约

## 12.1 原子架构切换

Studio 只保留 `Thread → Turn → Item` 会话结构与一个 root Agent 模型循环。Simple/Task 成为内置
Mode Skill；自定义模式复用相同发现、预加载和 GUI projection，并自行决定是否编译 workflow。旧 Task 数据不迁移，schema
版本升级时破坏性重建 Studio 本地数据库。

必须物理删除而非隐藏的旧边界包括：固定 Mode enum、root 角色分支、TaskCoordinator、TaskRuntime、
TaskRun、WorkUnit、ReviewRound、MergeRecord、Task issue/recovery、worktree/branch/merge 管理、Git
门禁、专用 PlanConfirmation、Task driver provider 和旧 task tool catalog。
旧计划工具遗留的输入流投影、Plan trace part、Thread plan item、FRB/Dart plan union 与专用
timeline 渲染同样必须删除；工作流计划通过普通 assistant 内容展示。新的自由工具 `submit_plan` 复用
旧工具的 typed Markdown 输入、完成校验、submitted receipt 与 end-turn 行为，但把确认映射为通用
`UserInput`；它不恢复旧 Task runtime、Plan 产品类型或专用投影。

## 12.2 实施顺序

1. 在 `design/*` 定义 Mode namespace、workflow 协议、Profile、存储和 GUI 事实归属。
2. 在 `pl-protocol` 增加 typed workflow/Profile 类型，在 `pl-core` 实现纯函数编译器和状态工具。
3. 将 workflow 放入 `AgentWorkingState`，接入 tool-call identity、Solo batch 与原子 checkpoint。
4. 以 Mode Skill catalog 和统一 TurnFactory 替换模式/角色分支。
5. 注册内置 Profile，加载用户单 TOML Profile，并改造协作工具使用 `profileId`。
6. 删除旧 runtime/store/projection/bridge/Flutter surface，执行数据库破坏性版本切换。
7. 生成 FRB/l10n，完成单元、workspace、GUI integration 和真实 live GUI 验收。

## 12.3 完成定义

- repo 搜索不再发现可执行路径中的旧 Task runtime tool、PlanConfirmation 或 Task 产品类型；
- 内置 Mode 不可覆盖，活跃 run 使用冻结 snapshot，自定义模式无需代码改动即可出现；
- workflow 编译、CAS、幂等、supersede、Solo 与 checkpoint rollback 有自动化测试；
- `complete` 的 typed schema、Solo 执行、完成事实和 root finalization 有自动化测试；
- 系统 Profile 只读且只能禁用，用户 Profile 一个文件一个配置；
- 无 `.git` 临时项目能够通过真实 `mode.simple` 直接完成以及真实 `mode.task` GUI 全流程并重启恢复；
- 全量门禁与 `git diff --check` 通过，live 缺少真实凭据时必须明确记为未执行。
