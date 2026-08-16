# Pure-Lang Agent 协作规范

本文件是本仓库面向 Codex、Claude 和其他 AI agent 的唯一项目级协作规范。与用户交流、计划、总结和 PR 描述默认使用中文。

## 协作与变更原则

- 默认直接实现用户明确要求的修改；只有存在真实不确定、权限阻塞或文档冲突时才暂停确认。
- 修改前先理解现有边界和模式，优先沿用仓库已有抽象、命名和测试方式。
- 不回滚用户或其他 agent 的未提交改动；同一文件存在相关改动时，先读懂再叠加。
- 涉及架构、接口、协议、运行时行为或长期约定时，先更新 `design/*`，实现后再核对文档与代码一致。
- 设计文档只描述架构、接口、行为和约定；具体实现细节留在代码、类型签名和必要注释中。
- 文档与需求或可行方案冲突时，暂停并请用户选择：遵循现有文档、采用新方案并同步文档，或继续补充需求。
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

### Windows GUI

- Windows GUI 构建和运行必须从仓库根目录通过 xtask：

  ```powershell
  cargo xtask run-gui [--demo] [--driver]
  cargo xtask build-gui [--demo] [--no-clean] [--check-generated]
  ```

- 不支持直接执行 `flutter build windows` 或 `flutter run -d windows`，也不新增 PowerShell GUI wrapper。
- `cargo xtask run-gui --driver` 使用 `test_driver/driver_main.dart` 启用 Flutter Driver extension，供 Dart MCP 的 `flutter_driver_command` 操作 GUI；xtask 不负责启动实验性的 `dart mcp-server`。
- GUI smoke 使用 `cargo xtask run-gui --demo`；需要确定性数据和交互验收时使用 `cargo xtask run-gui --demo --driver`。
- Driver 命令结束后，其 Flutter、DTD 和 GUI 子进程必须随 Windows Job Object 一起退出，不得残留。
- 调试和验收 Flutter GUI 时，Flutter Driver 能覆盖的交互必须使用 Flutter Driver，不使用 Computer Use。只有 Driver 无法覆盖且确有必要时，才可使用 Computer Use，以减少对用户鼠标、键盘、窗口焦点和桌面状态的影响。

### GUI 生成文件

- Riverpod、Freezed、l10n 和 FRB 文件属于生成边界，只能从仓库根目录统一生成：

  ```powershell
  cargo xtask generate-gui
  ```

- 不得手工修改生成文件，也不得直接调用单个生成器。
- 修改生成输入后运行 `cargo xtask check-gui-generated` 检查 canonical 输出，并分开审查生成 diff 与手写 diff。
- `run-gui` 和 `build-gui` 会按内容指纹自动刷新过期输出；需要 CI 级构建检查时使用 `cargo xtask build-gui --check-generated`。

## 工程边界

### Rust 模块与文件

- 模块默认私有，只在 crate 根或明确边界使用 `pub use` 暴露稳定 API。
- Rust 模块统一使用 `foo/mod.rs` + `foo/child.rs`；不保留 `foo.rs` 作为目录模块入口。
- `lib.rs` 和 `mod.rs` 是目录页：先写职责，再声明模块并导出稳定入口；大量实现必须下沉到职责明确的子文件。
- 只有当某个模块的全部公共项都属于上层稳定 API 时，才使用 `pub use module::*`；不得用通配 re-export 暴露 `raw`、`imp`、`sys`、`unsafe_impl` 等内部细节。
- 文件和模块按变化原因拆分。单个生产模块目标不超过 500 行；超过约 800 行时必须拆分或说明其仍为单一职责。
- 源文件按阅读顺序组织：核心类型与公共入口在前，编排步骤向下展开，边界 helper 靠后，`#[cfg(test)] mod tests` 作为最后一个 item。
- `main.rs` 只负责参数解析、初始化、错误报告和调用库入口，不承载业务规则。

### Rust API 与实现

- 名字表达业务意图；类型和 trait 使用 `UpperCamelCase`，函数、变量和模块使用 `snake_case`，常量使用 `SCREAMING_SNAKE_CASE`。
- 编排函数只展示业务步骤，不夹杂底层解析、IO 或 unsafe 细节；不要为只调用一次且不能显著降低复杂度的逻辑创建 helper。
- 超过 3 个参数时优先考虑 options struct、builder、领域对象或拆分职责；禁止语义不清的 `bool` / `Option<bool>` 参数。既有 API 无法修改时，在调用点用参数名注释说明语义。
- 除非确实需要取得所有权，否则参数优先接收借用；用返回值、元组或结构体代替输出参数。
- 用 newtype、enum 和领域类型表达约束，让非法状态难以构造；封闭集合优先使用可穷尽匹配的 enum。
- struct 字段默认私有，通过构造函数和方法维护不变量；避免无必要的 clone、共享可变状态和 `Arc<Mutex<_>>`。
- 领域对象维护业务不变量，不直接承担数据库、HTTP、文件系统、硬件或 wire 格式转换；DTO、存储模型和领域对象必须分离。
- 第三方依赖类型只存在于 adapter/repository/runtime 等边界层，不污染核心领域 API。
- trait 必须小而专注；共享状态用组合，共享行为用 trait，封闭变化用 enum，静态多态用泛型，确需运行期开放扩展时才用 `dyn Trait`。
- 新增公共 trait 和公共 API 必须有 rustdoc，说明职责边界，以及适用的 `# Errors`、`# Panics`、`# Safety`；公共类型实现有意义的 `Debug`，文档示例优先用 `?` 而不是 `unwrap`。

### Rust 代码风格与安全

- 禁止使用 `#[async_trait]` 和 `#[allow(async_fn_in_trait)]`。
- 异步 trait 使用原生 RPITIT，并在返回 future 上显式声明 `Send`：

  ```rust
  pub trait Tool: Send + Sync {
      fn execute(
          &self,
          input: ToolInput,
      ) -> impl std::future::Future<Output = Result<ToolOutput>> + Send;
  }
  ```

- 领域 enum、协议消息和状态机使用穷尽 `match`，不得用 `_ => {}` 静默吞掉未来变体。
- 生产路径不得裸 `unwrap`；可恢复失败使用具体 `Result<T, E>`，不得用 `String`、`()` 或 panic 表达业务失败。错误包含操作、关键上下文和下层 source。测试可使用 `unwrap`，关键断言应提供清楚上下文。
- `format!` 使用内联变量；合并可折叠 `if`；优先方法引用，避免无意义闭包。
- JSON、TOML、YAML 等结构化数据使用 typed struct + serde 或现有解析库，不手写字符串拼接或解析。
- 锁作用域必须短；不得持锁执行 IO、`.await`、复杂计算或外部回调。异步运行时中的阻塞任务必须隔离。
- `unsafe` 块保持最小并由 safe API 包装；每个块说明安全不变量，`unsafe fn` 提供 `# Safety` 文档和边界测试。
- 优先使用函数、泛型、trait 和 derive；宏只能解决无法由这些机制清晰消除的重复，不得隐藏复杂控制流。
- 不把 `todo!()` 或 `unimplemented!()` 合入主分支。
- 新依赖需说明必要性、维护状态、license、公共 API 影响、编译/体积/安全成本和现有替代方案。

### 核心 crate

- `pl-core` 是核心编排层，组合 turn、session、model、store 和 tool runtime。
- 跨 crate 公共协议类型放在 `pl-protocol`。
- Provider 适配、模型元数据和 provider stream 归一化放在 `pl-model`。
- 向 `pl-core` 添加概念前先确认它属于核心编排；否则下沉到具体 crate 或提升到 `pl-protocol`。
- OpenAI-compatible、Zhipu-compatible 等现役 provider 适配不是 legacy 层，不得误删；只清理真正废弃的 wrapper。

### Flutter、Bridge 与 wire

- Dart reducer 和 projection 优先消费 typed DTO/union，不新增 raw JSON 运行期兼容入口。
- Settings/GUI 状态更新必须以 bridge 返回的 canonical snapshot 或事件流为准，不能只修改本地 draft。
- 序列化类型统一使用 `#[serde(rename_all = "camelCase")]`；Rust 字段保持 `snake_case`。
- ID 使用 `String`，内部按需解析 UUID；时间戳使用 Unix 秒 `i64`，字段命名为 `*_at`。
- 结构化协议变更必须同步 Rust protocol、FRB DTO、Dart domain model、reducer/projection 和设计文档。
- 前端测试、浏览器验证和临时 dev server 不得占用 `1420` 端口，该端口保留给用户脚本。

## 测试、检查与交付

- 每个核心业务规则必须有测试；每个 bug 修复必须增加能覆盖该问题的回归测试。
- 测试名称表达场景和期望；优先比较完整对象并使用 `pretty_assertions::assert_eq!` 获得清晰 diff。
- 测试 helper 只服务测试时放在测试模块或专用测试模块，不为测试方便扩大生产 API。
- 避免在测试中修改进程环境变量；确需修改时必须隔离并恢复。
- 提交前在本地执行与 CI 门禁一致的检查清单（只需保证当前环境通过；
  `PUB_HOSTED_URL` 镜像导致的 pubspec.lock hosted URL 差异由 xtask 自动
  规范化为 pub.dev canonical，无需手工处理）：

  ```powershell
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  cargo xtask verify-gui
  ```

- 不默认启用 `--all-features`：`live-tests` 等 feature 依赖外部服务与有效
  API key，需要时以 `cargo test -p pl-core --features live-tests` 等显式
  opt-in 执行，CI 与本地默认检查都不包含。
- CI（PR Quality Gate）只运行上述确定性检查，外加 Conventional PR 标题与发布
  配置校验；Flutter Driver smoke、任务流 harness 与 live 模型验收不在 CI 中
  运行——涉及 GUI 行为改动时，交付前本地执行 `cargo xtask verify-gui
  --integration`，交互验收使用 `cargo xtask run-gui --demo --driver` 与对应
  harness。
- `cargo doc --workspace --all-features --no-deps` 为按需检查项，CI 不执行。

- Flutter、Dart、GUI 和生成文件检查统一使用前文定义的项目命令入口；验收时结合 widget tree、窗口截图和日志判断结果。
- 交付前运行 `git diff --check`，确认无意外生成文件、无范围外改动，并在总结中列出实际执行的测试、未执行项及原因。
