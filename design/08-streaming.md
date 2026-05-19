# 08 - 原生全流式架构修正

> 本文档是对 01-07 设计文档的系统性修正，将整个系统从"请求-响应"模式改为**原生全流式**。

## 8.1 问题诊断

### 当前设计的流式缺陷

| 位置 | 问题 | 影响 |
|------|------|------|
| 02-crates: `LlmProvider::complete()` | 非流式是默认路径 | 流式变为附加特性 |
| 02-crates: `ToolOutput::content: String` | 工具输出必须完整缓冲 | 命令执行无法实时显示 |
| 02-crates: `ExecutionResult::stdout: String` | 沙箱输出必须完整缓冲 | 用户看到长时间空白 |
| 03-pipeline: `CompileOutput` 完整返回 | 五阶段管线全部完成才有输出 | 用户无法看到中间进度 |
| 03-pipeline: `Plan::tasks: Vec<Task>` | 必须生成完整计划 | 无法边规划边执行 |
| 07-model: `stream: bool` 可选标志 | 流式是 opt-in | 与"原生流式"矛盾 |
| 07-model: 流式列为 P1 | 流式不是基础特性 | 架构根基错误 |

### 核心原则：流式是默认，非流式是特例

```
错误: complete() 为主 + stream() 为辅
正确: stream() 为主 + complete() = stream().collect()
```

---

## 8.2 流式架构总览

```
┌─────────────────────────────────────────────────────────────┐
│                     CLI / TUI 渲染层                         │
│              AdaptiveChunking + StreamController             │
├─────────────────────────────────────────────────────────────┤
│                      AgentEvent Stream                       │
│       (系统唯一的输出通道，所有内容通过此流向下推送)            │
├──────┬──────────┬──────────────┬──────────┬─────────────────┤
│      │          │              │          │                 │
│ LLM  │  Tool    │  Compiler    │  Memory  │  Runtime        │
│ Delta│  Stream  │  Pipeline    │  Stream  │  Stream         │
│      │          │  Events      │          │                 │
│      │          │              │          │                 │
│ SSE/ │  子进程   │  阶段事件    │ 查询结果  │  stdout/stderr  │
│ WS   │  stdout  │  流式下发    │ 流式返回  │  实时流          │
└──────┴──────────┴──────────────┴──────────┴─────────────────┘
```

**核心变化**：系统只有一个统一的输出通道——`AgentEvent` stream。所有子系统将自己的事件流汇入此通道，CLI/TUI 从中消费并渲染。

---

## 8.3 统一事件流设计

### AgentEvent（统一事件类型）

替代之前所有"完整返回"的 struct，改为事件流：

```rust
/// 系统统一的输出事件流
#[derive(Debug, Clone)]
pub enum AgentEvent {
    // ── LLM 推理 ──

    /// LLM 文本输出增量
    TextDelta {
        content: String,
    },

    /// LLM 思考/推理增量（如 Claude thinking tokens）
    ThinkingDelta {
        content: String,
    },

    /// LLM 生成工具调用（增量参数）
    ToolCallDelta {
        id: String,
        name: String,
        arguments_delta: String,
    },

    /// LLM 完成一个工具调用（参数完整）
    ToolCallComplete {
        id: String,
        name: String,
        arguments: String,
    },

    // ── 工具执行 ──

    /// 工具开始执行
    ToolStarted {
        tool_name: String,
        call_id: String,
    },

    /// 工具输出增量（如命令执行的 stdout/stderr）
    ToolOutputDelta {
        call_id: String,
        stream: OutputStream,
        content: String,
    },

    /// 工具执行完成
    ToolDone {
        call_id: String,
        tool_name: String,
        exit_code: Option<i32>,
        duration: Duration,
    },

    // ── 编译管线 ──

    /// 管线阶段开始
    PipelineStageStarted {
        stage: PipelineStage,
    },

    /// 意图分析增量结果
    IntentDelta {
        field: String,
        value: String,
    },

    /// 任务计划增量（逐个下发任务）
    PlanTaskAdded {
        task: Task,
        index: usize,
    },

    /// 代码生成增量（逐文件下发）
    CodeGenerated {
        artifact: FileArtifact,
    },

    /// 管线阶段完成
    PipelineStageDone {
        stage: PipelineStage,
    },

    // ── 权限 ──

    /// 请求用户确认
    ApprovalNeeded {
        request: ApprovalRequest,
    },

    // ── 文件变更 ──

    /// 文件即将变更
    FileChangePending {
        path: PathBuf,
        operation: FileOperation,
    },

    /// 文件变更完成
    FileChangeDone {
        path: PathBuf,
        operation: FileOperation,
    },

    // ── 生命周期 ──

    /// Agent 开始新一轮推理
    TurnStarted {
        iteration: usize,
    },

    /// Agent 完成本轮推理
    TurnDone {
        iteration: usize,
    },

    /// 整个请求处理完成
    Done {
        summary: String,
    },

    /// 错误
    Error {
        message: String,
        severity: ErrorSeverity,
    },
}

/// 错误严重度
#[derive(Debug, Clone, Copy)]
pub enum ErrorSeverity {
    /// 瞬时错误，可自动重试
    Transient,
    /// 可恢复错误，Agent 可自行处理
    Recoverable,
    /// 致命错误，需要用户介入
    Fatal,
}

/// 输出流类型（区分 stdout 和 stderr）
#[derive(Debug, Clone, Copy)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// 管线阶段
#[derive(Debug, Clone, Copy)]
pub enum PipelineStage {
    IntentAnalysis,
    Planning,
    CodeGeneration,
    Verification,
    Integration,
}
```

### 事件流类型

```rust
/// Agent 事件流 —— 系统的核心输出通道
///
/// 使用 tokio::sync::broadcast 而非 mpsc，原因：
/// 1. 支持多消费者（CLI + 日志 + 审计）
/// 2. 天然支持背压（channel 满时生产者等待）
/// 3. 广播语义（所有订阅者都收到同一事件）
pub type AgentEventSender = tokio::sync::broadcast::Sender<AgentEvent>;
pub type AgentEventReceiver = tokio::sync::broadcast::Receiver<AgentEvent>;
```

---

## 8.4 各层流式修正

### 8.4.1 Model Provider 层

**修正前**：
```rust
async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse>;
async fn stream(&self, req: CompletionRequest) -> Result<Box<dyn CompletionStream>>;
```

**修正后**：删除 `complete()`，只保留流式接口。非流式场景通过 `stream().collect()` 实现。

```rust
pub trait ModelProvider: Debug + Send + Sync {
    fn info(&self) -> &ProviderInfo;
    fn capabilities(&self) -> ProviderCapabilities;

    /// 流式补全请求（唯一接口）
    ///
    /// 通过 event_tx 推送 LLM 输出增量。
    /// 调用方负责消费 event_rx。
    fn stream_complete(
        &self,
        request: CompletionRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompletionResponse>> + Send;
}
```

**为什么用 channel 而不是 `impl Stream`**：

| 特性 | `impl Stream<Item = StreamChunk>` | `mpsc::Sender<AgentEvent>` |
|------|-----------------------------------|---------------------------|
| 多消费者 | 不支持（ownership 转移） | 天然支持（clone sender） |
| 背压 | 需要手动实现 | channel 容量自动控制 |
| 与 AgentEvent 统一 | 需要额外转换 | 直接推送，零转换 |
| 取消支持 | 需要 CancellationToken | drop sender 即取消 |

### SSE 流解析

参考 Codex 的 SSE 实现：

```rust
pub struct SseStreamParser {
    event_tx: AgentEventSender,
    buffer: String,
    tool_call_buffers: HashMap<String, ToolCallAccumulator>,
}

impl SseStreamParser {
    /// 处理一个 SSE 行
    pub fn handle_line(&mut self, line: &str) -> Result<()> {
        if let Some(data) = line.strip_prefix("data: ") {
            let event: SseEvent = serde_json::from_str(data)?;
            match event.kind.as_str() {
                "response.output_text.delta" => {
                    let _ = self.event_tx.try_send(AgentEvent::TextDelta {
                        content: event.delta.unwrap_or_default(),
                    });
                }
                "response.reasoning_summary_text.delta" => {
                    let _ = self.event_tx.try_send(AgentEvent::ThinkingDelta {
                        content: event.delta.unwrap_or_default(),
                    });
                }
                "response.function_call_arguments.delta" => {
                    self.accumulate_tool_call_delta(&event);
                }
                "response.output_item.done" => {
                    self.finalize_item(event.item)?;
                }
                "response.completed" => {
                    // 返回最终的 CompletionResponse
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// 工具调用增量累积器
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
}
```

### 8.4.2 Tool 层

**修正前**：
```rust
async fn execute(&self, input: ToolInput, ctx: &ToolExecutionContext)
    -> Result<ToolOutput>;
```

**修正后**：工具执行通过 event_tx 流式推送输出。

```rust
pub trait Tool: Send + Sync {
    fn definition(&self) -> &ToolDefinition;

    /// 流式执行工具
    ///
    /// 工具通过 event_tx 推送进度和输出。
    /// 返回最终的 ToolResult（摘要）。
    fn execute_stream(
        &self,
        input: ToolInput,
        ctx: &ToolExecutionContext,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<ToolResult>> + Send;
}

/// 工具最终结果（轻量摘要）
pub struct ToolResult {
    pub call_id: String,
    pub exit_code: Option<i32>,
    pub output_summary: String,  // 截断后的摘要
    pub is_error: bool,
    pub duration: Duration,
}
```

### 内置工具流式示例：ExecuteTool

```rust
pub struct ExecuteTool;

impl Tool for ExecuteTool {
    fn execute_stream(
        &self,
        input: ToolInput,
        ctx: &ToolExecutionContext,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<ToolResult>> + Send {
        async move {
        let call_id = input.call_id.clone();
        let mut child = spawn_sandbox_command(&input, ctx)?;

        // 通知：工具开始
        let _ = event_tx.send(AgentEvent::ToolStarted {
            tool_name: "execute".into(),
            call_id: call_id.clone(),
        }).await;

        // 实时流式读取 stdout
        let mut stdout_reader = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut stderr_reader = BufReader::new(child.stderr.take().unwrap()).lines();

        loop {
            tokio::select! {
                line = stdout_reader.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            let _ = event_tx.send(AgentEvent::ToolOutputDelta {
                                call_id: call_id.clone(),
                                stream: OutputStream::Stdout,
                                content: format!("{}\n", line),
                            }).await;
                        }
                        Ok(None) => {}
                        Err(_) => break,
                    }
                }
                line = stderr_reader.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            let _ = event_tx.send(AgentEvent::ToolOutputDelta {
                                call_id: call_id.clone(),
                                stream: OutputStream::Stderr,
                                content: format!("{}\n", line),
                            }).await;
                        }
                        Ok(None) => {}
                        Err(_) => break,
                    }
                }
                status = child.wait() => {
                    let status = status?;
                    let _ = event_tx.send(AgentEvent::ToolDone {
                        call_id: call_id.clone(),
                        tool_name: "execute".into(),
                        exit_code: status.code(),
                        duration: start.elapsed(),
                    }).await;

                    return Ok(ToolResult {
                        call_id,
                        exit_code: status.code(),
                        output_summary: truncated_output,
                        is_error: !status.success(),
                        duration: start.elapsed(),
                    });
                }
            }
        }
        }
    }
}
```

### 8.4.3 Runtime 层

**修正前**：
```rust
pub struct ExecutionResult {
    pub stdout: String,
    pub stderr: String,
    ...
}
```

**修正后**：Runtime 不再返回完整输出，而是通过 channel 流式推送。

```rust
pub trait Runtime: Send + Sync {
    /// 流式执行命令
    ///
    /// stdout/stderr 通过 output_tx 实时推送。
    /// 返回退出码和执行时长。
    fn execute_stream(
        &self,
        request: ExecutionRequest,
        output_tx: tokio::sync::mpsc::Sender<ProcessOutput>,
    ) -> impl std::future::Future<Output = Result<ProcessResult>> + Send;
}

pub enum ProcessOutput {
    Stdout(String),
    Stderr(String),
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
```

### 8.4.4 Compiler 层

**修正前**：
```rust
async fn compile(&self, input: CompileInput) -> Result<CompileOutput>;
```

**修正后**：编译管线通过事件流逐步推送各阶段结果。

```rust
pub trait Compiler: Send + Sync {
    /// 流式编译
    ///
    /// 通过 event_tx 逐步推送：
    /// 1. PipelineStageStarted(IntentAnalysis)
    /// 2. IntentDelta(...)
    /// 3. PipelineStageDone(IntentAnalysis)
    /// 4. PipelineStageStarted(Planning)
    /// 5. PlanTaskAdded(...)
    /// 6. PipelineStageDone(Planning)
    /// 7. PipelineStageStarted(CodeGeneration)
    /// 8. CodeGenerated(...)
    /// 9. PipelineStageDone(CodeGeneration)
    fn compile_stream(
        &self,
        input: CompileInput,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompileSummary>> + Send;
}

/// 编译摘要（管线结束后的轻量总结）
pub struct CompileSummary {
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
    pub artifacts_count: usize,
    pub total_duration: Duration,
}
```

### 8.4.5 Agent 层

**修正前**：
```rust
async fn handle_input(&self, input: AgentInput) -> Result<Vec<AgentEvent>>;
```

**修正后**：Agent 不再返回事件列表，而是接管 event_tx 推送。

```rust
pub trait Agent: Send + Sync {
    /// 处理用户输入，通过 event_tx 流式推送所有事件
    fn handle_input(
        &self,
        input: AgentInput,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<AgentSummary>> + Send;

    /// 中断当前操作
    fn interrupt(&self) -> impl std::future::Future<Output = Result<()>> + Send;
}

/// Agent 处理摘要
pub struct AgentSummary {
    pub iterations: usize,
    pub tool_calls: usize,
    pub files_changed: usize,
    pub tokens_used: TokenUsage,
    pub duration: Duration,
}
```

### ReAct 循环流式版

```rust
impl PureAgent {
    async fn react_loop(
        &self,
        input: AgentInput,
        event_tx: AgentEventSender,
    ) -> Result<AgentSummary> {
        let mut iteration = 0;

        loop {
            iteration += 1;
            let _ = event_tx.send(AgentEvent::TurnStarted { iteration }).await;

            // 1. 构建上下文
            let context = self.context_manager.build_context().await?;

            // 2. 调用 LLM（流式）
            let response = self.provider.stream_complete(
                CompletionRequest {
                    model: self.config.model.clone(),
                    messages: context.to_messages(),
                    tools: self.tool_registry.schemas(),
                    ..Default::default()
                },
                event_tx.clone(),  // LLM 输出直接推给消费者
            ).await?;

            // 3. 处理工具调用
            for tool_call in response.tool_calls {
                let tool = self.tool_registry.get(&tool_call.name)?;

                // 流式执行工具（工具输出直接推给消费者）
                let result = tool.execute_stream(
                    tool_call.into(),
                    &self.tool_exec_ctx(),
                    event_tx.clone(),
                ).await?;

                // 更新上下文
                self.context_manager.add_tool_result(result).await?;
            }

            let _ = event_tx.send(AgentEvent::TurnDone { iteration }).await;

            // 4. 判断是否继续
            if self.is_complete(&response) || iteration >= self.config.max_iterations {
                break;
            }
        }

        let _ = event_tx.send(AgentEvent::Done {
            summary: self.generate_summary(),
        }).await;

        Ok(self.build_summary())
    }
}
```

### 8.4.6 CLI 渲染层

参考 Codex 的自适应分块渲染：

```rust
pub struct StreamRenderer {
    event_rx: AgentEventReceiver,
    /// 文本累积器（用于 Markdown 增量渲染）
    collector: MarkdownStreamCollector,
    /// 排队行（带时间戳）
    queued_lines: VecDeque<QueuedLine>,
    /// 自适应分块策略
    chunking: AdaptiveChunkingPolicy,
}

/// 自适应分块：平滑模式 vs 追赶模式
pub enum ChunkingMode {
    /// 平滑：每次 tick 提交一行
    Smooth,
    /// 追赶：一次提交所有排队行
    CatchUp,
}

impl StreamRenderer {
    /// 主渲染循环
    pub async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                // 接收事件
                event = self.event_rx.recv() => {
                    match event {
                        Some(AgentEvent::TextDelta { content }) => {
                            self.collector.push_text(&content);
                            self.enqueue_collector_lines();
                        }
                        Some(AgentEvent::ToolOutputDelta { content, stream, .. }) => {
                            self.render_tool_output(&content, stream);
                        }
                        Some(AgentEvent::Done { .. }) => {
                            self.flush_all();
                            break;
                        }
                        Some(other) => self.handle_event(other),
                        None => break,
                    }
                }
                // 定时提交 tick（渲染排队内容）
                _ = tokio::time::sleep(Duration::from_millis(16)) => {
                    self.commit_tick();
                }
            }
        }
        Ok(())
    }

    /// 提交 tick：根据背压决定提交多少行
    fn commit_tick(&mut self) {
        match self.chunking.decide(self.queued_lines.len(), self.oldest_age()) {
            ChunkingMode::Smooth => {
                if let Some(line) = self.queued_lines.pop_front() {
                    self.render_line(line);
                }
            }
            ChunkingMode::CatchUp => {
                while let Some(line) = self.queued_lines.pop_front() {
                    self.render_line(line);
                }
            }
        }
    }
}
```

---

## 8.5 数据流对比

### 修正前（请求-响应）

```
用户输入
  │
  ├── LLM 调用（等待完整响应... 5秒空白）
  │   └── 返回完整文本 + 工具调用
  │
  ├── 执行命令（等待完整输出... 3秒空白）
  │   └── 返回完整 stdout/stderr
  │
  └── 渲染结果（一次性输出所有内容）
```

### 修正后（原生全流式）

```
用户输入
  │
  ├── LLM 调用开始
  │   ├── TextDelta("我")        ← 50ms 后用户看到第一个字
  │   ├── TextDelta("来帮你")
  │   ├── TextDelta("创建...")
  │   ├── ToolCallDelta(...)     ← 实时显示工具调用参数
  │   ├── ToolCallComplete(...)
  │   └── 完成
  │
  ├── 工具执行开始
  │   ├── ToolStarted
  │   ├── ToolOutputDelta("Creating project...")  ← 实时显示命令输出
  │   ├── ToolOutputDelta("Adding dependencies...")
  │   ├── ToolOutputDelta("Done.")
  │   └── ToolDone
  │
  └── Done
```

**用户体验差异**：
- 修正前：8 秒空白，然后一次性看到所有输出
- 修正后：50ms 开始看到内容，全程实时反馈

---

## 8.6 对其他文档的修正清单

### 01-overview.md

- [ ] 6.1 架构总览图：添加 `AgentEvent Stream` 层
- [ ] 6.5 依赖关系图：`AgentEvent` 类型应在 `pl-core` 中定义
- [ ] 添加流式设计原则："流式是默认，非流式是特例"

### 02-crates.md

- [ ] `pl-core`: 添加 `AgentEvent` 枚举和 channel 类型定义
- [ ] `LlmProvider` trait：删除 `complete()`，只保留 `stream_complete()`
- [ ] `Tool` trait：`execute()` → `execute_stream()`，添加 `event_tx` 参数
- [ ] `Runtime` trait：`execute()` → `execute_stream()`，返回 `ProcessResult` 而非 `ExecutionResult`
- [ ] `Compiler` trait：`compile()` → `compile_stream()`
- [ ] `Agent` trait：`handle_input()` 返回 `AgentSummary`，通过 `event_tx` 推送事件
- [ ] `pl-cli`：添加 `StreamRenderer` 和 `AdaptiveChunkingPolicy`

### 03-pipeline.md

- [ ] 编译管线改为事件驱动：每个阶段通过事件流推送进度
- [ ] `Plan::tasks` 改为逐个 `PlanTaskAdded` 事件
- [ ] `GeneratedCode::files` 改为逐个 `CodeGenerated` 事件
- [ ] 端到端数据流示例全部改为流式

### 04-security.md

- [ ] `ExecutionResult::stdout/stderr` 改为流式推送
- [ ] `SandboxConstraints` 添加输出流式相关约束（max_stream_size）

### 05-extension.md

- [ ] 自定义工具示例改为流式版本
- [ ] 自定义工具需实现 `execute_stream()` 而非 `execute()`

### 06-phases.md

- [ ] 流式从 P1 提升为 P0
- [ ] MVP 阶段必须包含流式渲染

### 07-model.md

- [ ] `CompletionRequest` 删除 `stream: bool` 字段
- [ ] `ModelProvider` trait 删除 `complete()` 方法
- [ ] 流式从 P1 提升为 P0
- [ ] 添加 SSE/WebSocket 流解析器设计

---

## 8.7 channel 容量与背压策略

```rust
/// channel 容量配置
pub struct ChannelConfig {
    /// AgentEvent 通道容量
    /// 过小：LLM 输出被背压阻塞（用户感知到卡顿）
    /// 过大：内存占用高，渲染延迟增加
    pub agent_event_capacity: usize,  // 默认 256

    /// Runtime stdout/stderr 通道容量
    pub process_output_capacity: usize,  // 默认 64
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            agent_event_capacity: 256,
            process_output_capacity: 64,
        }
    }
}
```

### 背压传播路径

```
CLI 渲染慢
    │
    ├── event_rx 消费慢 → event_tx.send() 阻塞
    │                       │
    │                       ├── Agent 发送事件慢
    │                       │   └── LLM 推送 Delta 慢
    │                       │       └── HTTP stream 背压到 TCP
    │                       │           └── 服务器暂停发送
    │                       │
    │                       └── Tool 推送输出慢
    │                           └── stdout 管道缓冲区满
    │                               └── 子进程 write() 阻塞
    │
    └── 最终效果：整个系统自动减速，不丢数据
```

---

## 8.8 事件流广播（多消费者）

同一事件需要同时推送给多个消费者：

```
AgentEvent Channel
    │
    ├── CLI/TUI 渲染（实时显示）
    ├── 审计日志（持久化记录）
    ├── 记忆系统（更新上下文）
    └── 测试断言（验证行为）
```

```rust
/// 事件广播器
pub struct EventBus {
    /// 使用 tokio::broadcast 实现多消费者
    inner: tokio::sync::broadcast::Sender<AgentEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(capacity);
        Self { inner: tx }
    }

    /// 发布事件（所有订阅者都会收到）
    pub fn publish(&self, event: AgentEvent) -> Result<()> {
        self.inner.send(event)?;
        Ok(())
    }

    /// 订阅事件流
    pub fn subscribe(&self) -> EventStream {
        EventStream {
            rx: self.inner.subscribe(),
        }
    }
}

pub struct EventStream {
    rx: tokio::sync::broadcast::Receiver<AgentEvent>,
}

impl EventStream {
    pub async fn next(&mut self) -> Option<AgentEvent> {
        self.rx.recv().await.ok()
    }
}
```

---

## 8.9 关键设计决策总结

| 决策 | 选择 | 理由 |
|------|------|------|
| 事件传输方式 | `tokio::sync::broadcast` | 多消费者、背压、取消 |
| 流式粒度 | 行级/增量级 | 平衡性能和可读性 |
| LLM 接口 | 只有 `stream_complete()` | 非流式通过流式 + collect 实现 |
| 工具输出 | 实时推送 stdout/stderr | 用户无需等待命令完成 |
| 管线输出 | 阶段事件 + 增量结果 | 用户能看到中间进度 |
| CLI 渲染 | 自适应分块 | 防止快速输出时闪烁 |
| 背压策略 | channel 容量 + 自动传播 | 系统整体减速，不丢数据 |
