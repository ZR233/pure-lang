# 06 - 实施阶段

## 6.1 总体路线图

```
Phase 1 (MVP)          Phase 2              Phase 3
最小可运行版本          功能完善              生态建设
──────────────    ────────────────    ────────────────
 pl-core           pl-skill 完善        MCP 协议支持
 pl-model          FileMemoryStore      向量记忆后端
 pl-tool           上下文压缩            插件市场
 pl-memory         OS 原生沙箱          TUI 界面
 pl-runtime        子代理并行            多语言支持
 pl-compiler       审计日志              Web UI
 pl-agent
 pl-cli
```

---

## 6.2 Phase 1: MVP（最小可运行版本）

### 目标

实现一个端到端可运行的自然语言编译器，能接收用户输入，分析意图，生成代码，并在沙箱中执行。

### 各 Crate 实现优先级

| 优先级 | Crate | 实现范围 |
|-------|-------|---------|
| P0 | `pl-core` | PureError、Message、PermissionLevel、AgentEvent、AgentEventSender |
| P0 | `pl-model` | ModelProvider trait、WireDispatch/WireAdapter、ModelsManager、OpenAI 兼容实现 |
| P0 | `pl-tool` | Tool trait（流式）、ToolRegistry、ReadFile、WriteFile、Execute 三个内置工具 |
| P0 | `pl-runtime` | Runtime trait（流式）、ProcessRuntime（受限子进程，跨平台） |
| P0 | `pl-memory` | MemoryStore trait、InMemoryStore、基础 ContextManager |
| P0 | `pl-compiler` | Compiler trait（流式）、IntentAnalyzer、CodeGenerator（LLM prompt 驱动） |
| P0 | `pl-agent` | Agent trait（流式）、PureAgent 基础 ReAct 循环 |
| P0 | `pl-cli` | clap 参数解析、基础交互循环、AgentEvent 流式渲染 |
| P0 | `pure-lang` | main.rs 入口，组装所有组件 |
| P1 | `pl-skill` | SkillMetadata、SkillManager 基础加载（从目录） |

### 开发阶段

#### 阶段 A: 基础骨架

1. 创建所有 crate 目录结构和 Cargo.toml
2. 实现 `pl-core` 基础类型（PureError、Message、PermissionLevel、AgentEvent）
3. 实现 `pl-model` 的 `ModelProvider` trait + OpenAI 兼容 API 实现（流式）
4. 编写配置加载逻辑（pure.toml）

**产出**：所有 crate 编译通过，LLM Provider 可流式调用

#### 阶段 B: 工具系统

5. 实现 `Tool` trait（流式 execute_stream）和 `ToolRegistry`
6. 实现 `ReadFile`、`WriteFile`、`Execute` 三个内置工具
7. 编写工具系统单元测试

**产出**：工具可注册、查询、流式执行

#### 阶段 C: 运行时与记忆

8. 实现 `Runtime` trait（流式 execute_stream）和 `ProcessRuntime`
9. 实现 `MemoryStore` trait 和 `InMemoryStore`
10. 实现 `ContextManager` 基础功能（消息添加、上下文窗口构建）

**产出**：命令可在受限子进程中执行，上下文可追踪

#### 阶段 D: 编译管线

11. 实现 `Compiler` trait（流式 compile_stream）
12. 实现 `IntentAnalyzer`（LLM prompt + JSON 输出解析）
13. 实现 `CodeGenerator`（LLM prompt + 代码提取）
14. 实现 `Planner`（LLM prompt + 任务分解）

**产出**：NL 输入可被分析意图并流式生成代码

#### 阶段 E: Agent 循环

15. 实现 `Agent` trait（流式 handle_input）
16. 实现 `PureAgent` 的 ReAct 循环（通过 AgentEventSender 推送事件）
17. 集成工具调用和错误恢复
18. 集成记忆系统

**产出**：Agent 可自主完成多步任务

#### 阶段 F: CLI 与集成

19. 实现 CLI 参数解析（clap）
20. 实现交互式循环
21. 实现 AgentEvent 流式渲染
22. 在 `pure-lang` main.rs 中组装所有组件
23. 端到端集成测试

**产出**：完整的可运行程序

### MVP 验收标准

- [ ] 用户能通过命令行输入自然语言描述
- [ ] 系统能分析意图并流式生成代码
- [ ] 代码能在沙箱（受限子进程）中执行，输出通过 channel 流式推送
- [ ] 执行结果能实时反馈给用户
- [ ] 文件读写和命令执行工具正常工作
- [ ] 上下文能在单次会话中持续
- [ ] 基本的权限控制生效（危险操作需确认）
- [ ] AgentEvent stream 作为统一的进度推送通道

---

## 6.3 Phase 2: 功能完善

| 功能 | Crate | 说明 |
|------|-------|------|
| 技能系统完善 | pl-skill | 提示模板渲染、隐式触发、项目级技能 |
| 文件持久化 | pl-memory | FileMemoryStore（JSONL 格式） |
| 上下文压缩 | pl-memory | Summarize 策略（LLM 辅助摘要） |
| OS 原生沙箱 | pl-runtime | Windows Job Object / Linux Landlock |
| 子代理 | pl-agent | 并行任务执行 |
| 审计日志 | pl-core | 操作审计记录 |
| 检查点 | pl-memory | Agent 状态快照与恢复 |
| 搜索工具 | pl-tool | 代码搜索（grep/ripgrep） |
| Anthropic Provider | pl-model | Claude API 支持 |

---

## 6.4 Phase 3: 生态建设

| 功能 | 说明 |
|------|------|
| MCP 协议 | 接入外部工具服务器 |
| 向量记忆 | 语义搜索的记忆后端 |
| TUI 界面 | 基于 ratatui 的富终端界面 |
| 插件系统 | 动态加载 .dll/.so 工具插件 |
| 多 LLM Fallback | 主 provider 失败自动切换 |
| Web UI | 可选的 Web 管理界面 |
| 多语言生成 | 支持生成 Python、Go、TypeScript 等语言的代码 |

---

## 6.5 技术选型汇总

| 用途 | 选型 | 说明 |
|------|------|------|
| 异步运行时 | tokio | 业界标准，功能完善 |
| HTTP 客户端 | reqwest | LLM API 调用（stream 特性） |
| 序列化 | serde + serde_json | 配置和数据 |
| 错误处理 | thiserror + anyhow | 库用 thiserror，应用用 anyhow |
| CLI 解析 | clap (derive) | 声明式参数定义 |
| 终端渲染 | crossterm | 跨平台终端控制 |
| 日志 | tracing + tracing-subscriber | 结构化日志 |
| 模板引擎 | tera | 技能提示模板渲染 |
| 配置 | toml | 项目配置格式 |
| 位标志 | bitflags | ModelCapabilities 等位标志集 |
| 测试 | tokio::test + mockall | 异步测试 + mock |

---

## 6.6 项目结构（最终）

```
pure-lang/
├── Cargo.toml                  # Workspace 配置
├── Cargo.lock
├── LICENSE                     # Apache 2.0
├── .gitignore
├── pure.toml                   # 项目配置（示例）
├── PURE.md                     # 项目知识（示例）
│
├── design/                     # 设计文档
│   ├── 01-overview.md
│   ├── 02-crates.md
│   ├── 03-pipeline.md
│   ├── 04-security.md
│   ├── 05-extension.md
│   └── 06-phases.md
│
└── code/
    ├── pl-core/                # 核心抽象
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── error.rs
    │       ├── message.rs
    │       ├── event.rs
    │       ├── permission.rs
    │       └── config.rs
    │
    ├── pl-model/               # LLM Provider 层
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── default_models.rs
    │       ├── model_info.rs
    │       ├── provider.rs
    │       ├── provider_info.rs
    │       ├── request.rs
    │       ├── wire_api.rs
    │       ├── sse.rs
    │       ├── manager.rs
    │       ├── capabilities.rs
    │       └── openai.rs
    │
    ├── pl-tool/                # 工具系统
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── trait.rs
    │       ├── registry.rs
    │       └── builtin/
    │           ├── mod.rs
    │           ├── read_file.rs
    │           ├── write_file.rs
    │           └── execute.rs
    │
    ├── pl-skill/               # 技能系统
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── manager.rs
    │       └── types.rs
    │
    ├── pl-memory/              # 记忆系统
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── store.rs
    │       ├── context.rs
    │       └── compaction.rs
    │
    ├── pl-runtime/             # 沙箱运行时
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── trait.rs
    │       └── process.rs
    │
    ├── pl-compiler/            # 编译管线
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── trait.rs
    │       ├── intent.rs
    │       ├── planner.rs
    │       └── generator.rs
    │
    ├── pl-agent/               # Agent 循环
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── trait.rs
    │       ├── agent.rs
    │       └── state.rs
    │
    ├── pl-cli/                 # CLI 交互
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── args.rs
    │       └── render.rs
    │
    └── pure-lang/              # 主程序入口
        ├── Cargo.toml
        └── src/
            └── main.rs
```
