# 02 - 各 Crate 详细设计

## 2.1 pl-core — 核心抽象与共享类型

**职责**：定义所有其他 crate 共享的核心 trait、错误类型、配置结构和公共类型。不包含具体实现逻辑。

**依赖**：无内部依赖，是整个依赖图的最底层。

**外部依赖**：`serde`, `serde_json`, `async-trait`, `thiserror`, `tracing`

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

### LLM Provider Trait

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;
    async fn stream(&self, request: CompletionRequest) -> Result<CompletionStream>;
    fn model_name(&self) -> &str;
    fn context_window(&self) -> usize;
}

pub struct CompletionRequest {
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
    pub max_tokens: Option<usize>,
    pub temperature: Option<f32>,
}

pub struct CompletionResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: TokenUsage,
}

pub struct TokenUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
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

## 2.2 pl-tool — 工具系统

**职责**：定义工具 trait、工具注册表、工具发现、工具执行引擎。支持内置工具和动态注册。

**依赖**：`pl-core`

**外部依赖**：`serde_json`, `async-trait`, `tokio`, `tracing`

### Tool Trait

```rust
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

pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
    pub metadata: HashMap<String, String>,
}

pub struct ToolExecutionContext {
    pub workdir: PathBuf,
    pub permission_level: PermissionLevel,
    pub session_id: SessionId,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> &ToolDefinition;
    async fn execute(
        &self,
        input: ToolInput,
        context: &ToolExecutionContext,
    ) -> Result<ToolOutput>;
    fn requires_confirmation(&self, input: &ToolInput) -> bool {
        self.definition().danger_level != DangerLevel::Safe
    }
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

## 2.3 pl-skill — 技能/插件系统

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

## 2.4 pl-memory — 记忆与上下文管理

**职责**：管理对话上下文、会话状态、项目知识和用户偏好。

**依赖**：`pl-core`

**外部依赖**：`serde`, `serde_json`, `tokio`, `chrono`, `uuid`, `tracing`

### MemoryStore Trait

```rust
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

#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn store(&self, entry: MemoryEntry) -> Result<MemoryId>;
    async fn retrieve(&self, query: &MemoryQuery) -> Result<Vec<MemoryEntry>>;
    async fn delete(&self, id: &MemoryId) -> Result<()>;
    async fn search(&self, keywords: &[&str], limit: usize) -> Result<Vec<MemoryEntry>>;
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

## 2.5 pl-runtime — 沙箱执行引擎

**职责**：提供安全的代码执行环境，所有 LLM 生成的代码在受限沙箱中执行。

**依赖**：`pl-core`

**外部依赖**：`tokio`（process）, `tracing`

### Runtime Trait

```rust
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

pub struct ExecutionResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
    pub timed_out: bool,
}

#[async_trait]
pub trait Runtime: Send + Sync {
    async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionResult>;
    fn is_available(&self) -> bool;
    fn sandbox_type(&self) -> SandboxType;
}
```

### 平台实现策略

| 平台 | 首版方案 | 后续方案 |
|------|---------|---------|
| Windows | 受限子进程（Job Object） | Windows Sandbox |
| Linux | 受限子进程 | Landlock + Bubblewrap |
| macOS | 受限子进程 | Seatbelt |

---

## 2.6 pl-compiler — 自然语言编译管线

**职责**：将自然语言输入通过多阶段编译管线转换为可执行代码。

**依赖**：`pl-core`, `pl-runtime`

**外部依赖**：`async-trait`, `tokio`, `serde_json`, `tracing`

### Compiler Trait

```rust
#[async_trait]
pub trait Compiler: Send + Sync {
    async fn compile(&self, input: CompileInput) -> Result<CompileOutput>;
    async fn analyze_intent(&self, input: &str) -> Result<Intent>;
    async fn plan(&self, intent: &Intent) -> Result<Plan>;
    async fn generate(&self, task: &Task) -> Result<GeneratedCode>;
}

pub struct CompileInput {
    pub natural_language: String,
    pub context: CompileContext,
    pub options: CompileOptions,
}

pub struct CompileOutput {
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

## 2.7 pl-agent — Agent Loop

**职责**：实现 ReAct 模式的 Agent 循环，驱动整个编译和执行过程。

**依赖**：`pl-core`, `pl-compiler`, `pl-tool`, `pl-memory`, `pl-skill`, `pl-runtime`

**外部依赖**：`tokio`, `async-trait`, `tracing`, `futures`

### Agent Trait

```rust
pub enum AgentStatus {
    Idle,
    Thinking,
    ExecutingTool { tool_name: String },
    WaitingForApproval,
    Error(String),
}

#[async_trait]
pub trait Agent: Send + Sync {
    async fn handle_input(&self, input: AgentInput) -> Result<Vec<AgentEvent>>;
    async fn interrupt(&self) -> Result<()>;
    fn status(&self) -> AgentStatus;
}
```

### Agent Event（流式输出）

```rust
pub enum AgentEvent {
    Thinking { content: String },
    ToolCall { tool_name: String, input: serde_json::Value },
    ToolResult { tool_name: String, result: ToolOutput },
    ApprovalRequest { action: String, details: String },
    TextOutput { content: String },
    FileChange { path: PathBuf, operation: FileOperation },
    Done { summary: String },
    Error { message: String },
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
    llm_provider: Box<dyn LlmProvider>,
}
```

---

## 2.8 pl-cli — 交互界面

**职责**：命令行参数解析、用户交互、事件展示。

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

## 2.9 pure-lang — 主程序入口

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

## 2.10 Workspace Cargo.toml

```toml
[workspace]
members = [
    "code/pl-core",
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
async-trait = "0.1"
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

pl-core = { path = "code/pl-core" }
pl-tool = { path = "code/pl-tool" }
pl-skill = { path = "code/pl-skill" }
pl-memory = { path = "code/pl-memory" }
pl-runtime = { path = "code/pl-runtime" }
pl-compiler = { path = "code/pl-compiler" }
pl-agent = { path = "code/pl-agent" }
pl-cli = { path = "code/pl-cli" }
```
