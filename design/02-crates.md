# 02 - Crate 边界

本文是 crate 划分与依赖方向的唯一权威源；其他文档只链接本文，不重复定义。

## 2.1 依赖方向

```text
pl-studio-bridge / pl-studio-server → pl-studio-runtime
pl-studio-runtime → pl-core / pl-model / pl-tool / pl-protocol / pl-trace
pl-model → pl-core
pl-tool → pl-core
pl-trace → pl-core
```

model 与 tool 不相互依赖；core 不反向依赖实现或产品协议，包括测试依赖。完整成员清单以根
Cargo.toml 为准；Flutter 应用目录本身不是 workspace 成员，其 Rust 桥接 crate 单独纳入成员。

## 2.2 稳定职责

库 crate 统一使用 `pl-` 前缀；Flutter 应用的 package 名是 `anywork`。

| crate | 所有权 |
| --- | --- |
| pl-core | context/model/tool/thread/storage 通用契约、内存 owner、不可变日志和可选 SQLite |
| pl-model | provider/transport、模型目录、配置值对象、路由、媒体编码、缓存、usage 与核心模型会话适配 |
| pl-tool | 文件、命令、SSH、Git、LSP、MCP、Skill、搜索、交互、笔记、todo、完成和协作工具 |
| pl-protocol | Studio 与外围 adapter 使用的产品 wire、DTO、错误和业务状态类型 |
| pl-trace | core 日志的只读诊断与用量投影 |
| pl-lsp | 语言服务连接、探测和协议 |
| pl-output / pl-patch / pl-skill-core | 输出算法、patch 规则、Skill 元数据与路径规则 |
| pl-remote-helper | 本地进程监督、SSH 远端文件/进程协议及统一进程创建策略 |
| pl-studio-runtime | 配置文件、项目、Profile、Mode/workflow/Plan、协调、资源租约与唯一 Thread 装配 |
| pl-studio-bridge / pl-studio-server | 同一 Studio 运行时的 FRB / HTTP 适配 |

core 不提供默认工具安装、provider 配置、MCP 目录、产品 working set 或旧引擎门面。工具实例通过
不透明注册句柄向 Thread 转移所有权（见 [09](./09-tool-runtime.md)）；模型通过核心模型会话契约
传入。共用物理服务通过明确租约共享，不共享可变 Thread 工具实例。

## 2.3 存储与投影

Thread 内存是活动状态唯一事实源；SQLite 是 core 的可选能力，默认纯内存。通用会话条目与纯重放
始终可用，存储不按业务类型分表或执行 decoder。Studio 使用通用扩展 CAS 保存业务状态并提交其模型
上下文投影；GUI 只消费 canonical 产品 DTO。Provider 配置从 model、工具配置从 tool、业务协议从
protocol 导入，不借 core 镜像导出。

契约详见 [15](./15-session-storage.md) 与 [16](./16-core-contracts.md)。文档定义边界；验收结果
以实际检查记录为准。
