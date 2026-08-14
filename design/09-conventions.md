# 09 - 约定

## 9.1 Crate 命名

- 库 crate 使用 `pl-` 前缀。
- Flutter bridge crate 使用 `pl-studio-bridge`，Flutter app package 使用 `pure_studio`。
- 公共协议类型放入 `pl-protocol`。

## 9.2 依赖方向

```text
pl-protocol
    ↑
pl-trace
    ↑
pl-model
    ↑
pl-core
    ↑
pl-studio-bridge
    ↑
pure-studio
```

允许 `pl-core` 同时直接依赖 `pl-protocol`、`pl-trace`、`pl-model` 和 `pl-lsp`。

禁止 `pl-model` 依赖 `pl-core`，避免循环依赖。

## 9.3 异步 Trait

禁止使用 `#[async_trait]` 和 `#[allow(async_fn_in_trait)]`。

异步 trait 方法使用原生 RPITIT，并显式声明 `Send` bound：

```rust
pub trait ModelProvider: Send + Sync {
    fn stream_complete(&self, request: CompletionRequest, event_tx: AgentEventSender)
        -> impl std::future::Future<Output = Result<CompletionResponse>> + Send;
}
```

## 9.4 参数设计

核心 API 不暴露语义模糊的 `bool` 或 `Option<bool>`。

前端输入应在 `pure-studio` 边界转换为明确类型，例如 `CompileMode`。

工具 schema 必须完整描述影响参数有效性的约束。分页 cursor 只能与生成它的请求投影配套使用；续页必须保留 cursor 所绑定的过滤、路径和匹配参数，工作区发生变更后旧 cursor 失效。

Codex patch 的 Update hunk 每行首字符是控制前缀：空格表示上下文、`-` 表示删除、`+` 表示新增。内容本身以 `-` 或 `+` 开头时，该字符必须放在控制前缀之后；例如把 Markdown 项目符号 `- old` 替换为 `- new` 时，删除行写作 `-- old`，新增行写作 `+- new`。

## 9.5 模块和导出

模块默认私有。公开 API 通过 crate 根明确 `pub use`。

`pl-core` 可以重导出常用 `pl-protocol` 类型，方便核心层用户使用；raw `pl-trace` 类型只作为内部运行事件边界，不应作为 Studio wire 或前端事实源。

## 9.6 文档口径

- 项目名：Pure-Lang。
- 桌面编译器前端：`pure-studio`。
- 核心逻辑层：`pl-core`。
- LLM provider 层：`pl-model`。
- 公共协议层：`pl-protocol`。
- 内部 trace 协议层：`pl-trace`。

当前版本不承诺独立沙箱。工具系统必须由明确 `PermissionMode`、execution policy 和工具访问分类控制；默认模式为 `request-approval`，不保留独立审批策略。

## 9.8 后台进程约定

- GUI 运行时派生 shell、git、MCP server、LSP 等后台子进程时，Windows 必须使用
  `CREATE_NO_WINDOW`，禁止弹出新的命令行窗口；Unix 使用独立进程组便于整树回收。
- 进程配置的唯一工厂是 `pl_core::process`（`configure_background_command`、
  `configure_background_std_command` 和 `wrap_background_command`），原生 Command 与
  `process-wrap`/Job Object 路径都必须从该工厂取得等价策略，其他 crate 不得复制实现；`pl-lsp` 因依赖
  方向（pl-core → pl-lsp）保留自己的 `spawn_background` 统一入口，语义与
  pl-core 工厂等价；`pl-xtask` 在自身 process 模块内统一配置，所有子进程
  创建入口必须经过它。
- stdio MCP 配置保存跨平台命令名，不写入 `.cmd` 等平台后缀，也不统一套 `pwsh`/shell；
  connector 在 Windows 按 `PATHEXT` 解析 CreateProcess 可执行目标，并保持 `shell=false` 语义；
  标准 npm/npx launcher 必须直接展开为 `node.exe + npm CLI`，不得让长期运行的 MCP 连接
  由 `.cmd` shim 持有。
  stdin、stdout、stderr 必须全部管道化并消费，禁止继承启动 Studio 的终端；stderr 只允许以
  有界、凭证脱敏的形式补充启动错误。
- MCP 客户端优先使用 `server/discover` 协商已知协议版本；仅当对端明确返回
  `METHOD_NOT_FOUND`、证明它是传统协议服务时，回退标准 `initialize` 协商。不得把网络失败
  或任意协议错误当成旧版本并盲目重试，也不得为单一 provider 写专用版本特判。
- 启动路径的慢能力（MCP 探测、LSP probe）一律后台异步执行，结果经产品事件
  流推送，不阻塞主界面骨架。
- 单个 MCP 启动或探测失败必须归属到该 server 的运行时 health，投影为
  `unavailable` 和有界、脱敏的错误消息；配置启用态与运行时可用态不得混用，
  单个 MCP 失败也不得阻塞 Studio shell。

## 9.7 配置约定

- 配置文件固定为 `~/.pure/config.toml`。
- 本地 TOML 使用 `snake_case`。
- 不设置 `active_provider`。
- 固定角色 key：`explorer`、`planner`、`executor`、`reviewer`。
- 普通对话默认使用 `planner`。
- provider 必须持久化完整 models 列表，以支持用户自定义模型。
