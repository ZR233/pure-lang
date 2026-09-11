# Pure-Lang Agent 协作规范

本文件是本仓库面向 Codex、Claude 和其他 AI agent 的唯一项目级协作规范。与用户交流、计划、总结和 PR 描述默认使用中文。命令、路径、代码标识符和正式技术名称保留原文；通用质量方法通过下文技能入口按需读取。

## 协作与变更原则

- 将“帮我”“修复”“实现”等明确行动请求视为执行授权；在用户指定范围内连续完成必要的读取、修改、相关文档同步、验证和失败修复，不在步骤之间反复询问是否继续。已有授权持续有效，常规实现选择按现有约定自行处理。
- 先从会话、代码和文档核实缺失信息；只有无法推断且会改变业务语义、交付目标或授权范围的信息才询问。明确指出缺什么及影响，等待期间可完成不依赖答案的已授权工作，不猜测关键业务决策。
- 只读审查不隐含修改授权；本地修改不自动授权提交、推送、合并或部署。用户要求“修复并提 PR”时，可连续完成修改、验证、提交、推送和创建 PR；未授权的外部写入或破坏性操作须先完成可独立准备的工作，再就具体动作询问，并遵守实际工具权限限制。
- 用户明确指令优先于本文件和技能中的工作流建议；技能补充领域要求，不重复设置审批步骤。若规则确实阻塞任务，引用具体文件与条款并说明原因，区分明确要求和自身推断。
- 修改前先理解现有边界和模式，优先沿用仓库已有抽象、命名和测试方式。
- 不回滚用户或其他 agent 的未提交改动；同一文件存在相关改动时，先读懂再叠加。
- 涉及产品架构、接口、协议、运行时行为或长期约定时，先更新对应 `design/*`，实现后再核对文档与代码一致；仅调整 agent 协作规则或技能时直接维护相应文件。用户限定文件范围时不扩改，范围外所需同步列为建议。
- 设计文档只描述架构、接口、行为和约定；具体实现细节留在代码、类型签名和必要注释中。
- 用户已明确要求改变的行为，可在授权范围内同步文档并实现；只有无法从需求与上下文判断的业务冲突才保留现状、说明冲突并询问，不因文档尚未同步而再次索要相同授权。
- 提 PR 或创建分支时，优先推送到用户账号 fork，并优先向 fork 源仓库的默认分支提 PR。

## Git 提交与 PR

- commit subject 和 PR 标题必须使用 Conventional Commit：`<type>(<scope>): <description>`。
- `type` 仅使用 `build`、`chore`、`ci`、`docs`、`feat`、`fix`、`perf`、`refactor`、`revert`、`test`。
- `scope` 可省略，但优先使用稳定范围，如 `studio`、`agent-runtime`、`release`。
- 中文描述可以使用，但必须保留英文冒号，例如 `fix(studio): 修复项目清理确认无效`。
- 用户可见缺陷优先使用 `fix`，新能力优先使用 `feat`；破坏性变更使用 `!` 或 `BREAKING CHANGE` footer。
- squash 合并前同时检查 PR 标题和实际 commit subject，确保最终提交可被 Release Please 解析。

## 项目目录与命令入口

### 基本目录

- 仓库 Python 脚本统一用 `python3` 运行；系统 `python` 是 Python 2。
- Rust crate 名称统一以 `pl-` 开头，例如 `pl-core`、`pl-model`、`pl-studio-bridge`。
- Flutter app 的 Dart package 名称是 `pure_studio`。
- Flutter 项目根目录是 `code/pure-studio`。

### Flutter 与 Dart 命令

- 禁止在 Flutter 或 Dart SDK 安装目录执行本项目的 `flutter`、`dart` 命令。项目命令的工作目录必须是 `code/pure-studio`，或由仓库包装命令显式切换到该目录。
- 从仓库根目录执行 Flutter/Dart 命令时，优先使用 `cargo flutter <args...>` 和 `cargo dart <args...>`；参数原样透传，包装命令会自动切换到 `code/pure-studio`。
- 只有包装命令无法满足需求时，才可直接运行 `flutter` 或 `dart`，且必须先把工作目录切换到 `code/pure-studio`；不得把 SDK 目录当作项目目录。
- 常用命令：

  ```powershell
  cargo flutter analyze
  cargo flutter test
  cargo dart format lib test
  ```

### 桌面 GUI

- GUI 构建和运行必须从仓库根目录通过 xtask（按当前 OS 选择目标平台：Windows / Linux）：

  ```powershell
  cargo xtask run-gui [--demo] [--driver]
  cargo xtask build-gui [--demo] [--no-clean] [--check-generated]
  ```

- 不支持直接执行 `flutter build windows|linux` 或 `flutter run -d windows|linux`，也不新增 PowerShell GUI wrapper。
- Linux 原生 GUI 需要 Clang/C++ 标准库、CMake、Ninja、pkg-config 与 GTK 3 开发文件；Debian/Ubuntu 示例为 `sudo apt-get install -y clang cmake ninja-build pkg-config build-essential libgtk-3-dev`。xtask 必须用当前 PATH 和真实最小 GTK/C++ 工程预检，缺失时透传实际命令与原始错误；不得写死编译器版本、系统库路径或注入机器专用 include/library 环境。Rust 桥以 `libpl_studio_bridge.so` 预构建并经 `PURE_STUDIO_BRIDGE_LIBRARY` 环境变量注入 CMake，与 Windows 的 DLL 契约一致。
- `cargo xtask run-gui --driver` 使用 `test_driver/driver_main.dart` 启用 Flutter Driver extension，供 Dart MCP 的 `flutter_driver_command` 操作 GUI；xtask 不负责启动实验性的 `dart mcp-server`。
- 本地原生验收默认使用当前受支持宿主平台，报告中明确平台与覆盖范围，不把单平台结果外推为跨平台通过；跨平台任务按实际影响说明其他平台的验证结果或缺口。
- GUI smoke 使用 `cargo xtask run-gui --demo`；需要确定性数据和交互验收时使用 `cargo xtask run-gui --demo --driver`。
- Driver 命令在正常完成、失败或取消后都必须等待并回收所属 Flutter、DTD、GUI 及其子进程，不得残留；Windows 使用 Job Object 管理进程树，Linux 使用项目的进程树生命周期机制。
- 调试和验收 Flutter GUI 时，Flutter Driver 能覆盖的交互必须使用 Flutter Driver，不使用 Computer Use。只有 Driver 无法覆盖且确有必要时，才可使用 Computer Use，以减少对用户鼠标、键盘、窗口焦点和桌面状态的影响。

### Web GUI 远程验收

- `cargo xtask verify-gui --web-integration` 使用纯 Dart demo 和同一套 Flutter integration test 在无头 Chrome/Chromium 中验收布局与交互；它不替代原生 bridge、文件系统、进程、MCP/LSP 或真实 provider 验收。
- 首次使用先运行 `cargo flutter config --enable-web`，并安装主版本匹配的 Chrome/Chromium 与 ChromeDriver；浏览器不在 PATH 时设置 `CHROME_EXECUTABLE`。xtask 负责发现版本、解析可执行 wrapper/sandbox 载荷、分配临时端口、启动及回收 driver 进程树，失败日志写入 `code/pure-studio/build/web-integration-artifacts`。
- canonical Web 交互使用 Flutter integration test 与稳定 `ValueKey`；Playwright 等工具只能补充截图、console 或可访问性观察，不能维护第二套 DOM/坐标状态机。

### GUI 生成文件

- Riverpod、Freezed、l10n 和 FRB 文件属于生成边界，只能从仓库根目录统一生成：

  ```powershell
  cargo xtask generate-gui
  ```

- 不得手工修改生成文件，也不得直接调用单个生成器。
- 修改生成输入后运行 `cargo xtask check-gui-generated` 检查 canonical 输出，并分开审查生成 diff 与手写 diff。已授权的代码修改或生成一致性验证包含这些命令所需的生成操作，不为常规生成另行确认；用户明确限定只读时，不运行会改写跟踪文件的命令，报告未验证项。
- 普通 `run-gui` 和 `build-gui` 只消费当前源码，不运行生成器或可写格式化；修改生成输入后必须先显式运行 `cargo xtask generate-gui`。
- `check-gui-generated`、`verify-gui` 和 `build-gui --check-generated` 会重新生成并检查 canonical 输出，适用于提交前、CI 和发布门禁。
- Git 索引与普通源码统一使用 LF；PowerShell、RC 和 Inno Setup 文件使用 CRLF，`.gitattributes` 与 `.editorconfig` 必须保持一致。不得通过全局 Git 配置、构建后索引刷新或全仓 renormalize 修复行尾。

### 预置系统技能

- `pl-studio-runtime` 的上游预置技能（`canvas-design`、`frontend-design` 来自 anthropics/skills；`docx`、`pdf`、`powerpoint`、`xlsx` 来自 NousResearch/hermes-agent）只能通过仓库根目录的命令同步：

  ```powershell
  cargo xtask sync-skills
  ```

- 命令浅拉取各上游默认分支最新提交到 `target/xtask-sync-skills/` 缓存，完全替换 `code/pl-studio-runtime/assets/skills/` 下同名技能目录，替换前校验 frontmatter。
- 同步结果必须提交进源码库；源码库是 canonical 内容，构建期不访问网络，也不使用 build.rs 下载上游内容。
- 同步后人工核对 `code/pl-studio-runtime/THIRD_PARTY_NOTICES.md` 中的 revision 与许可边界（上游许可必须允许再分发；anthropics/skills 的 `pdf`/`docx`/`pptx`/`xlsx` 为禁止再分发的专有许可，不得预置）。

## 工程边界

### 质量技能入口

通用设计、实现与测试规则由以下技能维护，本文件只规定项目约束。按任务语义选择，不以文件名或改动规模代替判断；多个边界同时涉及时组合使用，不把普通改动扩展为无关专项审计。

| 任务语义 | 技能 |
| --- | --- |
| Rust 实现、错误传播、重构与审查 | [rust-code-quality](.agents/skills/rust-code-quality/SKILL.md) |
| 公共接口、类型、错误、模块、依赖与协议 | [rust-api-design](.agents/skills/rust-api-design/SKILL.md) |
| 同步、异步取消、回调与资源生命周期 | [rust-concurrency-safety](.agents/skills/rust-concurrency-safety/SKILL.md) |
| unsafe、FFI、外部句柄与安全证明 | [rust-unsafe-safety](.agents/skills/rust-unsafe-safety/SKILL.md) |
| 测试设计、回归、层级、必要性与去重 | [test-quality](.agents/skills/test-quality/SKILL.md) |

质量技能以 TGOSKits `4713bfc98f` 为迁移基线，经泛化后在本仓库独立维护；运行任务时不依赖源仓库。命令、授权与项目架构以本文件为准；技能负责相应方法与证明，不复制另一套审批或门禁。

### Rust 异步接口约定

- 禁止使用 `#[async_trait]` 和 `#[allow(async_fn_in_trait)]`。
- 异步 trait 使用原生 RPITIT，在返回 future 上显式声明 `Send`；不通过宏隐藏生命周期或线程安全约束。

### 核心 crate

- `pl-core` 是产品无关的 Thread 内核，拥有上下文、模型/工具端口、通用状态与可选冷存储；不依赖 `pl-protocol`、`pl-model`、`pl-tool`、`pl-trace`、`pl-output` 或 Studio，包括测试依赖。
- 跨产品和适配器的 wire 协议放在 `pl-protocol`；通用 Thread、Model、Tool、context 与 storage 契约由 `pl-core` 独立定义。
- Provider 适配、模型配置值对象、元数据和 stream 归一化放在 `pl-model`；具体工具与物理服务放在 `pl-tool`。二者实现并依赖 core 契约，不相互依赖。
- 向 `pl-core` 添加概念前先确认它属于核心编排；否则归属 `pl-model`、`pl-tool` 或 `pl-studio-runtime`；不得借 core 门面镜像产品协议。Studio 独占配置持久化、Mode/Plan/workflow、子代理协调及 root/child/恢复装配。
- OpenAI-compatible、Zhipu-compatible 等现役 provider 适配不是 legacy 层，不得误删；只清理真正废弃的 wrapper。

### Flutter、Bridge 与 wire

- Dart reducer 和 projection 优先消费 typed DTO/union，不新增 raw JSON 运行期兼容入口。
- Settings/GUI 状态更新必须以 bridge 返回的 canonical snapshot 或事件流为准，不能只修改本地 draft。
- 序列化类型统一使用 `#[serde(rename_all = "camelCase")]`；Rust 字段保持 `snake_case`。
- ID 使用 `String`，内部按需解析 UUID；时间戳使用 Unix 秒 `i64`，字段命名为 `*_at`。
- 结构化协议变更必须同步 Rust protocol、FRB DTO、Dart domain model、reducer/projection 和设计文档。
- 前端测试、浏览器验证和临时 dev server 不得占用 `1420` 端口，该端口保留给用户脚本。

## 测试、检查与交付

- 验证按实际影响选择。纯文档、技能或协作规则修改（包括提交前）只检查内容、引用、规则一致性及适用格式，不运行 Rust、Flutter、GUI 或 live 全量验收；若同时改变构建／生成输入、运行配置或测试执行语义，则按这些实际影响验证。代码修改先运行定向检查，跨 crate、协议、构建或依赖变更扩展到相关消费者；提交前执行下述代码门禁，生成输入、GUI 行为与 live 验收另遵守对应要求。
- 适用检查通过且需求已满足后直接交付；只有新修改、失败或尚未解决的具体风险才扩展或重复验证。只读分析以结论与出处为完成条件；修改任务以范围内修改完成、适用验证通过和差异可审查为完成条件；提 PR 任务还须交付 PR 链接及实际 CI 状态，排队或运行中不算通过。环境或权限阻塞时报告已完成项、原始失败和剩余工作，不声称全部完成。
- 测试设计、布局、必要性与确定性回归统一遵循 [test-quality](.agents/skills/test-quality/SKILL.md)。核心行为必须有充分证明；修复优先复用或增强已有功能测试，同一回归在错误实现失败、修复后通过，不要求每个 bug 新增用例。
- `code/pure-studio/pubspec.lock` 是必须纳入 Git 的 canonical 应用依赖快照，不得加入 ignore；
  Flutter 直接依赖升级后必须同步提交重新解析的 lockfile。
- 代码、构建、依赖或生成输入变更提交前，在本地执行与 CI 一致的检查清单（纯说明性变更按本节首条豁免；只需保证当前环境通过；
  `PUB_HOSTED_URL` 镜像导致的已跟踪 pubspec.lock hosted URL 差异由 xtask 自动
  规范化为 pub.dev canonical，无需手工处理）：

  ```powershell
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  cargo xtask verify-gui
  ```

- Linux 上运行未启用 `embedded-remote-helpers` 的 Studio/Server 或工作区测试前，先执行
  `cargo install --path code/pl-remote-helper --locked`，确保同一生产进程 worker 位于 PATH。
  桌面 xtask 构建会嵌入 worker；两种宿主均不使用裸 shell 后备路径。

- 不默认启用 `--all-features`：`live-tests` 等 feature 依赖外部服务与有效
  API key，需要时以 `cargo test -p pl-model --features live-tests` 等显式
  opt-in 执行，CI 与本地默认检查都不包含。
- CI（PR Quality Gate）只运行上述确定性检查，外加 Conventional PR 标题与发布
  配置校验；Flutter Driver smoke、任务流 harness 与 live 模型验收不在 CI 中
  运行——涉及 GUI 行为改动时，交付前本地执行 `cargo xtask verify-gui
  --integration`，交互验收使用 `cargo xtask run-gui --demo --driver` 与对应
  harness。
- `cargo doc --workspace --all-features --no-deps` 为按需检查项，CI 不执行。

- Flutter、Dart、GUI 和生成文件检查统一使用前文定义的项目命令入口；验收时结合 widget tree、窗口截图和日志判断结果。
- 交付前运行 `git diff --check`，确认无意外生成文件、无范围外改动，并在总结中列出实际执行的测试、未执行项及原因。
