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
pub trait HealthProbe: Send + Sync {
    fn check(&self)
        -> impl std::future::Future<Output = Result<HealthSnapshot>> + Send;
}
```

## 9.4 参数设计

核心 API 不暴露语义模糊的 `bool` 或 `Option<bool>`。

前端输入应在 `pure-studio` 边界转换为明确类型，例如 `CompileMode`。

工具 schema 必须完整描述影响参数有效性的约束。分页 cursor 只能与生成它的请求投影配套使用；续页必须保留 cursor 所绑定的过滤、路径和匹配参数，工作区发生变更后旧 cursor 失效。

Codex patch 的 Update hunk 每行首字符是控制前缀：空格表示上下文、`-` 表示删除、`+` 表示新增。内容本身以 `-` 或 `+` 开头时，该字符必须放在控制前缀之后；例如把 Markdown 项目符号 `- old` 替换为 `- new` 时，删除行写作 `-- old`，新增行写作 `+- new`。

## 9.5 模块和导出

模块默认私有。稳定领域边界优先使用 `pub mod` 形成可读命名空间；只有少量高频入口
确实适合上层时，才使用精确 `pub use`。当一个模块的全部公共项都属于上层稳定 API 时，
允许 `pub use module::*` 简化目录页；内部 `raw`/`imp`/`sys`/`unsafe_impl` 边界不得被通配暴露。
同一公开接口只保留一条 canonical 路径，无明确跨版本兼容责任时不保留根与子模块双轨导出。
单一职责模块使用 `foo.rs`；模块拥有多个真实、内聚的子职责时才使用
`foo/mod.rs` + `foo/child.rs`。禁止只有 `mod.rs` 的目录，也禁止 `mod.rs` 只转发唯一子文件。

`pl-core` 可以在自身领域边界重导出常用 `pl-protocol` 类型，方便核心层用户使用；`pl-studio-runtime`
在 crate 根按公共签名实际使用的类型精确重导出 `pl-protocol` 项；两者都不代理重导出其他
专项 crate 的整套 API。raw `pl-trace` 类型只作为内部运行事件边界，不应作为 Studio wire 或前端事实源。

### 9.5.1 生命周期状态机

具有时间顺序、非法转换、终态或恢复语义的领域对象统一使用单一状态机聚合：身份和跨状态上下文
位于 aggregate，唯一可写状态是带 payload 的 enum。每个状态 variant 承载字段私有的独立 struct，
并按 `state/mod.rs + state/<variant>.rs` 组织；只适用于某个状态的时间、失败、进度和结果不得提升为
aggregate 上的平行 `Option`、布尔值或第二个 status enum。

状态变化只接受语义明确的 command，并返回包含 next state、durable effects 和 external effects 的
decision。状态模块是纯领域代码，不执行 IO、等待、加锁或外部回调；adapter 在事务和 lifecycle
边界解释 effects。禁止通用 `set_state`、`can_transition_to`、`from_parts` 兼容构造、`dyn State` 和
泛型 typestate。终态不可继续迁移；恢复必须是从指定可恢复状态出发的显式 command。重复 operation、
mail 或 revision 只有完全命中幂等身份时才是 no-op，其他同态命令和过期 revision 必须返回 typed
transition error。

公共协议的生命周期统一使用
`#[serde(tag = "kind", content = "data", rename_all = "camelCase")]` tagged enum；Dart 使用
sealed union 并穷尽匹配。SQLite 对需要查询的生命周期保存完整 `state_json`，`state_kind` 只能是从
JSON discriminator 生成的 stored column。普通分类、配置、能力、scope、transport、severity、解析
游标和没有迁移规则的一次性结果继续使用普通 enum，不为形式统一强行引入状态机。

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
  `CREATE_NO_WINDOW`，禁止弹出新的命令行窗口；Job Object 路径在实际 CreateProcess 前必须
  最终合并 `CREATE_SUSPENDED | CREATE_NO_WINDOW`，不得依赖其他 wrapper 在 Job Object 覆盖后
  恢复 flags。Unix 使用独立进程组便于整树回收。
- 进程配置的唯一工厂是 `pl_core::process`（`configure_background_command`、
  `configure_background_std_command` 和 `wrap_background_command`），原生 Command 与
  `process-wrap`/Job Object 路径都必须从该工厂取得等价策略，其他 crate 不得复制实现；`pl-lsp` 因依赖
  方向（pl-core → pl-lsp）保留自己的 `spawn_background` 统一入口，语义与
  pl-core 工厂等价。`pl-xtask` 的同步构建、生成、签名等前台命令使用普通进程并继承终端
  stdout/stderr，以便实时显示编译过程；只有驻留命令使用 xtask 自身 process 模块的后台配置和
  进程树托管。Windows GUI 双击回归必须同时检查传统 `ConsoleWindowClass` 和现代终端
  使用的 `PseudoConsoleWindow`，不能只凭控制台启动或只检查传统窗口类判定无弹窗。
- stdio MCP 配置保存跨平台命令名，不写入 `.cmd` 等平台后缀，也不统一套 `pwsh`/shell；
  connector 在 Windows 按 `PATHEXT` 解析 CreateProcess 可执行目标，并保持 `shell=false` 语义；
  标准 npm/npx launcher 必须直接展开为 `node.exe + npm CLI`，不得让长期运行的 MCP 连接
  由 `.cmd` shim 持有。由于 Windows creation flags 不会自动传递到后代，npm CLI 创建包级
  launcher 时还必须显式启用 Node 的 `windowsHide`，禁止其再次通过可见的 `.cmd` 控制台启动
  MCP server；该约束只作用于 npm launcher，不通过 `NODE_OPTIONS` 传播给 MCP server。
  stdin、stdout、stderr 必须全部管道化并消费，禁止继承启动 Studio 的终端；stderr 只允许以
  有界、凭证脱敏的形式补充启动错误。
- MCP 客户端优先使用 `server/discover` 协商已知协议版本；仅当对端明确返回
  `METHOD_NOT_FOUND`、证明它是传统协议服务时，回退标准 `initialize` 协商。Streamable HTTP
  transport 若把 discovery bootstrap 的终止 SSE 折叠为关闭的 discover response，必须用全新
  transport 重试标准 `initialize`；该兼容重试不得扩展到认证、超时或其他任意协议错误，也不得为
  单一 provider 写专用版本特判。
- 启动路径的慢能力（MCP 探测、LSP probe）一律后台异步执行，结果经产品事件
  流推送，不阻塞主界面骨架。
- 单个 MCP 启动或探测失败必须归属到该 server 的运行时 health，投影为
  `unavailable` 和有界、脱敏的错误消息；配置启用态与运行时可用态不得混用，
  单个 MCP 失败也不得阻塞 Studio shell。

## 9.9 Studio 生成文件约定

- `code/pure-studio/pubspec.lock` 是应用级 canonical 依赖快照，必须由 Git 跟踪且不得被任何仓库
  ignore 规则排除。Flutter 直接依赖允许跨 major 升级，默认采用当前 stable SDK 可解析的稳定版本；
  prerelease 必须有独立需求，或由已选择的稳定直接依赖求解强制要求，并经过完整生成与测试验证。
  当前 `freezed 4.0.0-dev.3` 是后者：稳定的 `build_runner 2.16` 需要 analyzer 13，而最新稳定
  Freezed 3 只接受更早的 analyzer；待两者稳定约束重新相交后应回到稳定 Freezed。
  升级时同步更新 Flutter SDK pin、重新解析 lockfile、运行全部生成器，并迁移上游 breaking API 后
  再提交源码与生成输出。
- Riverpod、Freezed、Flutter l10n 和 FRB 输出只由 `cargo xtask generate-gui` 管理，
  不得手工修改。生成流程必须覆盖依赖解析、所有生成器、生成文件规范化和仅作用于生成输出的格式化，
  使本地手工改动由生成器恢复为 canonical 内容。生成流程中的依赖解析与 build_runner 都必须
  在返回前恢复已跟踪 lockfile 的 canonical hosted URL，并拒绝生成器改变依赖解析。当前锁定的 build_runner 会始终删除冲突
  输出，不得继续传递已移除的 `--delete-conflicting-outputs` 参数。
- Git 索引中的文本统一规范化为 LF；普通源码和 xtask 生成的 canonical 文本默认也使用 LF。
  `.gitattributes` 必须把普通文本 checkout 固定为 LF，并只为必须采用 Windows 原生行尾的
  PowerShell、RC 和 Inno Setup 文件声明 CRLF 例外；`.editorconfig` 必须表达相同例外，不能让
  编辑器与 Git 的 checkout 契约冲突。生成流程还必须在格式化前将生成的 Dart 文本从 CRLF
  规范化为 LF，避免平台相关的纯换行差异污染工作区。
- `cargo xtask run-gui` 和普通 `cargo xtask build-gui` 只运行或构建当前源码，不执行生成器，
  不格式化、改写或清理任何已跟踪源码。修改 Riverpod、Freezed、Flutter l10n 或 FRB 输入后，
  开发者必须显式运行 `cargo xtask generate-gui`。需要验证生成一致性时使用
  `cargo xtask check-gui-generated`；该命令先快照当前生成输出，再重新生成并检查前后内容一致。
  检查只验证生成结果是否稳定，不要求生成输出已提交，也不以 `HEAD` 或 Git 暂存区作为基线。
- CI 或发布流程直接构建 GUI 时使用 `cargo xtask build-gui --check-generated`，在构建前显式
  重新生成并拒绝前后不一致的生成输出；普通本地构建不承担该检查，也不得产生源码工作区噪声。
- `cargo xtask verify-gui` 必须复用 `check-gui-generated`，PR、默认分支和正式发布 CI
  只调用 xtask 入口，不在 workflow 中复制生成器命令。重新生成改变快照内容时检查必须失败，
  并提示先运行生成入口并审查结果，而不是指导开发者手工修补生成文件或强制提交。
- CI 质量门禁（PR Quality Gate）只运行确定性检查：Rust fmt/clippy/test、
  `cargo xtask verify-gui`、Conventional PR 标题和发布配置校验。Flutter Driver
  smoke、任务流 harness 与 live 模型验收不在 CI 中运行，交付前在本地 Windows/Linux
  环境执行；Linux headless 环境通过 xtask 自动选择 Xvfb。AGENTS.md 的提交前检查清单与 CI 门禁保持同构，本地通过即代表 CI
  可通过（已跟踪的 pubspec.lock hosted URL 由 xtask 自动规范化，无需手工处理镜像差异）。
- xtask 中的生成输出规则是重生成稳定性检查和生成文件规范化的共同事实来源。新增生成器或
  输出目录时必须扩展该规则及其测试，不能只修改 CI pathspec 或单个格式化分支。全仓
  `cargo fmt` 和 `dart format` 只作为只读门禁运行；可写格式化只能作用于明确的生成输出，
  禁止重新写入无关手写源码。
- xtask 启动驻留 GUI、Driver 或 live fixture 子进程时必须把绝对 deadline 传到统一的进程 supervisor；
  headless 与 GUI 验收具有相同的总超时、日志保留和进程树回收语义。fixture 复制、diff 与 artifact
  遍历使用不跟随符号链接的目录项类型，并对任何 symlink fail-loud，不能读取 workspace 根之外的内容。

## 9.10 测试分层与放置

- 测试只保护重要且稳定的行为节点：公共协议与 wire 形状、领域状态机迁移、持久化事务与
  CAS、并发竞态、资源生命周期、错误映射、回滚、恢复与安全边界。不为薄转发、简单 getter、构造器、
  私有 helper 的具体实现、视觉尺寸或同一规则的重复排列保留独立测试。
- 单元测试验证单个模块的私有规则，以 `#[cfg(test)] mod tests` 作为对应生产源文件的最后一个
  item。不在 `src/**/tests/` 或通用大型 `tests.rs` 中集中堆放跨职责单元测试；生产模块过长时先按
  责任拆分，再将测试放到各自源文件末尾。
- crate 根 `tests/` 只放集成测试，并且只能像外部使用者一样通过 crate 根暴露的 `pub` API 验证
  跨模块合同。集成测试不得访问 `pub(crate)`、私有模块、数据库 entity/raw SQL 或测试专用后门，
  也不得为了测试扩大生产 API。
- 同一重要节点只保留最接近事实所有者的单元测试，以及有必要的一个公共 API 集成契约；不在 provider、
  runtime、adapter 和 UI 每层重复相同断言。MCP、LSP 和其他公共协议的版本协商、兼容回退与 wire
  round-trip 不属于可删的历史兼容测试。
- bug 修复只保留能在故障实现上稳定失败、并在修复后通过的确定性回归。测试 helper 必须位于对应
  单元测试模块或 crate 根 `tests/` 的测试辅助模块中。

## 9.7 配置约定

- 配置文件固定为 `~/.pure/config.toml`。
- 本地 TOML 使用 `snake_case`。
- 不设置 `active_provider`。
- 固定角色 key：`explorer`、`planner`、`executor`、`worktree_executor`、`reviewer`。
- 普通对话默认使用 `planner`。
- provider 必须持久化完整 models 列表，以支持用户自定义模型。
