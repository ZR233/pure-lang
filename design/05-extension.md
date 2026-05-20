# 05 - 扩展点与配置

## 5.1 自定义工具注册

用户可以通过实现 `Tool` trait 来注册自定义工具：

```rust
use pl_tool::{Tool, ToolDefinition, ToolInput, ToolResult, ToolExecutionContext};
use pl_core::AgentEventSender;

struct GitLogTool;

impl Tool for GitLogTool {
    fn definition(&self) -> &ToolDefinition {
        static DEF: Lazy<ToolDefinition> = Lazy::new(|| ToolDefinition {
            name: "git_log".into(),
            description: "查看 Git 提交日志".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "count": { "type": "number", "description": "显示条数" }
                }
            }),
            category: ToolCategory::Execution,
            danger_level: DangerLevel::Safe,
        });
        &DEF
    }

    fn execute_stream(
        &self,
        input: ToolInput,
        ctx: &ToolExecutionContext,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<ToolResult>> + Send {
        async move {
            let count = input.arguments["count"].as_u64().unwrap_or(10);

            // 流式执行 git log 命令
            let mut child = tokio::process::Command::new("git")
                .args(["log", &format!("-{}", count)])
                .current_dir(&ctx.workdir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            let stdout = child.stdout.take().unwrap();
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            let mut output = String::new();
            while let Some(line) = lines.next_line().await? {
                output.push_str(&line);
                output.push('\n');
                // 推送增量输出
                let _ = event_tx.send(AgentEvent::ToolOutputDelta {
                    id: input.name.clone(),
                    content: line,
                });
            }

            let status = child.wait().await?;
            Ok(ToolResult {
                content: output,
                is_error: !status.success(),
                metadata: HashMap::new(),
            })
        }
    }
}

// 注册
registry.register(Box::new(GitLogTool))?;
```

### 工具发现机制

```
工具来源：
├── 内置工具        代码中硬编码，随程序发布
├── 项目工具        项目 .pure/tools/ 目录下的动态库
├── 用户工具        ~/.pure/tools/ 目录下的动态库
└── MCP 工具       通过 MCP 协议连接的外部工具服务
```

---

## 5.2 自定义记忆后端

通过实现 `MemoryStore` trait 可以接入不同的存储后端：

```rust
impl MemoryStore for SqliteMemoryStore {
    fn store(
        &self,
        entry: MemoryEntry,
    ) -> impl std::future::Future<Output = Result<MemoryId>> + Send {
        async move {
            // INSERT INTO memories (id, content, type, scope, timestamp, relevance)
            // VALUES (?, ?, ?, ?, ?, ?)
        }
    }

    fn retrieve(
        &self,
        query: &MemoryQuery,
    ) -> impl std::future::Future<Output = Result<Vec<MemoryEntry>>> + Send {
        async move {
            // SELECT * FROM memories WHERE scope = ? AND type = ?
            // ORDER BY relevance DESC LIMIT ?
        }
    }

    fn delete(
        &self,
        id: &MemoryId,
    ) -> impl std::future::Future<Output = Result<()>> + Send {
        async move {
            // DELETE FROM memories WHERE id = ?
        }
    }

    fn search(
        &self,
        keywords: &[&str],
        limit: usize,
    ) -> impl std::future::Future<Output = Result<Vec<MemoryEntry>>> + Send {
        async move {
            // SELECT * FROM memories
            // WHERE content LIKE '%keyword1%' OR content LIKE '%keyword2%'
            // ORDER BY timestamp DESC LIMIT ?
        }
    }
}
```

### 预期实现路线

| 阶段 | 后端 | 说明 |
|------|------|------|
| MVP | `InMemoryStore` | 纯内存，用于开发和测试 |
| Phase 1 | `FileMemoryStore` | JSONL 文件存储，零依赖持久化 |
| Phase 2 | `SqliteMemoryStore` | SQLite，支持高效查询 |
| Phase 3 | 向量存储 | 支持语义搜索的记忆后端 |

---

## 5.3 技能配置格式

### 目录结构

```
~/.pure/skills/                   # 用户全局技能
    └── rust-http-server/
        ├── skill.toml             # 技能元数据
        └── prompt.md              # 提示模板

.pure/skills/                     # 项目级技能
    └── api-design/
        ├── skill.toml
        └── prompt.md

<install-prefix>/share/pure/skills/   # 系统内置技能
    └── code-review/
        ├── skill.toml
        └── prompt.md
```

### skill.toml 格式

```toml
[metadata]
name = "rust-http-server"
version = "1.0.0"
description = "创建基于 axum 的 Rust HTTP 服务器"
scope = "user"                  # system / user / project

[tools]
required = ["read_file", "write_file", "execute"]   # 需要的工具

[trigger]
# 隐式触发关键词（用于 detect_implicit）
keywords = ["http", "server", "web", "api"]
# 显式调用命令
command = "/rust-http"
```

### prompt.md 格式

```markdown
# Rust HTTP 服务器技能

你是一个 Rust Web 开发专家，专注于 axum 框架。

## 创建步骤

1. 使用 `cargo init` 初始化项目
2. 在 Cargo.toml 添加依赖：
   - axum = "0.8"
   - tokio = { version = "1", features = ["full"] }
3. 实现 handler 函数
4. 配置路由
5. 启动服务器

## 代码规范

- 使用 Result 类型处理错误
- handler 函数使用 async fn
- 路由使用 axum::Router 的 nest API

## 注意事项

- 默认端口 3000
- 需要添加 graceful shutdown
```

---

## 5.4 项目配置文件

### pure.toml（项目级配置）

放在项目根目录，控制 pure-lang 在该项目中的行为：

```toml
# pure.toml

[agent]
permission_level = "accept_edits"     # 权限级别
max_iterations = 20                   # 单次请求最大 Agent 循环次数
auto_compact = true                   # 自动压缩上下文

[model]
provider = "openai"                   # openai / anthropic / ollama
model = "gpt-4"                       # 模型名称
temperature = 0.3                      # 生成温度
max_tokens = 4096                     # 单次最大输出 token

[runtime]
sandbox = "process"                   # process / native / container / none
timeout = "30s"                       # 命令执行超时
max_memory = "512MB"                  # 最大内存

[memory]
backend = "file"                      # memory / file / sqlite
compaction_strategy = "summarize"     # sliding_window / summarize / importance
max_context_tokens = 100000           # 上下文窗口最大 token 数

[tools]
# 允许的工具白名单（空 = 全部允许）
allow = []
# 禁止的工具黑名单
deny = []

[skills]
# 激活的技能
active = ["rust-http-server", "code-review"]
# 额外的技能搜索路径
search_paths = ["./custom-skills"]
```

### PURE.md（项目知识文件）

类似 Claude Code 的 `CLAUDE.md`，放在项目根目录。系统启动时自动加载到上下文中：

```markdown
# PURE.md

## 项目信息
- 语言: Rust (Edition 2024)
- 框架: Axum
- 数据库: PostgreSQL (通过 SQLx)
- 构建: Cargo Workspace

## 架构决策
- 使用 Repository 模式隔离数据访问
- API 遵循 REST 规范
- 错误处理统一使用 thiserror + anyhow

## 编码规范
- 公开 API 需要文档注释
- 使用 #[tracing::instrument] 记录关键函数
- 测试使用 #[tokio::test]

## 目录结构
- src/handlers/   - HTTP handler
- src/models/     - 数据模型
- src/repo/       - 数据访问层
- src/services/   - 业务逻辑
```

---

## 5.5 MCP 协议集成

MCP（Model Context Protocol）是一种标准化的工具集成协议。未来可通过 MCP 接入外部工具服务：

```rust
pub struct McpToolAdapter {
    client: McpClient,
    definition: ToolDefinition,
}

impl Tool for McpToolAdapter {
    fn definition(&self) -> &ToolDefinition {
        &self.definition
    }

    fn execute_stream(
        &self,
        input: ToolInput,
        _ctx: &ToolExecutionContext,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<ToolResult>> + Send {
        async move {
            let response = self.client
                .call_tool(&input.name, input.arguments)
                .await?;

            Ok(ToolResult {
                content: response.content,
                is_error: response.is_error,
                metadata: response.metadata,
            })
        }
    }
}
```

### MCP 配置

```toml
# pure.toml

[mcp.servers.filesystem]
command = "mcp-server-filesystem"
args = ["/path/to/project"]

[mcp.servers.github]
command = "mcp-server-github"
env = { GITHUB_TOKEN = "${GITHUB_TOKEN}" }
```

---

## 5.6 LLM Provider 配置

支持多个 LLM 提供者，通过配置切换：

```toml
[model]
provider = "deepseek"  # 当前使用的提供者

[model.providers.deepseek]
api_key_env = "API_KEY_DEEPSEEK"
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
context_window = 1000000

[model.providers.openai]
api_key_env = "API_KEY_OPENAI"
base_url = "https://api.openai.com/v1"
model = "gpt-5.5"
context_window = 1050000
```

### Fallback 策略

```toml
[model.fallback]
enabled = true
chain = ["openai", "anthropic", "ollama"]  # 依次尝试
```
