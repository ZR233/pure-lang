# Pure-Lang Agent 协作规范

本文件是本仓库面向 Codex、Claude 和其他 AI agent 的唯一项目级协作规范。与用户交流、计划、总结和 PR 描述默认使用中文。

## 工作方式

- 默认直接实现用户明确要求的修改；遇到真实不确定或文档冲突时再暂停确认。
- 修改代码前先理解现有边界和模式，优先沿用本仓库已有抽象、命名和测试方式。
- 不回滚用户或其他 agent 的未提交改动；如果同一文件里有相关改动，先读懂再叠加修改。
- 涉及架构、接口、协议、运行时行为或长期约定时，先更新 `design/*`，实现后再回看文档是否一致。
- 提 PR 或分支时，优先推送到用户账号 fork，并优先向 fork 源的默认分支提 PR。

## 项目结构

- Rust crate 名称统一以 `pl-` 开头，例如 `pl-core`、`pl-model`、`pl-studio-bridge`。
- Flutter app 的 Dart package 名称是 `pure_studio_flutter`，不按 Cargo crate 规则命名。
- Flutter 项目根目录是 `code/pure-studio-flutter`；运行 Flutter 命令必须在该目录下执行，或由 `xtask` 显式切换目录。
- 桌面 GUI 入口使用：
  - `cargo xtask run-gui [--demo] [--demo-fallback]`
  - `cargo xtask build-gui [--demo] [--no-clean]`
- 不再新增 PowerShell GUI wrapper；需要构建/运行 GUI 时优先使用 `xtask`。

## Rust 模块与边界

- 模块默认私有，只在 crate 根或明确边界用 `pub use` 导出稳定 API。
- 新增 Rust 目录模块使用 `文件夹 + mod.rs`，不要再新增“文件夹同名 `.rs` 作为模块入口”的结构。
- `mod.rs` 应主要作为目录页和稳定出口；大量实现下沉到职责明确的子文件。
- 单个模块目标控制在 500 行以内，不含测试；超过约 800 行时，新功能应拆到新模块。
- 高频修改文件要避免继续膨胀，优先按变化原因拆分，而不是按技术细节机械拆散。
- 测试模块放在源文件最后；测试 helper 若只服务测试，应放在测试模块内部或测试专用模块。

## Rust 代码风格

- 禁止使用 `#[async_trait]`。
- 禁止使用 `#[allow(async_fn_in_trait)]`。
- 异步 trait 使用原生 RPITIT，并在 trait 方法返回类型上显式声明 `Send` bound：

```rust
pub trait Tool: Send + Sync {
    fn execute(&self, input: ToolInput)
        -> impl std::future::Future<Output = Result<ToolOutput>> + Send;
}
```

- 避免语义不清的 `bool` 或 `Option<bool>` 参数；优先使用 enum、options struct 或 newtype。
- 如果无法改变既有 API，在调用点用参数名注释说明含义，例如 `tool.execute(input, /*event_tx*/ sender)`。
- 新增 trait 必须有文档注释，说明角色、边界和实现者应如何实现。
- 领域 enum、协议消息和状态机优先使用穷尽 `match`；不要用 `_ => {}` 静默吞掉未来变体。
- 不要为了只调用一次的逻辑创建 helper，除非它显著降低复杂度或符合已有抽象。
- 生产路径不要裸 `unwrap`；测试中可用 `unwrap`，但关键断言优先给出清楚错误上下文。
- `format!` 使用内联变量：`format!("{name}")`，不要写 `format!("{}", name)`。
- 合并可折叠 `if`，优先使用方法引用而不是无意义闭包，例如 `.map(String::len)`。
- JSON、TOML、YAML 等结构化数据优先用 typed struct + serde 或现有解析库处理，不手写字符串拼接/解析。

## 核心 crate 边界

- `pl-core` 是核心编排层，负责组合 turn、session、model、store、tool runtime 等流程。
- 跨 crate 公共协议类型放在 `pl-protocol`。
- Provider 适配、模型元数据和 provider stream 归一化放在 `pl-model`。
- 向 `pl-core` 添加新概念前，先判断它是否属于核心编排；否则下沉到更具体 crate 或提升到 `pl-protocol`。
- OpenAI-compatible、Zhipu-compatible 等现役 provider 适配不是“过时兼容层”，不要误删；真正 legacy wrapper 才应清理。

## Flutter 与 Bridge

- FRB 生成文件属于生成边界；改动 Rust DTO/handler surface 后优先运行 `flutter_rust_bridge_codegen generate`，并把生成 diff 与手写 diff 分开审查。
- Dart reducer 和 projection 优先消费 typed DTO/union，不新增 raw JSON 运行期兼容入口。
- Settings/GUI 状态更新必须以 bridge 返回的 canonical snapshot 或事件流为准，不只改本地 draft。
- 前端测试、浏览器验证或临时 dev server 不要占用 `1420` 端口，该端口保留给用户脚本。

## API 与 Wire 格式

- 序列化类型统一使用 `#[serde(rename_all = "camelCase")]` 作为 wire 格式。
- Rust 字段保持 `snake_case`。
- ID 使用 `String`；内部按需解析 UUID。
- 时间戳使用 Unix 秒整数，类型为 `i64`，字段命名为 `*_at`。
- 结构化协议变化要同步 Rust protocol、FRB DTO、Dart domain model、reducer/projection 和设计文档。

## 文档

- 设计文档描述架构、接口、行为和约定，不写过细实现步骤。
- 具体实现细节放在代码、类型签名和必要注释里。
- 如果实现过程中发现文档与需求或可行方案冲突，先暂停并向用户确认：
  1. 严格按现有文档实现。
  2. 按 agent 提供的新方案实现，并同步修正文档。
  3. 用户继续补充提示后再决定。

## 测试与验证

- 使用 `pretty_assertions::assert_eq!` 获得更清晰 diff。
- 优先比较完整对象，而不是逐字段拼断言。
- 避免在测试中修改进程环境变量；确需修改时必须隔离并恢复。
- 改动后按范围运行验证。常用命令：

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p <crate>
flutter analyze
flutter test
```

- Flutter 命令必须在 `code/pure-studio-flutter` 目录执行。
- GUI smoke 优先使用 `cargo xtask run-gui --demo`，再结合窗口截图或日志确认不是白屏/崩溃。

## 代码质量检查清单

- 名字是否表达业务意图，而不是实现细节。
- 文件开头是否能看到主要入口或核心类型。
- 编排函数是否像目录一样列出清楚步骤。
- 模块、结构体和 trait 是否按职责拆分，没有继续扩大大文件或上帝对象。
- 公共 API 是否隐藏实现细节，错误是否可诊断。
- 是否避免不必要 clone、共享可变状态和 `Arc<Mutex<_>>`。
- 是否用类型表达关键约束，而不是注释或 bool 标志。
- 是否没有新增运行期 legacy wrapper 或过时兼容路径。
- 是否有 focused tests 覆盖本次行为变化。
