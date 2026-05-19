# 03 - 编译管线与 Agent 循环

## 3.1 编译管线总览

Pure-Lang 将自然语言转换为应用程序的过程分为五个阶段，每个阶段通过 `AgentEvent` stream 推送实时进度：

```
┌─────────────┐    ┌───────────┐    ┌───────────┐    ┌──────────┐    ┌───────────┐
│  NL Input   │───>│  Intent   │───>│   Plan    │───>│  Code    │───>│   App     │
│  自然语言    │    │  意图分析  │    │  任务规划  │    │  代码生成 │    │  集成验证  │
└─────────────┘    └───────────┘    └───────────┘    └──────────┘    └───────────┘
                     Stage 1          Stage 2          Stage 3        Stage 4+5
                     (LLM)            (LLM)            (LLM)          (Runtime)
```

每个阶段开始时推送 `PipelineStageStarted`，结束时推送 `PipelineStageDone`。

### Stage 1: 意图分析（IntentAnalyzer）

**输入**：用户自然语言描述 + 上下文（项目结构、对话历史、技能注入）

**输出**：结构化的 `Intent`

**事件流**：`PipelineStageStarted::IntentAnalysis` → LLM 流式输出（`TextDelta`）→ `PipelineStageDone::IntentAnalysis`

```rust
pub struct Intent {
    pub primary: String,              // 主要意图
    pub sub_intents: Vec<String>,     // 子意图列表
    pub constraints: Vec<String>,     // 约束条件
    pub entities: Vec<Entity>,        // 识别的实体
    pub confidence: f32,              // 置信度
}
```

**LLM Prompt 策略**：

```
System: 你是一个意图分析器。分析用户的自然语言描述，提取：
1. 主要意图（用户想做什么）
2. 子意图（分解的步骤）
3. 约束条件（技术栈、性能要求等）
4. 关键实体（项目名、文件名、技术名词等）

输出 JSON 格式。

Context: {项目结构} {对话历史} {技能上下文}

User: {用户输入}
```

**示例**：

```
输入: "帮我创建一个 Rust HTTP 服务器，包含 /health 端点，使用 axum 框架"

输出:
{
  "primary": "创建 Rust HTTP 服务器项目",
  "sub_intents": [
    "初始化 Cargo 项目",
    "添加 axum 依赖",
    "实现 /health 端点",
    "编写启动代码"
  ],
  "constraints": [
    "语言: Rust",
    "框架: axum",
    "端点: /health"
  ],
  "entities": [
    { "name": "Rust", "type": "language" },
    { "name": "axum", "type": "framework" },
    { "name": "/health", "type": "endpoint" }
  ],
  "confidence": 0.95
}
```

### Stage 2: 任务规划（Planner）

**输入**：`Intent` + 项目上下文

**输出**：带依赖关系的 `Plan`

**事件流**：`PipelineStageStarted::Planning` → 逐个推送 `PlanTaskAdded` → `PipelineStageDone::Planning`

```rust
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

pub struct TaskDependency {
    pub from: TaskId,
    pub to: TaskId,
    pub dep_type: DependencyType,  // Sequential / Parallel / Conditional
}
```

**规划示例**：

```
Task 1: cargo init --name health-server          [ExecuteCommand]
Task 2: 修改 Cargo.toml 添加 axum 依赖           [ModifyFile]
    └── depends on: Task 1 (Sequential)
Task 3: 创建 src/main.rs 实现 /health 端点       [CreateFile]
    └── depends on: Task 2 (Sequential)
Task 4: cargo check 验证编译                     [ExecuteCommand]
    └── depends on: Task 3 (Sequential)
Task 5: cargo test 运行测试                      [ExecuteCommand]
    └── depends on: Task 4 (Sequential)
```

### Stage 3: 代码生成（CodeGenerator）

**输入**：单个 `Task` + 项目上下文

**输出**：`GeneratedCode`（文件内容或命令）

**事件流**：`PipelineStageStarted::CodeGeneration` → LLM 流式输出代码 → `PipelineStageDone::CodeGeneration`

```rust
pub struct GeneratedCode {
    pub files: Vec<FileArtifact>,
    pub commands: Vec<CommandArtifact>,
    pub notes: Vec<String>,
}

pub struct FileArtifact {
    pub path: PathBuf,
    pub content: String,
    pub operation: FileOperation,  // Create / Modify / Delete
}
```

每个 Task 独立生成，Agent 循环逐个执行。

### Stage 4: 验证（Verifier）

**输入**：生成的代码产物

**输出**：`VerificationResult`

**事件流**：`PipelineStageStarted::Verification` → 进程流式输出 → `PipelineStageDone::Verification`

验证方式取决于配置的 `VerificationLevel`：

| 级别 | 操作 |
|------|------|
| None | 跳过验证 |
| Syntax | 语法检查（如 `cargo check`） |
| TypeCheck | 类型检查 |
| Run | 编译运行，验证输出 |
| Full | 完整测试套件 |

### Stage 5: 集成（Integrator）

**输入**：所有验证通过的产物

**输出**：最终应用程序

**事件流**：`PipelineStageStarted::Integration` → 文件写入事件 → `PipelineStageDone::Integration`

将所有文件写入磁盘，执行安装依赖等收尾操作。

---

## 3.2 ReAct Agent 循环

Agent 循环是驱动整个编译管线的核心引擎。它采用 ReAct（Reasoning + Acting）模式，通过 `AgentEventSender` 推送所有中间状态。

### 循环结构

```
              ┌──────────────────────────┐
              │                          │
              ▼                          │
     ┌─────────────────┐                │
     │ 1. Observation   │                │
     │    观察当前状态    │                │
     └────────┬────────┘                │
              │                         │
              ▼                         │
     ┌─────────────────┐                │
     │ 2. Thought       │                │
     │    LLM 流式推理  │◄─── 记忆系统    │
     │    确定下一步     │     提供上下文   │
     └────────┬────────┘                │
              │                         │
              ▼                         │
     ┌─────────────────┐                │
     │ 3. Action        │                │
     │    流式执行工具/  │──── 工具注册表   │
     │    生成代码      │                  │
     └────────┬────────┘                │
              │                         │
              ▼                         │
     ┌─────────────────┐                │
     │ 4. Evaluate      │                │
     │    评估结果       │                │
     └────────┬────────┘                │
              │                         │
        ┌──────┴──────┐                  │
        │             │                  │
     未完成          完成                 │
        │             │                  │
        ▼             ▼                  │
     回到 1      推送 Done               │
              (continue)────────────────┘
```

### 单次循环的详细流程（流式）

```rust
async fn react_iteration(
    &self,
    state: &mut AgentState,
    event_tx: &AgentEventSender,
) -> Result<IterationOutcome> {
    // 1. Observation: 收集当前状态
    let context = self.context_manager.build_context_window(max_tokens).await?;
    let project_ctx = self.context_manager.get_project_context().await?;

    // 2. Thought: 流式调用 LLM 推理
    let messages = build_messages(&context, &project_ctx, &state.history);
    let response = self.model_provider.stream_complete(
        CompletionRequest {
            model: self.model_provider.default_model().to_string(),
            messages,
            tools: self.tool_registry.list_schemas(),
            ..
        },
        event_tx.clone(),
    ).await?;

    // 3. Action: 执行 LLM 返回的动作
    if !response.tool_calls.is_empty() {
        for call in response.tool_calls {
            // 3a. 推送 ToolCallStarted
            let _ = event_tx.send(AgentEvent::ToolCallStarted {
                id: call.id.clone(),
                name: call.name.clone(),
                input: call.arguments.clone(),
            });

            // 3b. 权限检查
            if self.needs_approval(&call) {
                let _ = event_tx.send(AgentEvent::ApprovalRequest { .. });
                // 等待用户确认
            }

            // 3c. 流式执行工具
            let tool = self.tool_registry.get(&call.name)?;
            let result = tool.execute_stream(
                ToolInput { name: call.name, arguments: call.arguments },
                &exec_ctx,
                event_tx.clone(),
            ).await?;

            // 3d. 推送 ToolCallCompleted
            let _ = event_tx.send(AgentEvent::ToolCallCompleted {
                id: call.id.clone(),
                result,
            });

            // 3e. 记录到记忆
            self.context_manager.add_message(/* tool result */).await?;
        }
    }

    // 4. Evaluate: 检查是否完成
    if self.is_task_complete(&state) {
        Ok(IterationOutcome::Done)
    } else {
        Ok(IterationOutcome::Continue)
    }
}
```

### Agent 状态管理

```rust
pub struct AgentState {
    pub session_id: SessionId,
    pub working_directory: PathBuf,
    pub permission_level: PermissionLevel,
    pub iteration_count: usize,
    pub max_iterations: usize,
    pub current_plan: Option<Plan>,
    pub completed_tasks: HashSet<TaskId>,
    pub history: Vec<AgentEvent>,
}
```

---

## 3.3 端到端数据流示例

用户输入：**"帮我创建一个 Rust HTTP 服务器，包含 /health 端点"**

```
[用户输入] ──> pl-cli 解析
    │
    ▼
[pl-agent] 创建 AgentState，推送 TurnStarted
    │
    ├── [pl-memory] 加载上下文
    │   ├── 读取 PURE.md（项目知识）
    │   ├── 加载对话历史摘要
    │   └── 加载用户偏好
    │
    ├── [pl-skill] 检测技能匹配
    │   └── 匹配 "rust-http" 技能 → 注入提示模板
    │
    ▼
═══ ReAct Iteration 1: 意图分析 ═══
    │
    ├── PipelineStageStarted::IntentAnalysis
    ├── Thought: LLM 流式推理 → TextDelta "我来帮你创建..."
    ├── Plan: 推送 PlanTaskAdded × 5
    └── PipelineStageDone::IntentAnalysis

═══ ReAct Iteration 2: 执行 Task 1 ═══
    │
    ├── ToolCallStarted { name: "Execute", input: "cargo init" }
    ├── [pl-runtime] 流式执行 → ProcessOutputDelta（stdout/stderr）
    ├── ToolCallCompleted { result: exit_code: 0 }
    └── Memory: 记录 "项目已初始化"

═══ ReAct Iteration 3: 执行 Task 2 ═══
    │
    ├── ToolCallStarted { name: "WriteFile", path: "Cargo.toml" }
    │   ├── 权限检查: Moderate → AcceptEdits 模式 → 自动通过
    ├── ToolCallCompleted { result: 写入成功 }
    └── Memory: 记录 "依赖已添加"

═══ ReAct Iteration 4: 执行 Task 3 ═══
    │
    ├── ToolCallStarted { name: "WriteFile", path: "src/main.rs" }
    ├── ToolCallCompleted { result: 写入成功 }
    └── Memory: 记录 "main.rs 已创建"

═══ ReAct Iteration 5: 执行 Task 4 ═══
    │
    ├── ToolCallStarted { name: "Execute", input: "cargo check" }
    ├── [pl-runtime] 流式执行 → ProcessOutputDelta
    ├── ToolCallCompleted { result: exit_code: 0 }
    └── PipelineStageDone::Verification

═══ 完成 ═══
    │
    ▼
AgentEvent::Done {
    summary: AgentSummary {
        tasks_completed: 5,
        files_modified: ["Cargo.toml", "src/main.rs"],
        tools_used: ["Execute", "WriteFile"],
        ...
    }
}
    │
    ▼
[pl-cli] 渲染结果给用户
```

---

## 3.4 子代理（Sub-Agent）设计

对于可并行的任务，主 Agent 可以派生子代理并行处理：

```
┌──────────────┐
│  Main Agent  │
└──────┬───────┘
       │
       ├── Spawn ──► Sub-Agent A (并行任务 1)
       │                ├── 流式执行工具
       │                └── 推送事件 ──┐
       │                              │
       ├── Spawn ──► Sub-Agent B (并行任务 2)   │
       │                ├── 流式执行工具           │
       │                └── 推送事件 ──┐          │
       │                              │          │
       └──────────────────────────────┴──────────┘
                   汇总结果，继续执行
```

子代理共享主 Agent 的工具注册表和记忆系统，但有独立的上下文窗口。子代理的事件通过主 Agent 的 `event_tx` 转发。

```rust
pub struct SubAgent {
    agent: PureAgent,
    parent_session: SessionId,
    task: Task,
}

impl SubAgent {
    pub async fn execute(&self, event_tx: &AgentEventSender) -> Result<TaskResult> {
        // 独立的 ReAct 循环，限定在单个 Task 范围内
        // 事件通过 event_tx 转发给主 Agent 的消费者
        self.agent.handle_input(/* task as input */, event_tx.clone()).await
    }
}
```

---

## 3.5 错误恢复机制

Agent 循环中每一步都可能失败，系统需要优雅地处理：

```
错误发生
    │
    ▼
推送 AgentEvent::Error { message, severity }
    │
    ▼
判断错误类型
    │
    ├── LLM API 错误
    │   └── 重试（指数退避，最多 3 次）
    │
    ├── 工具执行失败（编译错误、命令错误）
    │   └── Agent 分析错误 → 修改方案 → 重试
    │       └── 超过重试次数 → 请求用户介入
    │
    ├── 沙箱超时
    │   └── 终止进程 → Agent 决定是否调整并重试
    │
    ├── 权限拒绝
    │   └── 暂停 → 请求用户授权或调整方案
    │
    └── 上下文溢出
        └── 触发压缩 → 保留关键信息 → 继续
```

### 重试策略

```rust
pub struct RetryPolicy {
    pub max_retries: usize,
    pub backoff: BackoffStrategy,
    pub retryable_errors: Vec<PureErrorKind>,
}

pub enum BackoffStrategy {
    Fixed(Duration),
    Exponential { initial: Duration, max: Duration },
}
```
