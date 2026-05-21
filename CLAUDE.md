# Pure-Lang 项目规范

> 参考 OpenAI Codex (`codex-rs/AGENTS.md`) 的项目规范，适用于本项目的 Rust 编码和设计标准。

## Crate 命名

- 所有 crate 名称以 `pl-` 为前缀（如 `pl-core`、`pl-tool`、`pl-agent`）
- 二进制 crate 例外：`purec`

## 异步 Trait（禁止 `#[async_trait]`）

禁止使用 `#[async_trait]` 和 `#[allow(async_fn_in_trait)]`。

使用原生 RPITIT，显式声明 `Send` bound：

```rust
// 禁止
#[async_trait]
pub trait Tool: Send + Sync {
    async fn execute(&self, input: ToolInput) -> Result<ToolOutput>;
}

// 正确
pub trait Tool: Send + Sync {
    fn execute(&self, input: ToolInput)
        -> impl std::future::Future<Output = Result<ToolOutput>> + Send;
}
```

实现可以使用 `async fn`（当满足 Send bound 时）：

```rust
impl Tool for ReadFileTool {
    async fn execute(&self, input: ToolInput) -> Result<ToolOutput> {
        // ...
    }
}
```

## 避免 bool / 模糊 Option 参数

禁止使用无法从调用点理解含义的参数。

```rust
// 禁止
fn create_tool(dangerous: bool) -> Tool
fn search(query: &str, case_sensitive: Option<bool>) -> Vec<Result>

// 正确
fn create_tool(danger_level: DangerLevel) -> Tool
fn search(query: &str, options: SearchOptions) -> Vec<Result>

// 或使用 newtype
struct CaseSensitive(bool);
fn search(query: &str, case: CaseSensitive) -> Vec<Result>
```

如果无法更改 API，使用 `/*param_name*/` 注释：

```rust
tool.execute_stream(input, /*event_tx*/ sender)
```

## 私有模块 + 显式导出

模块默认私有，通过 `pub use` 显式导出公开 API：

```rust
// pl-core/src/lib.rs
mod model;
mod error;
mod message;
mod permission;

pub use error::PureError;
pub use message::{Message, MessageRole, MessageContent};
pub use permission::PermissionLevel;
pub use model::{ModelInfo, ProviderInfo, ModelProvider, AgentEvent};
```

## 模块大小限制

- 目标：单个模块 < 500 行代码（不含测试）
- 超过 ~800 行时，新功能必须放入新模块
- 高频修改文件需要额外警惕膨胀

## Trait 规范

所有新增 trait 必须包含文档注释，说明其角色和实现者应如何使用：

```rust
/// LLM Provider 运行时抽象。
///
/// 封装了认证、API 调用、能力查询等 provider 特定逻辑。
/// 每个 provider 实现此 trait，通过工厂函数创建。
///
/// 实现者应：
/// - 使用 `ProviderInfo` 配置连接参数
/// - 通过 `AgentEventSender` 推送流式输出
/// - 在 `capabilities()` 中如实报告支持的功能
pub trait ModelProvider: Debug + Send + Sync {
    // ...
}
```

## match 语句

优先使用穷尽匹配，避免通配符分支：

```rust
// 禁止
match event {
    AgentEvent::TextDelta { .. } => { ... }
    _ => {}  // 静默忽略其他变体
}

// 正确：显式列出所有变体
match event {
    AgentEvent::TextDelta { content } => { ... }
    AgentEvent::ThinkingDelta { content } => { ... }
    AgentEvent::ToolCallDelta { .. } => { ... }
    // ... 其他变体
}
```

## 不要创建只调用一次的辅助方法

如果一个方法只在一个地方被引用，不要提取它。保持代码局部性。

## pl-core 核心边界

`pl-core` 是核心逻辑层，负责组合 turn、session、model、store 等编译流程。
跨 crate 公共协议类型放在 `pl-protocol`，provider 适配和模型元数据放在 `pl-model`。
添加新概念/功能前，先考虑：

1. 是否属于核心编译流程？
2. 是否应下沉到 `pl-protocol` 或保留在更具体的 crate？

## 格式化

- 始终内联 `format!` 变量：`format!("{name}")` 而非 `format!("{}", name)`
- 合并可折叠的 `if` 语句
- 优先使用方法引用而非闭包：`.map(String::len)` 而非 `.map(|s| s.len())`

## 测试

- 使用 `pretty_assertions::assert_eq!` 以获得更清晰的 diff
- 优先比较完整对象，而非逐字段比较
- 避免在测试中修改进程环境变量

## 纯净的 API 边界

序列化类型统一使用 `#[serde(rename_all = "camelCase")]`（wire 格式），Rust 侧保持 snake_case。

ID 使用 `String` 类型（内部按需做 UUID 解析）。
时间戳使用整数 Unix 秒（`i64`），命名 `*_at`。

## 不要在文档中写实现细节

设计文档描述架构和接口，不写具体实现代码（除 trait 定义和类型签名）。
实现细节应通过代码和注释表达。

## 代码提交前

1. `cargo fmt`
2. `cargo clippy -- -D warnings`
3. 运行变更 crate 的测试：`cargo test -p <crate>`
