# 02 - 各 Crate 详细设计

## 2.1 pl-core — 核心抽象与共享类型

**职责**：定义所有其他 crate 共享的核心类型、错误类型、事件定义。不包含具体实现逻辑，不包含 LLM 相关类型。

**依赖**：无内部依赖，是整个依赖图的最底层。

**外部依赖**：`serde`, `serde_json`, `thiserror`, `tokio`（broadcast channel）, `tracing`

### 关键类型

```rust
// 错误类型
#[derive(Debug, thiserror::Error)]
pub enum PureError {
    #[error("LLM provider error: {0}")]
    LlmError(String),

    #[error("Context window exceeded: {0} tokens")]
    ContextOverflow(usize),

    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Tool execution failed: {tool}: {error}")]
    ToolExecutionFailed { tool: String, error: String },

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Sandbox error: {0}")]
    SandboxError(String),

    #[error("Memory store error: {0}")]
    MemoryError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

### AgentEvent（统一事件流）

```rust
use tokio::sync::broadcast;

/// AgentEventSender 是 broadcast channel 的发送端。
/// 所有子系统通过它推送实时进度事件。
pub type AgentEventSender = broadcast::Sender<AgentEvent>;

pub enum AgentEvent {
    // LLM 输出
    TextDelta { content: String },
    ThinkingDelta { content: String },
    ToolCallDelta { id: String, name: String, arguments_delta: String },

    // 工具执行
    ToolCallStarted { id: String, name: String, input: serde_json::Value },
    ToolOutputDelta { id: String, content: String },
    ToolCallCompleted { id: String, result: ToolResult },

    // 管线阶段
    PipelineStageStarted { stage: PipelineStage },
    PipelineStageDone { stage: PipelineStage },
    PlanTaskAdded { task: PlanTask },

    // 运行时输出
    ProcessOutputDelta { id: String, output: ProcessOutput },

    // 生命周期
    TurnStarted,
    Done { summary: AgentSummary },
    Error { message: String, severity: ErrorSeverity },
}

pub enum ErrorSeverity {
    Transient,
    Recoverable,
    Fatal,
}

pub enum PipelineStage {
    IntentAnalysis,
    Planning,
    CodeGeneration,
    Verification,
    Integration,
}
```

### 消息类型

```rust
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

pub struct Message {
    pub role: MessageRole,
    pub content: MessageContent,
    pub metadata: HashMap<String, String>,
}

pub enum MessageContent {
    Text(String),
    MultiPart(Vec<ContentPart>),
}

pub struct ContentPart {
    pub part_type: ContentPartType,
    pub text: String,
}
```

### 权限级别

```rust
pub enum PermissionLevel {
    Ask,          // 每个操作都需用户确认
    AcceptEdits,  // 自动接受编辑，执行类需确认
    Plan,         // 只规划不执行
    Auto,         // 自动执行非破坏性操作
    Bypass,       // 全部自动（危险）
}
```

---

## 2.2 pl-model — LLM Provider 层

**职责**：LLM Provider 运行时抽象、模型发现与元数据管理、API 协议适配。

**依赖**：`pl-core`

**外部依赖**：`reqwest`, `serde`, `serde_json`, `tokio`, `tracing`, `futures`

### ModelProvider Trait

```rust
use std::fmt::Debug;

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

### 模型元数据

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

pub struct ModelInfo {
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,
    pub context_window: Option<u64>,
    pub max_context_window: Option<u64>,
    pub auto_compact_token_limit: Option<u64>,
    pub default_temperature: Option<f32>,
    pub max_output_tokens: Option<u64>,
    pub reasoning_efforts: Vec<String>,
    pub capabilities: ModelCapabilities,
    pub input_modalities: Vec<InputModality>,
    pub truncation_policy: TruncationPolicy,
    pub base_instructions: String,
    pub used_fallback: bool,
}

pub struct ProviderInfo {
    pub name: String,
    pub base_url: Option<String>,
    pub env_key: Option<String>,
    pub default_model: String,
    pub bearer_token: Option<String>,
    pub auth_command: Option<AuthCommand>,
    pub wire_api: WireApi,
    pub http_headers: Option<HashMap<String, String>>,
    pub request_max_retries: Option<u32>,
    pub stream_max_retries: Option<u32>,
    pub stream_idle_timeout_ms: Option<u64>,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct ProviderCapabilities: u32 {
        const STREAMING             = 0b00000001;
        const FUNCTION_CALLING      = 0b00000010;
        const VISION                = 0b00000100;
        const PARALLEL_TOOL_CALLS   = 0b00001000;
    }
}

pub struct CompletionRequest {
    pub model: String,
    pub instructions: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
    pub tool_choice: String,
    pub parallel_tool_calls: bool,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u64>,
    pub reasoning: Option<ReasoningConfig>,
    pub stream: bool,
}

pub struct ReasoningConfig {
    pub effort: Option<String>,
    pub summary: Option<ReasoningSummary>,
}

pub struct CompletionResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: TokenUsage,
    pub finish_reason: FinishReason,
    pub model: String,
}

pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}
```

### 内置默认模型

`pl-model/src/default_models.rs` 直接用 Rust 结构体提供内置模型，不再维护外部 JSON 数据文件。

```rust
pub(crate) fn default_model_slugs() -> &'static [&'static str];
pub(crate) fn default_models() -> Vec<ModelInfo>;
```

当前内置模型为 `deepseek-v4-flash`、`gpt-5.5`、`gpt-5.4`、`gpt-5.4-mini`、`gpt-5.4-nano`。默认 provider 为 DeepSeek，默认模型为 `deepseek-v4-flash`。

### WireAdapter Trait

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

pub enum WireDispatch {
    Responses,
    Chat,
}
```

### ModelsManager Trait

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

---

## 2.3 pl-tool — 工具系统

**职责**：定义工具 trait、工具注册表、工具发现、工具执行引擎。支持内置工具和动态注册。

**依赖**：`pl-core`

**外部依赖**：`serde_json`, `tokio`, `tracing`

### Tool Trait

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

pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,  // JSON Schema
    pub category: ToolCategory,
    pub danger_level: DangerLevel,
}

pub enum ToolCategory {
    FileSystem,
    Execution,
    Search,
    Network,
    Llm,
    Custom,
}

pub enum DangerLevel {
    Safe,       // 只读操作
    Moderate,   // 写入操作
    Dangerous,  // 破坏性操作（删除、执行命令）
}

pub struct ToolInput {
    pub name: String,
    pub arguments: serde_json::Value,
}

pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
    pub metadata: HashMap<String, String>,
}

pub struct ToolExecutionContext {
    pub workdir: PathBuf,
    pub permission_level: PermissionLevel,
    pub session_id: SessionId,
}
```

### 工具注册表

```rust
pub struct ToolRegistry { /* HashMap<String, Box<dyn Tool>> */ }

impl ToolRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, tool: Box<dyn Tool>) -> Result<()>;
    pub fn unregister(&mut self, name: &str) -> Result<()>;
    pub fn get(&self, name: &str) -> Option<&dyn Tool>;
    pub fn list(&self) -> Vec<&ToolDefinition>;
    pub fn list_by_category(&self, category: ToolCategory) -> Vec<&ToolDefinition>;
}
```

### 内置工具（首版实现）

| 工具 | 类别 | 危险级别 | 说明 |
|------|------|---------|------|
| `ReadFile` | FileSystem | Safe | 读取文件内容 |
| `WriteFile` | FileSystem | Moderate | 创建或修改文件 |
| `Execute` | Execution | Dangerous | 在沙箱中执行命令 |
| `Search` | Search | Safe | 搜索代码/文本 |
| `AskUser` | Custom | Safe | 向用户询问信息 |

---

## 2.4 pl-skill — 技能/插件系统

**职责**：技能的加载、解析、注入和生命周期管理。技能是可复用的能力模块。

**依赖**：`pl-core`, `pl-tool`

**外部依赖**：`serde`, `toml`, `tera`（模板引擎）, `tracing`

### Skill 结构

```rust
pub struct SkillMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub scope: SkillScope,
    pub tools: Vec<String>,       // 需要的工具名
}

pub enum SkillScope {
    System,    // 系统内置
    User,      // 用户全局
    Project,   // 项目级别
}

pub struct Skill {
    pub metadata: SkillMetadata,
    pub prompt_template: String,
    pub tool_bindings: Vec<String>,
}
```

### SkillManager

```rust
pub struct SkillManager { /* skills: HashMap<String, Skill> */ }

impl SkillManager {
    pub async fn load_from_dir(&mut self, path: &Path) -> Result<()>;
    pub fn get(&self, name: &str) -> Option<&Skill>;
    pub fn list_available(&self) -> Vec<&SkillMetadata>;
    pub fn render_injection(&self, name: &str, context: &SkillContext) -> Option<String>;
    pub fn detect_implicit(&self, input: &str) -> Option<&Skill>;
}
```

### 技能文件格式

```toml
# skills/rust-http-server/skill.toml
[metadata]
name = "rust-http-server"
version = "1.0.0"
description = "创建 Rust HTTP 服务器"
scope = "user"

[tools]
required = ["read_file", "write_file", "execute"]
```

```markdown
# skills/rust-http-server/prompt.md
你是一个 Rust Web 开发专家。
当用户需要创建 HTTP 服务器时，遵循以下步骤：
1. 使用 Cargo 初始化项目
2. 添加 axum 依赖
...
```

---

## 2.5 pl-memory — 记忆与上下文管理

**职责**：管理对话上下文、会话状态、项目知识和用户偏好。

**依赖**：`pl-core`

**外部依赖**：`serde`, `serde_json`, `tokio`, `chrono`, `uuid`, `tracing`

### MemoryStore Trait

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

pub struct MemoryEntry {
    pub id: MemoryId,
    pub content: String,
    pub memory_type: MemoryType,
    pub scope: MemoryScope,
    pub timestamp: DateTime<Utc>,
    pub relevance: f32,
    pub metadata: HashMap<String, String>,
}

pub enum MemoryType {
    Conversation,
    SessionState,
    ProjectKnowledge,
    UserPreference,
    Checkpoint,
}

pub enum MemoryScope {
    ShortTerm,
    MidTerm,
    LongTerm,
}

pub struct MemoryQuery {
    pub keywords: Vec<String>,
    pub scope: Option<MemoryScope>,
    pub memory_type: Option<MemoryType>,
    pub limit: usize,
}
```

### ContextManager

```rust
pub struct ContextManager { /* store: Box<dyn MemoryStore> */ }

impl ContextManager {
    pub async fn build_context_window(&self, max_tokens: usize) -> ContextWindow;
    pub async fn add_message(&self, message: Message) -> Result<()>;
    pub async fn compact(&self, strategy: CompactionStrategy) -> Result<()>;
    pub async fn get_project_context(&self) -> Result<ProjectContext>;
    pub async fn create_checkpoint(&self) -> Result<CheckpointId>;
    pub async fn restore_checkpoint(&self, id: &CheckpointId) -> Result<()>;
}

pub enum CompactionStrategy {
    SlidingWindow { keep_recent: usize },
    Summarize { max_summary_tokens: usize },
    ImportanceBased { threshold: f32 },
}
```

### 内置实现

| 实现 | 说明 | 用途 |
|------|------|------|
| `InMemoryStore` | 纯内存存储 | 测试、短期上下文 |
| `FileMemoryStore` | JSONL 文件存储 | 持久化（首版） |

---

## 2.6 pl-runtime — 沙箱执行引擎

**职责**：提供安全的代码执行环境，所有 LLM 生成的代码在受限沙箱中执行。通过 channel 流式推送进程输出。

**依赖**：`pl-core`

**外部依赖**：`tokio`（process）, `tracing`

### Runtime Trait

```rust
/// 沙箱执行环境抽象。
///
/// 所有 LLM 生成的命令在受限沙箱中执行。
/// 通过 ProcessOutput channel 流式推送 stdout/stderr。
///
/// 实现者契约：
/// - execute_stream() 必须尊重 SandboxConstraints 中的所有限制
/// - 超时时终止子进程并返回 ExitStatus::TimedOut
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

pub enum SandboxType {
    OsNative,    // OS 原生沙箱
    Container,   // Docker/Podman
    Process,     // 受限子进程
    None,        // 无沙箱（仅测试）
}

pub struct ExecutionRequest {
    pub command: String,
    pub args: Vec<String>,
    pub workdir: PathBuf,
    pub env: HashMap<String, String>,
    pub stdin: Option<String>,
    pub timeout: Duration,
    pub constraints: SandboxConstraints,
}

pub struct SandboxConstraints {
    pub allow_network: bool,
    pub allow_file_write: bool,
    pub writable_paths: Vec<PathBuf>,
    pub readable_paths: Vec<PathBuf>,
    pub max_memory: Option<usize>,
    pub max_cpu_time: Option<Duration>,
    pub max_output_size: Option<usize>,
}

pub struct ProcessResult {
    pub exit_status: ExitStatus,
    pub duration: Duration,
}

pub enum ExitStatus {
    Exited(i32),
    TimedOut,
    Signaled(i32),
}

pub enum ProcessOutput {
    Stdout(String),
    Stderr(String),
}
```

### 平台实现策略

| 平台 | 首版方案 | 后续方案 |
|------|---------|---------|
| Windows | 受限子进程（Job Object） | Windows Sandbox |
| Linux | 受限子进程 | Landlock + Bubblewrap |
| macOS | 受限子进程 | Seatbelt |

---

## 2.7 pl-compiler — 自然语言编译管线

**职责**：将自然语言输入通过多阶段编译管线转换为可执行代码。通过 AgentEvent stream 推送各阶段进度。

**依赖**：`pl-core`, `pl-model`

**外部依赖**：`tokio`, `serde_json`, `tracing`

### Compiler Trait

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

pub struct CompileInput {
    pub natural_language: String,
    pub context: CompileContext,
    pub options: CompileOptions,
}

pub struct CompileSummary {
    pub intent: Intent,
    pub plan: Plan,
    pub artifacts: Vec<Artifact>,
    pub verification_results: Vec<VerificationResult>,
}
```

### 管线阶段

```rust
pub struct Intent {
    pub primary: String,
    pub sub_intents: Vec<String>,
    pub constraints: Vec<String>,
    pub entities: Vec<Entity>,
    pub confidence: f32,
}

pub struct Plan {
    pub tasks: Vec<Task>,
    pub dependencies: Vec<TaskDependency>,
}

pub struct Task {
    pub id: TaskId,
    pub description: String,
    pub task_type: TaskType,
    pub input: String,
}

pub enum TaskType {
    CreateFile,
    ModifyFile,
    DeleteFile,
    ExecuteCommand,
    LlmGeneration,
    Search,
    Verify,
}

pub enum DependencyType {
    Sequential,
    Parallel,
    Conditional,
}
```

### 五阶段管线

```
NL Input → IntentAnalyzer → Planner → CodeGenerator → Verifier → Integrator
              (LLM)          (LLM)     (LLM)          (Runtime)  (Runtime)
```

---

## 2.8 pl-agent — Agent Loop

**职责**：实现 ReAct 模式的 Agent 循环，驱动整个编译和执行过程。通过 AgentEvent stream 推送所有中间状态。

**依赖**：`pl-core`, `pl-model`, `pl-compiler`, `pl-tool`, `pl-memory`, `pl-skill`, `pl-runtime`

**外部依赖**：`tokio`, `tracing`, `futures`

### Agent Trait

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

pub enum AgentStatus {
    Idle,
    Thinking,
    ExecutingTool { tool_name: String },
    WaitingForApproval,
    Error(String),
}

pub struct AgentSummary {
    pub tasks_completed: usize,
    pub files_modified: Vec<PathBuf>,
    pub tools_used: Vec<String>,
    pub total_tokens: TokenUsage,
}
```

### PureAgent 主实现

```rust
pub struct PureAgent {
    compiler: Box<dyn Compiler>,
    tool_registry: ToolRegistry,
    context_manager: ContextManager,
    skill_manager: SkillManager,
    sandbox: Box<dyn Runtime>,
    model_provider: Box<dyn ModelProvider>,
}
```

---

## 2.9 pl-cli — 交互界面

**职责**：命令行参数解析、用户交互、事件流渲染。

**依赖**：`pl-agent`, `pl-core`

**外部依赖**：`clap`, `crossterm`, `tokio`

```rust
pub struct Cli { /* agent: PureAgent, config: Config */ }

impl Cli {
    pub fn parse_args() -> CliArgs;
    pub async fn run_interactive(&self) -> Result<()>;
    pub async fn run_single(&self, prompt: String) -> Result<()>;
}
```

### CLI 命令设计

```
pure-lang                    # 交互模式
pure-lang "创建 HTTP 服务器"  # 单次执行模式
pure-lang --plan "..."       # 只规划不执行
pure-lang --auto "..."       # 自动执行模式
pure-lang skill list         # 列出可用技能
pure-lang skill add <path>   # 添加技能
pure-lang config             # 显示配置
```

---

## 2.10 pure-lang — 主程序入口

**职责**：二进制入口点，读取配置，组装所有组件，启动 CLI。

**依赖**：`pl-cli`, `pl-core`

```rust
// pure-lang/src/main.rs
#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    let cli = Cli::build(config).await?;
    cli.run().await
}
```

---

## 2.11 Workspace Cargo.toml

```toml
[workspace]
members = [
    "code/pl-core",
    "code/pl-model",
    "code/pl-tool",
    "code/pl-skill",
    "code/pl-memory",
    "code/pl-runtime",
    "code/pl-compiler",
    "code/pl-agent",
    "code/pl-cli",
    "code/pure-lang",
]
resolver = "3"

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "Apache-2.0"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
futures = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
thiserror = "2"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
regex = "1"
clap = { version = "4", features = ["derive"] }
reqwest = { version = "0.12", features = ["stream"] }
bitflags = "2"

pl-core = { path = "code/pl-core" }
pl-model = { path = "code/pl-model" }
pl-tool = { path = "code/pl-tool" }
pl-skill = { path = "code/pl-skill" }
pl-memory = { path = "code/pl-memory" }
pl-runtime = { path = "code/pl-runtime" }
pl-compiler = { path = "code/pl-compiler" }
pl-agent = { path = "code/pl-agent" }
pl-cli = { path = "code/pl-cli" }
```
