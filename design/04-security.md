# 04 - 安全模型

## 4.1 安全架构概述

Pure-Lang 的核心安全原则：**LLM 生成的代码不可信，所有执行必须在受限环境中进行。**

安全模型分为三个层次：

```
┌─────────────────────────────────────────┐
│           权限控制层 (Permission)         │
│    决定哪些操作需要用户确认               │
├─────────────────────────────────────────┤
│           工具策略层 (Tool Policy)        │
│    决定工具的执行条件和限制               │
├─────────────────────────────────────────┤
│           沙箱隔离层 (Sandbox)           │
│    物理隔离执行环境                       │
└─────────────────────────────────────────┘
```

---

## 4.2 权限层级

系统定义 5 个权限级别，从严格到宽松：

```
PermissionLevel::Ask (最严格)
    │  每个操作都需要用户显式确认
    │
PermissionLevel::Plan
    │  只生成规划，不执行任何操作
    │
PermissionLevel::AcceptEdits
    │  自动接受文件编辑，命令执行需确认
    │
PermissionLevel::Auto
    │  自动执行非破坏性操作，破坏性操作需确认
    │
PermissionLevel::Bypass (最宽松)
       全部自动执行，不请求确认（仅限受信环境）
```

### 权限决策流程

```
工具请求执行
    │
    ▼
检查工具的 DangerLevel
    │
    ├── Safe（只读操作）
    │   └── 所有权限级别均直接执行 ✓
    │
    ├── Moderate（写入操作）
    │   ├── Ask → 请求确认
    │   ├── Plan → 拒绝（不执行）
    │   ├── AcceptEdits → 直接执行 ✓
    │   ├── Auto → 直接执行 ✓
    │   └── Bypass → 直接执行 ✓
    │
    └── Dangerous（破坏性操作）
        ├── Ask → 请求确认
        ├── Plan → 拒绝（不执行）
        ├── AcceptEdits → 请求确认
        ├── Auto → 请求确认
        └── Bypass → 直接执行 ✓
```

### 确认请求格式

当操作需要用户确认时，Agent 发出 `ApprovalRequest` 事件：

```rust
pub struct ApprovalRequest {
    pub action: String,           // "执行命令" / "修改文件" / "删除文件"
    pub target: String,           // 具体目标（命令内容或文件路径）
    pub danger_level: DangerLevel,
    pub reason: String,           // 为什么需要执行此操作
    pub reversible: bool,         // 是否可逆
}

// 用户响应
pub enum ApprovalResponse {
    Allow,                        // 本次允许
    AllowAll,                     // 允许所有同类操作
    Deny,                         // 拒绝
    DenyAll,                      // 拒绝所有同类操作
}
```

---

## 4.3 工具执行策略

### 内置工具权限分类

| 工具 | 操作 | DangerLevel | 说明 |
|------|------|-------------|------|
| ReadFile | 读取文件 | Safe | 无副作用 |
| Search | 搜索内容 | Safe | 只读 |
| AskUser | 询问用户 | Safe | 仅交互 |
| WriteFile | 创建文件 | Moderate | 新增内容 |
| WriteFile | 修改文件 | Moderate | 变更内容 |
| Execute | 执行命令 | Dangerous | 任意代码执行 |

### 工具执行约束

每个工具执行都受 `SandboxConstraints` 约束：

```rust
pub struct SandboxConstraints {
    // 文件系统
    pub readable_paths: Vec<PathBuf>,     // 允许读取的路径
    pub writable_paths: Vec<PathBuf>,     // 允许写入的路径

    // 网络
    pub allow_network: bool,              // 是否允许网络访问

    // 资源限制
    pub max_memory: Option<usize>,        // 最大内存（字节）
    pub max_cpu_time: Option<Duration>,   // 最大 CPU 时间
    pub max_output_size: Option<usize>,   // 最大输出大小（字节）
}
```

### 默认约束策略

```rust
impl Default for SandboxConstraints {
    fn default() -> Self {
        Self {
            readable_paths: vec![workdir.clone()],
            writable_paths: vec![workdir.clone()],
            allow_network: false,
            max_memory: Some(512 * 1024 * 1024),  // 512MB
            max_cpu_time: Some(Duration::from_secs(60)),
            max_output_size: Some(1024 * 1024),    // 1MB
        }
    }
}
```

---

## 4.4 沙箱隔离架构

### 隔离层次

```
┌──────────────────────────────────────────────────┐
│                    Host OS                        │
│                                                   │
│  ┌────────────────────────────────────────────┐  │
│  │          Pure-Lang 主进程                    │  │
│  │                                            │  │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐ │  │
│  │  │  Agent   │  │  Tool    │  │  Memory  │ │  │
│  │  │  Loop    │  │ Registry │  │  Store   │ │  │
│  │  └──────────┘  └────┬─────┘  └──────────┘ │  │
│  │                      │                      │  │
│  │              ┌───────▼───────┐              │  │
│  │              │   Sandbox     │              │  │
│  │              │   Manager     │              │  │
│  │              └───────┬───────┘              │  │
│  └──────────────────────┼─────────────────────┘  │
│                          │                        │
│         ┌────────────────▼────────────────┐      │
│         │         Sandbox 进程             │      │
│         │                                 │      │
│         │  限制：                          │      │
│         │  ├─ 文件系统：仅工作目录          │      │
│         │  ├─ 网络：禁止                   │      │
│         │  ├─ 内存：受限                   │      │
│         │  ├─ CPU 时间：受限               │      │
│         │  ├─ 子进程：禁止或受限            │      │
│         │  └─ 环境变量：最小集合            │      │
│         │                                 │      │
│         │  stdin ──> 命令输入              │      │
│         │  stdout <── 捕获输出             │      │
│         │  stderr <── 捕获错误             │      │
│         └─────────────────────────────────┘      │
└──────────────────────────────────────────────────┘
```

### 平台沙箱实现

#### 首版：进程级沙箱（跨平台）

```rust
pub struct ProcessRuntime {
    // 基于 tokio::process::Command
    // 通过约束参数限制子进程行为
}

#[async_trait]
impl Runtime for ProcessRuntime {
    async fn execute(&self, request: ExecutionRequest) -> Result<ExecutionResult> {
        let mut child = tokio::process::Command::new(&request.command);
        child
            .args(&request.args)
            .current_dir(&request.workdir)
            .env_clear()
            .envs(&request.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // 超时控制
        let output = tokio::time::timeout(
            request.timeout,
            child.output(),
        ).await??;

        Ok(ExecutionResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into(),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
            duration: elapsed,
            timed_out: false,
        })
    }
}
```

#### 后续：OS 原生沙箱

| 平台 | 技术 | 隔离能力 |
|------|------|---------|
| Linux | Landlock + Bubblewrap | 文件系统、网络、PID 命名空间 |
| macOS | Seatbelt (sandbox-exec) | 文件系统、网络、进程 |
| Windows | Job Object + 受限令牌 | 内存、CPU、文件系统 |

---

## 4.5 错误处理体系

### 错误分类

```rust
pub enum ErrorSeverity {
    Warning,     // 可继续，但需注意
    Recoverable, // 可自动恢复
    Fatal,       // 需要用户介入
}
```

### 错误恢复策略

| 错误类型 | 严重度 | 恢复策略 |
|---------|--------|---------|
| LLM API 超时 | Recoverable | 指数退避重试，最多 3 次 |
| LLM API 限流 | Recoverable | 等待 `Retry-After` 后重试 |
| LLM 输出格式错误 | Recoverable | 重新请求，附带格式纠正提示 |
| 工具执行失败（编译错误） | Recoverable | Agent 分析错误，修改代码重试 |
| 工具执行失败（权限不足） | Fatal | 请求用户调整权限或方案 |
| 沙箱超时 | Recoverable | 终止进程，Agent 决定是否重试 |
| 沙箱内存溢出 | Recoverable | 终止进程，调整约束后重试 |
| 上下文溢出 | Recoverable | 压缩上下文，保留关键信息 |
| 配置文件错误 | Fatal | 提示用户修复配置 |
| 检查点恢复失败 | Fatal | 提示用户手动介入 |

### 检查点机制

Agent 在关键节点自动创建检查点：

```rust
pub struct Checkpoint {
    pub id: CheckpointId,
    pub timestamp: DateTime<Utc>,
    pub agent_state: AgentState,
    pub memory_snapshot: MemorySnapshot,
    pub description: String,
}

// 检查点触发时机：
// 1. 每个 Task 执行成功后
// 2. 用户确认操作后
// 3. Agent 循环每 5 次迭代
// 4. 执行危险操作前
```

---

## 4.6 审计日志

所有操作记录审计日志，用于回溯和安全审查：

```rust
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub session_id: SessionId,
    pub event: AuditEvent,
}

pub enum AuditEvent {
    ToolInvoked { tool: String, input: String, result: String },
    PermissionGranted { action: String, level: PermissionLevel },
    PermissionDenied { action: String, reason: String },
    SandboxExecution { command: String, exit_code: i32 },
    CheckpointCreated { id: CheckpointId },
    CheckpointRestored { id: CheckpointId },
    LlmRequest { model: String, tokens: TokenUsage },
}
```
