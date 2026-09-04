---
name: add-tool
description: Use when defining, exposing, registering, or testing a Pure-Lang static, dynamic, hosted, MCP, LSP, plugin, or built-in tool through the unified DynTool runtime.
category: guides
platforms: ["windows", "linux", "macos"]
---

# 添加与注册 Tool

Pure-Lang 只有一条工具注册和执行链：

```text
StaticTool / typed builder ──From──┐
                                  ├── DynTool ── ToolInstallGroup ── ToolPlan
MCP / plugin / hosted ToolExecutor ┘
```

`pl-core` 公开工具契约、typed builder、所有可复用内置工具和安装容器。下游可以自由选择
需要的内置工具，与自己定义的工具放进同一个 `ToolInstallGroup`；不要新增按来源区分的 registry、
执行 enum 或兼容注册入口。

## 先确定契约

1. 工具名使用 `ToolName::bare` 或 `ToolName::namespaced` 构造，在定义边界完成校验。
2. Rust 静态工具输入使用 `DeserializeOwned + JsonSchema + Send + 'static`；字段 rustdoc 是模型看到的参数说明，
   `#[schemars(...)]` 表达长度、范围等结构约束，`#[serde(deny_unknown_fields)]` 拒绝未知字段。
3. `StaticToolDefinition` 显式提供工具总体用途；Schemars 不推断业务语义，也不替代 handler
   中依赖运行时状态的业务校验。
4. `ToolPolicy` 声明 effect、并行、批次、锁、缓存和预算语义。工具本身不创建第二套审批机制。
5. 内置、LSP、控制类和宿主静态工具通常使用 `Direct`；MCP、插件和大型动态目录通常使用
   `Deferred`，由 `tool_search` 在下一模型 step 揭示。
6. 跨工具工作流规则放在组级 developer instructions 中，不塞进单个参数 Schema。

## 使用 typed builder

产品或下游 crate 的普通 Rust 工具优先使用 `static_tool`：

```rust
use pl_core::{
    DynTool, StaticToolDefinition, ToolCallContext, ToolGroupId, ToolInstallGroup, ToolName,
    ToolPolicy, ToolResult, static_tool,
};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LookupInput {
    /// 要查询的业务键。
    #[schemars(length(min = 1, max = 128))]
    key: String,
}

let definition = StaticToolDefinition::new(
    ToolName::namespaced("host", "external_lookup")?,
    "查询宿主应用中的业务数据。",
);

let tool: DynTool = static_tool::<LookupInput>(definition)
    .policy(ToolPolicy::read_only())
    .build(|input, context: ToolCallContext| async move {
        context.check_cancelled()?;
        ToolResult::json(serde_json::json!({"key": input.key}))
    });

agent_tools.install(ToolInstallGroup::direct(
    ToolGroupId::new("embedding-application"),
    vec![tool],
))?;
```

builder 在构造时生成并缓存输入 Schema，并直接返回 `DynTool`。输入反序列化失败时不会进入 typed
handler。不要为 builder 工具再包装一次注册类型。

## 实现 `StaticTool`

需要保存复杂运行时依赖或复用实现类型时，直接实现 `StaticTool`：

```rust
use std::future::Future;
use pl_core::{
    Result, StaticTool, StaticToolDefinition, ToolCallContext, ToolName, ToolPolicy, ToolResult,
};

#[derive(Debug)]
struct ExternalLookupTool;

impl StaticTool for ExternalLookupTool {
    type Input = LookupInput;

    fn definition(&self) -> StaticToolDefinition {
        StaticToolDefinition::new(
            ToolName::namespaced("host", "external_lookup")
                .expect("static tool name is valid"),
            "查询宿主应用中的业务数据。",
        )
    }

    fn policy(&self) -> ToolPolicy {
        ToolPolicy::read_only()
    }

    fn execute(
        &self,
        input: Self::Input,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult>> + Send {
        async move {
            context.check_cancelled()?;
            ToolResult::json(serde_json::json!({"key": input.key}))
        }
    }
}

let tool: pl_core::DynTool = ExternalLookupTool.into();
```

`From<T: StaticTool>` 只做不会失败的类型擦除；名称等可失败校验必须在更早的构造边界完成，
Schema、重名、执行方式与 provider 能力则在安装/冻结边界验证。

## 动态、MCP、插件与 hosted 工具

运行时才知道定义的工具直接实现对象安全的 `ToolExecutor`，或使用
`DynamicToolExecutor`，再显式进入 newtype：

```rust
let executor = pl_core::DynamicToolExecutor::new(
    definition,
    policy,
    pl_core::ToolExecution::Local,
    |invocation| async move {
        let (_input, context) = invocation.into_parts();
        context.check_cancelled()?;
        Ok(pl_core::ToolResult::success("done"))
    },
);
let tool = pl_core::DynTool::new_executor(executor);
```

MCP adapter 保留 generation、turn lease 和关闭语义，hosted adapter 保留 provider 执行语义；
Registry 和 `ToolPlan` 只看到 `DynTool`，不按来源再次分派。不要同时实现会重叠的泛型
`From<T>`；动态来源使用 `DynTool::new_executor`。

## 选择并注册 `pl-core` 内置工具

内置工具实现与大多数构造器由 `pl-core` crate 根公开；远程 workspace 构造器保留在
`pl_core::remote` 命名空间。下游按能力自由组合，例如：

```rust
let mut tools: Vec<pl_core::DynTool> = vec![
    pl_core::AskUserTool.into(),
    pl_core::PlanSubmitTool.into(),
    pl_core::StatPathTool::new(tool_workspace.clone()).into(),
    pl_core::WriteFileTool::new(tool_workspace.clone()).into(),
];
tools.extend(pl_core::lsp_tools(lsp_registry, tool_workspace));

agent_tools.install(
    pl_core::ToolInstallGroup::direct(pl_core::ToolGroupId::new("selected-builtins"), tools)
        .with_developer_instructions(
            "先读取和确认现状，再修改 workspace；语义查询优先使用 LSP。",
        ),
)?;
```

主要公共构造入口包括：

- 文件与 workspace：`StatPathTool`、`WriteFileTool`、`CreateDirectoryTool`、
  `DeletePathTool`、`CopyPathTool`、`MovePathTool`、`WorkspaceFileTool`、
  `LocalWorkspaceFileTool`、`pl_core::remote::remote_workspace_mutation_tools`；
- 命令：`ExecTool`、`WriteStdinTool`、`command_tool_pair`、
  `local_command_tool_pair_with_environment`；
- LSP 与图片：`LspCapabilitiesTool`、`LspQueryTool`、`lsp_tools`、`ViewImageTool`；
- Git、Skill 与会话状态：`GitTool`、`SkillsListTool`、`SkillViewTool`、
  `SkillManageTool`、`skill_tools_from_catalog`、`SessionNoteTool`、`TodoListTool`、
  `WorkflowCurrentTool`、`WorkflowNextTool`、`WorkflowGraphTool`、`WorkflowHistoryTool`、
  `WorkflowTransitionTool`、`WorkflowRestartTool`；
- 控制与交互：`AskUserTool`、`PlanSubmitTool`、`CompleteTool`、
  `AgentCollaborationTools::tools`；
- 搜索：`WebSearchClient` + `WebSearchTool`，以及 provider-hosted
  `HostedWebSearchTool`。

构造器返回 `StaticTool` 实现时使用 `.into()`；已经返回 `Vec<DynTool>` 或本身实现
`ToolExecutor` 的入口不要重复包装。`command_tool_pair` 与
`local_command_tool_pair_with_environment` 返回具体工具元组，必须先解构，再分别调用 `.into()`。

## Direct、Deferred 与组提示

```rust
agent_tools.install(
    ToolInstallGroup::deferred(ToolGroupId::new("mcp:business"), mcp_tools)
        .with_developer_instructions(
            "这些工具只访问业务目录；先通过 tool_search 揭示最相关的工具。",
        ),
)?;
```

Deferred 目录的 fingerprint 和 reveal 状态属于当前 `AgentSession`。工具 generation 或策略变化时
旧 reveal 自动失效；子代理默认不继承父代理的揭示状态。`ToolPlan` 是不可变快照，旧 plan 继续
持有旧 executor，新模型 step 才看到替换后的 generation。

## 测试与检查

- 公共契约测试必须从 `pl_core` crate 根导入 API，证明下游无需私有模块。
- 覆盖 `StaticTool::into()`、builder、`DynTool::new_executor()` 以及同一 plan 混合执行。
- Schema 集中验证 rustdoc、枚举、范围、长度、必填项和未知字段；业务规则继续测 handler。
- 动态来源覆盖 generation/lease，deferred 来源覆盖搜索、下一 step 揭示和失效。
- 修改工具框架后运行：

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask verify-gui
git diff --check
```
