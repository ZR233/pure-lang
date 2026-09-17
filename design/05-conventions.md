# 05 - 通用约定

本文收录跨 crate 的设计与编码约定。协作流程、命令入口、异步接口写法、生成文件与测试门禁由
AGENTS.md 与相应技能统一规定，本文不重复；事实源归属如下。

| 主题 | 事实源 |
| --- | --- |
| crate 划分与依赖方向 | [02](./02-crates.md) |
| 异步 trait 写法、生成文件、测试与 CI 门禁 | AGENTS.md |
| 测试设计方法 | test-quality 技能 |
| 权限与安全 | [04](./04-security.md) |
| 配置 schema | [20](./20-config.md) |

## 5.1 参数与类型设计

- 核心接口不暴露语义模糊的布尔参数；前端输入在 anywork 边界转换为明确类型。
- 工具 schema 必须完整描述影响参数有效性的约束。分页 cursor 只能与生成它的请求投影配套使用；
  续页必须保留 cursor 所绑定的过滤、路径与匹配参数，工作区变更后旧 cursor 失效。
- Codex patch 的 Update hunk 每行首字符是控制前缀：空格表示上下文、`-` 表示删除、`+` 表示新增；
  内容本身以 `-` 或 `+` 开头时，该字符必须放在控制前缀之后（例如删除行 `- old` 写作
  `-- old`，新增行 `- new` 写作 `+- new`）。

## 5.2 模块与导出

- 模块默认私有；稳定领域边界用可读命名空间组织，只有少量高频入口才精确重导出。同一公开接口只
  保留一条 canonical 路径，不保留根与子模块双轨导出；内部底层边界不得被通配暴露。
- 单一职责模块保持单文件；拥有多个真实、内聚子职责时才拆分子模块；禁止只做转发的目录层。
- core 只导出自身通用契约，不镜像 provider、工具或产品 wire；model、tool 与 Studio 在公共签名
  需要时精确重导出依赖类型，不建立整套 API 转发门面。

## 5.3 生命周期状态机模式

具有时间顺序、非法转换、终态或恢复语义的领域对象统一使用单一状态机聚合：身份与跨状态上下文位于
aggregate，唯一可写状态是带 payload 的 tagged enum，每个状态承载字段私有的独立数据。只适用于
某个状态的时间、失败、进度与结果不得提升为 aggregate 上的平行可选字段、布尔值或第二个 status
enum。

状态变化只接受语义明确的 command，并返回包含下一状态、durable effects 与 external effects 的
decision；状态模块是纯领域代码，不执行 IO、等待、加锁或外部回调，adapter 在事务与生命周期边界
解释 effects。禁止通用 set-state、can-transition 查询、from-parts 兼容构造与动态状态分发；终态
不可继续迁移，恢复必须是从指定可恢复状态出发的显式 command。重复 operation、mail 或 revision
只有完全命中幂等身份时才是 no-op；其他同态命令与过期 revision 必须返回类型化转换错误。

公共协议的生命周期统一使用 tagged enum（kind/data 判别，camelCase）；Dart 使用 sealed union 并
穷尽匹配。SQLite 对需要查询的生命周期保存完整状态 JSON，判别列只能从 JSON discriminator 生成。
普通分类、配置、能力、scope、transport、severity、解析游标与没有迁移规则的一次性结果继续使用
普通 enum，不为形式统一强行引入状态机。

## 5.4 命名口径

- 项目名：Pure-Lang；桌面应用：anywork（糊来帮）。
- 库 crate 统一 `pl-` 前缀；Flutter package 名为 `anywork`。
- 产品 wire 放入 pl-protocol；core 通用契约在自身领域模块定义。

## 5.5 后台进程与外部服务

- GUI 运行时派生 shell、git、MCP server、LSP 等后台子进程时，Windows 必须不弹出新的命令行窗口
  （CREATE_NO_WINDOW），Job Object 路径在真正创建进程前合并该标志；Unix 使用独立进程组便于整树
  回收。进程配置策略由 pl-remote-helper 的统一工厂提供，原生路径与 Job Object 路径都必须从该
  工厂取得等价策略，其他 crate（包括 pl-lsp）不得复制实现。
- stdio MCP 配置保存跨平台命令名，不写平台后缀，也不统一套 shell；Windows 上按 PATHEXT 解析
  可执行目标并保持直启语义，标准 npm/npx launcher 必须直接展开为 node + CLI，不得让长期运行的
  MCP 连接由 shell shim 持有；npm launcher 创建子进程时显式隐藏窗口。stdin/stdout/stderr 全部
  管道化并消费，禁止继承 Studio 终端；stderr 只允许以有界、脱敏形式补充启动错误。
- MCP 客户端优先使用 `server/discover` 协商已知协议版本；仅当对端明确返回 METHOD_NOT_FOUND、
  证明是传统协议服务时，回退标准 `initialize` 协商。Streamable HTTP transport 若把 discovery
  bootstrap 的终止 SSE 折叠为关闭的 discover response，必须用全新 transport 重试标准
  initialize；该兼容重试不得扩展到认证、超时或其他协议错误，也不得为单一 provider 写专用版本
  特判。
- 启动路径的慢能力（MCP 探测、LSP probe）一律后台异步执行，结果经产品事件流推送，不阻塞主界面
  骨架。单个 MCP 启动或探测失败归属到该 server 的运行时 health，投影为 unavailable 与有界、
  脱敏的错误消息；配置启用态与运行时可用态不得混用，单个失败也不得阻塞 Studio shell。
