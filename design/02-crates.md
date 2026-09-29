# 02 - Crate 边界

本文是 crate 划分与依赖方向的唯一权威源；其他文档只链接本文，不重复定义。

## 2.1 依赖方向

```text
pl-studio-bridge / pl-studio-server → pl-studio-runtime
pl-studio-runtime → pl-core / pl-model / pl-tool / pl-protocol / pl-trace
pl-model → pl-core
pl-tool → pl-core / pl-model
pl-trace → pl-core
```

model 不依赖 tool；tool 消费 model 已解释的 provider 能力与路由。core 不依赖 pl-protocol、pl-model、pl-tool、pl-trace、pl-output
或 Studio，包括测试依赖。新增概念先确认是否属于通用 Thread 编排；provider、工具和产品
专属概念分别归对应适配层，不借 core 门面镜像。完整成员清单以根
Cargo.toml 为准；Flutter 应用目录本身不是 workspace 成员，其 Rust 桥接 crate 单独纳入成员。

## 2.2 稳定职责

库 crate 统一使用 `pl-` 前缀；Flutter 应用的 package 名是 `anywork`。

| crate | 所有权 |
| --- | --- |
| pl-core | context/model/tool/thread 通用契约、内存 owner、当前状态 checkpoint、不可变 effect，以及产品无关的 Session/ChatView 有界时间线协调 |
| pl-model | provider/transport、模型目录、配置值对象、路由、媒体编码、缓存、usage 与核心模型会话适配 |
| pl-tool | 文件、命令、SSH、Git、LSP、MCP、Skill、搜索、交互、笔记、todo、完成和协作工具 |
| pl-protocol | Studio 与外围 adapter 使用的产品 wire、DTO、错误和业务状态类型 |
| pl-trace | core effect 与调用记录的只读诊断和用量投影 |
| pl-lsp | 语言服务连接、探测和协议 |
| pl-output / pl-patch / pl-skill-core | 输出算法、patch 规则、Skill 元数据与路径规则 |
| pl-remote-helper | 本地进程监督、SSH 远端文件/进程协议及统一进程创建策略 |
| pl-studio-runtime | TOML/SQLite 持久化及 core 时间线存储适配、产品条目投影、项目、Profile、Mode/workflow/Plan、子代理协调、资源租约与 root/child/恢复的唯一 Thread 装配 |
| pl-studio-bridge / pl-studio-server | 同一 Studio 运行时的 FRB / HTTP 适配 |
| pl-xtask | 轻量工程命令编排；按需通过子进程调用独立验收工具，不链接 Studio 运行时、模型或数据库 |
| pl-studio-acceptance | 隔离 GUI 验收场景、canonical 配置构造、模拟供应商协调、数据库故障观察与证据采集 |
| pl-dev-support | 工程工具共享的仓库路径、命令执行及平台进程树所有权，不依赖 Studio、模型、数据库或模拟供应商 |

工程工具的依赖方向为 `pl-xtask → pl-dev-support`、
`pl-studio-acceptance → pl-dev-support / pl-studio-runtime / pl-model / pl-provider-fixture`。
xtask 对验收工具只有命令调用关系，不建立 Cargo 依赖；验收工具也不反向依赖 xtask。
验收所需数据库依赖归验收工具。普通工程命令不因验收能力而编译 Studio 运行时；GUI 构建
显式构建 bridge、验收显式构建验收工具以及全 workspace 检查仍正常编译各自所需组件。

core 不提供默认工具安装、provider 配置、MCP 目录、产品 working set 或旧引擎门面。工具实例通过
不透明注册句柄向 Thread 转移所有权（见 [09](./09-tool-runtime.md)）；模型通过核心模型会话契约
传入。共用物理服务通过明确租约共享，不共享可变 Thread 工具实例。

## 2.3 存储与投影

Thread 内存是活动执行状态唯一事实源；core 默认纯内存，导出当前 checkpoint、不可变 effect，
并负责有界聊天窗口对最近缓存、活动条目、待保存事实和历史查询的统一合并。core 不依赖产品协议；
core 定义按身份与顺序查询的存储能力，并提供通用 SQLite session store 与 typed Thread checkpoint/effect 查询。
Studio 负责产品条目投影、产品配置和自己的持久化状态。历史分页由 ChatView 协调，Studio 不能另建 GUI 专用历史缓存或消息
交接链；冷历史浏览不激活模型、工具或执行 owner。可丢统计由全局 calls 库独立写入。Studio 使用
通用扩展 CAS 保存业务状态并提交其模型上下文投影；GUI 只消费 canonical 产品 DTO。Provider 配置
从 model、工具配置从 tool、业务协议从 protocol 导入，不借 core 镜像导出。

契约详见 [15](./15-session-storage.md) 与 [16](./16-core-contracts.md)。文档定义边界；验收结果
以实际检查记录为准。
