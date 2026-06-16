# Pure-Lang 项目记忆

本文件记录适用于本仓库的 Codex/Agent 协作约定。修改代码时优先遵守这些规则，并保持与现有 Rust 代码风格一致。项目使用中文进行交流和文档编写。

## Crate 命名

- 所有 crate 名称以 `pl-` 为前缀，例如 `pl-core`、`pl-tool`、`pl-agent`。
- Slint 桌面二进制 crate 例外，使用 `pure-studio`。

## 异步 Trait

- 禁止使用 `#[async_trait]`。
- 禁止使用 `#[allow(async_fn_in_trait)]`。
- 定义异步 trait 方法时使用原生 RPITIT，并显式声明 `Send` bound。

```rust
pub trait Tool: Send + Sync {
    fn execute(&self, input: ToolInput)
        -> impl std::future::Future<Output = Result<ToolOutput>> + Send;
}
```

实现 trait 时可以使用 `async fn`，前提是满足 trait 声明中的 `Send` bound。

## 参数设计

- 避免无法从调用点理解含义的 `bool` 参数。
- 避免语义模糊的 `Option<bool>` 参数。
- 优先使用明确的 enum、options struct 或 newtype。

```rust
fn create_tool(danger_level: DangerLevel) -> Tool
fn search(query: &str, options: SearchOptions) -> Vec<Result>

struct CaseSensitive(bool);
fn search(query: &str, case: CaseSensitive) -> Vec<Result>
```

如果无法更改现有 API，在调用点用参数名注释说明含义：

```rust
tool.execute_stream(input, /*event_tx*/ sender)
```

## 模块和导出

- 模块默认私有。
- 通过 `pub use` 在 crate 根或明确边界处导出公开 API。

```rust
mod model;
mod error;
mod message;
mod permission;

pub use error::PureError;
pub use message::{Message, MessageContent, MessageRole};
pub use model::{AgentEvent, ModelInfo, ModelProvider, ProviderInfo};
pub use permission::PermissionLevel;
```

## 模块大小

- 单个模块目标控制在 500 行以内，不含测试。
- 超过约 800 行时，新功能应放入新模块。
- 高频修改文件要特别警惕继续膨胀。

## Trait 规范

所有新增 trait 必须包含文档注释，说明其角色，以及实现者应该如何使用或实现它。

```rust
/// LLM Provider 运行时抽象。
///
/// 封装认证、API 调用、能力查询等 provider 特定逻辑。
/// 每个 provider 实现此 trait，并通过工厂函数创建。
pub trait ModelProvider: Debug + Send + Sync {
    // ...
}
```

## Match 语句

- 优先使用穷尽匹配。
- 避免用 `_ => {}` 静默忽略其他变体。
- 当 enum 代表领域事件、状态或协议消息时，显式列出所有变体。

## 辅助方法

不要为了只调用一次的逻辑创建辅助方法。除非能显著降低复杂度或匹配现有抽象，否则保持代码局部性。

## `pl-core` 边界

`pl-core` 是核心逻辑层，负责组合 turn、session、model、store 等编译流程。
跨 crate 公共协议类型放在 `pl-protocol`，provider 适配和模型元数据放在 `pl-model`。
向 `pl-core` 添加新概念或功能前，先判断：

1. 是否属于核心编译流程。
2. 是否应下沉到 `pl-protocol` 或保留在更具体的 crate。

## 格式化偏好

- 始终内联 `format!` 变量：使用 `format!("{name}")`，不要写 `format!("{}", name)`。
- 合并可折叠的 `if` 语句。
- 优先使用方法引用而非闭包，例如 `.map(String::len)`，不要写 `.map(|s| s.len())`。

## 测试

- 使用 `pretty_assertions::assert_eq!` 获得更清晰的 diff。
- 优先比较完整对象，而不是逐字段比较。
- 避免在测试中修改进程环境变量。
- 前端测试、浏览器验证或临时 dev server 不要占用 `1420` 端口；该端口保留给用户脚本，避免端口冲突。需要启动 Vite/预览服务时使用其他空闲端口。

## API 边界

- 序列化类型统一使用 `#[serde(rename_all = "camelCase")]` 作为 wire 格式。
- Rust 侧字段命名保持 `snake_case`。
- ID 使用 `String` 类型，内部按需解析 UUID。
- 时间戳使用 Unix 秒整数，类型为 `i64`，命名为 `*_at`。

## 文档

- 设计文档描述架构和接口，不写具体实现细节。
- 除 trait 定义和类型签名外，具体实现应留在代码和必要注释中表达。

## 文档与实现同步流程

- 涉及架构、接口、行为或约定的修改，确认 plan 后必须先更新相关文档，再开始修改代码。
- 代码修改完成后，必须回看并更新对应文档，确保文档与实现一致。
- 如果现有文档未覆盖本次变更，应补充文档说明新的架构、接口或行为约定。
- 如果实现过程中发现文档与实际需求或可行方案冲突，必须暂停并向用户确认选择：
  1. 严格按照现有文档实现。
  2. 按照 Agent 提供的新方案实现，并同步修正文档。
  3. 由用户继续补充提示后再决定。
- 未获得确认前，不应擅自用代码实现覆盖文档中的明确设计约定。

## 提交前检查

提交或交付前，根据变更范围运行：

```powershell
cargo fmt
cargo clippy -- -D warnings
cargo test -p <crate>
```
