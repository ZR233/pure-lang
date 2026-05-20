# 09 - 编码规范与设计约束

> 基于 Codex `AGENTS.md` 提炼的项目级规范，约束设计文档中所有 trait 和类型定义。

## 9.1 关键规范与设计文档偏差修正

### 规范 1：禁止 `#[async_trait]`，使用原生 RPITIT

**偏差文档**：02-crates, 03-pipeline, 07-model, 08-streaming

所有设计文档中的 trait 定义使用了 `#[async_trait]`。修正为：

```rust
// 02-crates.md 修正
// 旧：
#[async_trait]
pub trait Tool: Send + Sync {
    async fn execute_stream(...) -> Result<ToolResult>;
}

// 新：
pub trait Tool: Send + Sync {
    fn execute_stream(
        &self,
        input: ToolInput,
        ctx: &ToolExecutionContext,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<ToolResult>> + Send;
}
```

影响的 trait 列表：

| 文档 | Trait | 修正 |
|------|-------|------|
| 02-crates | `Tool` | `async fn` → `fn ... -> impl Future<...> + Send` |
| 02-crates | `MemoryStore` | 同上 |
| 02-crates | `Runtime` | 同上 |
| 02-crates | `Compiler` | 同上 |
| 02-crates | `Agent` | 同上 |
| 07-model | `ModelProvider` | 同上 |
| 07-model | `CompletionStream` | 同上 |
| 07-model | `WireAdapter` | 同上 |
| 07-model | `ModelsManager` | 同上 |
| 07-model | `ModelsEndpointClient` | 同上 |
| 08-streaming | `Tool` | 同上 |
| 08-streaming | `Runtime` | 同上 |
| 08-streaming | `Compiler` | 同上 |
| 08-streaming | `Agent` | 同上 |

### 规范 2：避免 bool / 模糊 Option 参数

**偏差文档**：02-crates, 07-model, 08-streaming

| 位置 | 偏差 | 修正 |
|------|------|------|
| 07-model `ModelInfo::supports_streaming: bool` | 单个 bool 能力标记 | 改为 `capabilities: ModelCapabilities` 位标志集 |
| 07-model `ModelInfo::supports_vision: bool` | 同上 | 合并到 `ModelCapabilities` |
| 07-model `CompletionRequest::stream: bool` | wire 层当前固定构造流式请求 | 保留字段，调用点不使用裸 bool 参数 |
| 08-streaming `AgentEvent::Error::recoverable: bool` | 模糊 bool | 改为 `severity: ErrorSeverity` 枚举 |
| 04-security `ExecutionResult::timed_out: bool` | 模糊 bool | 改为 `ExitStatus` 枚举 |

**ModelCapabilities 设计**：

```rust
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct ModelCapabilities: u32 {
        const STREAMING             = 0b00000001;
        const FUNCTION_CALLING      = 0b00000010;
        const VISION                = 0b00000100;
        const PARALLEL_TOOL_CALLS   = 0b00001000;
        const REASONING             = 0b00010000;
        const WEB_SEARCH            = 0b00100000;
    }
}
```

**ExitStatus 设计**：

```rust
pub enum ExitStatus {
    Exited(i32),
    TimedOut,
    Signaled(i32),
}
```

### 规范 3：私有模块 + 显式导出

**偏差文档**：02-crates, 07-model

当前设计文档中模块结构已基本遵循，但需要强调：

```
pl-core/src/
├── lib.rs              # 仅 pub use，不含实现
├── model/
│   ├── mod.rs          # 仅 pub use
│   ├── provider.rs     # impl 不导出，只导出 trait
│   ├── model_info.rs
│   ├── provider_info.rs
│   ├── manager.rs
│   ├── auth.rs
│   ├── wire_api.rs
│   └── sse.rs
├── event.rs            # AgentEvent
├── error.rs            # PureError
├── message.rs
└── permission.rs
```

### 规范 4：Trait 文档注释

**偏差文档**：所有包含 trait 定义的文档

每个 trait 必须包含：
1. 一句话说明角色
2. 实现者应遵循的契约
3. 关键 invariants

示例（已修正）：

```rust
/// 工具执行器抽象。
///
/// 定义了一个可被 Agent 调用的原子操作能力。
/// 工具注册到 ToolRegistry 后，Agent 在 ReAct 循环中通过名称查找并调用。
///
/// 实现者契约：
/// - `definition()` 必须返回静态或缓存的定义（每次调用不应重新分配）
/// - `execute_stream()` 通过 event_tx 推送进度，最终返回 ToolResult
/// - 长时间运行的工具应定期推送 ToolOutputDelta 防止超时
pub trait Tool: Send + Sync {
    fn definition(&self) -> &ToolDefinition;
    fn execute_stream(
        &self,
        input: ToolInput,
        ctx: &ToolExecutionContext,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<ToolResult>> + Send;
}
```

### 规范 5：不要创建只调用一次的辅助方法

**偏差文档**：03-pipeline, 08-streaming

以下辅助方法被提取但没有复用价值，应在设计中删除或标注为内联：

| 位置 | 方法 | 处理 |
|------|------|------|
| 03-pipeline `find_model_by_longest_prefix()` | 参考自 Codex | 保留（Codex 中有复用） |
| 08-streaming `truncated_output` | ExecuteTool 中的变量 | 内联，不提取 |
| 07-model `default_model_from_available()` | 参考自 Codex | 保留 |

### 规范 6：不向 pl-core 无节制添加代码

**偏差文档**：07-model 将 model 层放在 pl-core 中

重新评估：model 层随着 Provider 实现、SSE 解析、WireAdapter 等代码增长，可能很快膨胀。

**修正方案**：将 model 层独立为 `pl-model` crate：

```
code/
├── pl-core/          # 共享类型（PureError, Message, PermissionLevel, AgentEvent）
├── pl-model/         # Model 层（ProviderInfo, ModelProvider, SSE, WireAdapter）
├── pl-tool/
├── pl-skill/
├── pl-memory/
├── pl-runtime/
├── pl-compiler/
├── pl-agent/
├── pl-cli/
└── pure-lang/
```

依赖关系更新：

```
pl-core (无内部依赖)
    ↑
pl-model (依赖 pl-core)
    ↑
pl-tool, pl-memory, pl-runtime (各依赖 pl-core)
    ↑
pl-compiler, pl-skill (依赖 pl-core + 各自的特定依赖)
    ↑
pl-agent (依赖所有)
    ↑
pl-cli → pure-lang
```

---

## 9.2 完整的 trait 签名修正

以下是所有 trait 的修正后签名（合并 02-crates + 07-model + 08-streaming 的修正）：

### pl-core::AgentEvent

（无 trait，仅为枚举。保持 08-streaming 中的定义不变。）

### pl-model::ModelProvider

```rust
/// LLM Provider 运行时抽象。
///
/// 封装认证、API 调用、能力查询等 provider 特定逻辑。
/// 通过工厂函数 `create_provider()` 创建。
///
/// 实现者契约：
/// - 通过 event_tx 推送 LLM 输出增量（TextDelta/ThinkingDelta/ToolCallDelta）
/// - capabilities() 如实报告支持的功能
/// - auth_token() 返回当前有效的认证凭据
pub trait ModelProvider: Debug + Send + Sync {
    fn info(&self) -> &ProviderInfo;
    fn capabilities(&self) -> ProviderCapabilities;

    fn stream_complete(
        &self,
        request: CompletionRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompletionResponse>> + Send;

    fn auth_token(
        &self,
    ) -> impl std::future::Future<Output = Result<Option<String>>> + Send;

    fn model_info(&self, model: &str) -> ModelInfo;
    fn default_model(&self) -> &str;
}
```

### pl-model::WireAdapter

```rust
/// API 协议适配器。
///
/// 将内部统一的 CompletionRequest 转换为不同 provider 的 wire 格式，
/// 并将 provider 返回的响应解析回 CompletionResponse。
///
/// 实现者契约：
/// - build_request_body() 产生的 JSON 必须符合目标 API 规范
/// - parse_stream_event() 处理单个 SSE 事件，返回 None 表示跳过
pub trait WireAdapter: Send + Sync {
    fn build_request_body(&self, request: &CompletionRequest) -> serde_json::Value;
    fn parse_response(&self, body: serde_json::Value) -> Result<CompletionResponse>;
    fn parse_stream_event(
        &self,
        event: &SseStreamEvent,
    ) -> Result<Option<StreamEvent>>;
}
```

### pl-model::ModelsManager

```rust
/// 模型元数据管理。
///
/// 当前实现通过 provider 查询单个模型，并通过内置 default_models 列出默认模型。
///
/// 实现者契约：
/// - model_info() 对未知模型返回 fallback 元数据
/// - list_models() 使用 default_model_slugs()
pub trait ModelsManager: Send + Sync {
    fn model_info(&self, slug: &str) -> ModelInfo;
    fn list_models(&self) -> Vec<ModelInfo>;
    fn default_model(&self) -> &str;
}
```

### pl-tool::Tool

```rust
/// 工具执行器抽象。
///
/// 定义一个可被 Agent 调用的原子操作能力。
/// 注册到 ToolRegistry 后，Agent 在 ReAct 循环中查找并调用。
///
/// 实现者契约：
/// - definition() 返回静态或缓存的定义
/// - execute_stream() 通过 event_tx 推送进度，最终返回 ToolResult
/// - 长时间运行的工具应定期推送 ToolOutputDelta 防止超时
pub trait Tool: Send + Sync {
    fn definition(&self) -> &ToolDefinition;

    fn execute_stream(
        &self,
        input: ToolInput,
        ctx: &ToolExecutionContext,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<ToolResult>> + Send;

    fn danger_level(&self) -> DangerLevel {
        self.definition().danger_level
    }
}
```

### pl-memory::MemoryStore

```rust
/// 记忆存储后端抽象。
///
/// 支持短/中/长三层记忆的存储、检索和删除。
///
/// 实现者契约：
/// - store() 返回的 MemoryId 必须全局唯一
/// - retrieve() 按 time_range 过滤时应包含边界
/// - delete() 对不存在的 id 应静默返回 Ok(())
pub trait MemoryStore: Send + Sync {
    fn store(
        &self,
        entry: MemoryEntry,
    ) -> impl std::future::Future<Output = Result<MemoryId>> + Send;

    fn retrieve(
        &self,
        query: &MemoryQuery,
    ) -> impl std::future::Future<Output = Result<Vec<MemoryEntry>>> + Send;

    fn delete(
        &self,
        id: &MemoryId,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn search(
        &self,
        keywords: &[&str],
        limit: usize,
    ) -> impl std::future::Future<Output = Result<Vec<MemoryEntry>>> + Send;
}
```

### pl-runtime::Runtime

```rust
/// 沙箱执行环境抽象。
///
/// 所有 LLM 生成的命令在受限沙箱中执行。
/// 通过 ProcessOutput channel 流式推送 stdout/stderr。
///
/// 实现者契约：
/// - execute_stream() 必须尊重 SandboxConstraints 中的所有限制
/// - 超时时终止子进程并设置 timed_out 标志
/// - 不允许子进程继承父进程的环境变量
pub trait Runtime: Send + Sync {
    fn execute_stream(
        &self,
        request: ExecutionRequest,
        output_tx: Sender<ProcessOutput>,
    ) -> impl std::future::Future<Output = Result<ProcessResult>> + Send;

    fn is_available(&self) -> bool;
    fn sandbox_type(&self) -> SandboxType;
}
```

### pl-compiler::Compiler

```rust
/// 自然语言编译管线抽象。
///
/// 将 NL 输入通过多阶段管线转换为代码产物。
/// 通过 AgentEvent stream 推送各阶段进度。
///
/// 实现者契约：
/// - 管线阶段按 IntentAnalysis → Planning → CodeGeneration → Verification → Integration 顺序推送事件
/// - 每个阶段开始推送 PipelineStageStarted，结束推送 PipelineStageDone
/// - 规划阶段应逐个推送 PlanTaskAdded，而非等待完整计划
pub trait Compiler: Send + Sync {
    fn compile_stream(
        &self,
        input: CompileInput,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompileSummary>> + Send;
}
```

### pl-agent::Agent

```rust
/// Agent 推理-行动循环抽象。
///
/// 驱动整个编译和执行过程。通过 AgentEvent stream 推送所有中间状态。
///
/// 实现者契约：
/// - handle_input() 通过 event_tx 推送从 TurnStarted 到 Done 的完整事件流
/// - interrupt() 应终止当前正在执行的 LLM 请求和工具调用
/// - 所有事件推送使用 try_send 避免因消费者慢而阻塞 Agent
pub trait Agent: Send + Sync {
    fn handle_input(
        &self,
        input: AgentInput,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<AgentSummary>> + Send;

    fn interrupt(&self) -> impl std::future::Future<Output = Result<()>> + Send;
    fn status(&self) -> AgentStatus;
}
```

---

## 9.3 文档编号与总览更新

### 修正后的文档结构

```
design/
├── 01-overview.md        系统总览（需更新 crate 列表：添加 pl-model）
├── 02-crates.md          Crate 详细设计（需更新 trait 签名）
├── 03-pipeline.md        编译管线（需更新为流式管线）
├── 04-security.md        安全模型（需更新 ExecutionResult）
├── 05-extension.md       扩展点（需更新工具示例）
├── 06-phases.md          实施阶段（需更新 crate 列表和优先级）
├── 07-model.md           Model 层（需更新 trait 签名、删除 async_trait）
├── 08-streaming.md       全流式架构（需更新 trait 签名）
└── 09-conventions.md     编码规范与设计约束（本文档）
```

### 关键修正汇总

| 编号 | 修正项 | 影响文档 |
|------|--------|---------|
| R1 | 禁止 `#[async_trait]`，使用 RPITIT + Send | 02, 03, 07, 08 |
| R2 | bool → 枚举/位标志 | 02, 07 |
| R3 | model 层独立为 `pl-model` crate | 01, 02, 06 |
| R4 | Trait 添加文档注释 | 02, 07, 08 |
| R5 | 私有模块 + 显式导出 | 02, 07 |
| R6 | 穷尽 match | 08 |
| R7 | 不向 core 膨胀 | 02, 07 |
