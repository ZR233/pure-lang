# 02 - Crate 边界

## 依赖方向

```text
pl-studio-bridge / pl-studio-server → pl-studio-runtime
pl-studio-runtime → pl-core / pl-model / pl-tool / pl-protocol / pl-trace
pl-model → pl-core
pl-tool → pl-core
pl-trace → pl-core
```

model 与 tool 不相互依赖，core 不反向依赖实现或产品协议，包括 dev-dependencies。
完整成员以根 Cargo.toml 为准；Flutter `code/anywork` 不是 Cargo 成员。

## 稳定职责

| crate | 所有权 |
| --- | --- |
| pl-core | context/model/tool/thread/storage 通用契约、内存 owner、不可变日志和可选 SQLite |
| pl-model | provider/transport、模型目录、配置值对象、路由、媒体编码、缓存、usage 与核心 ModelSession 适配 |
| pl-tool | 文件、命令、SSH、Git、LSP、MCP、Skill、搜索、交互、笔记、todo、完成和协作工具 |
| pl-protocol | Studio 与外围 adapter 使用的产品 wire、DTO、错误和业务状态类型 |
| pl-trace | core 日志的只读诊断与用量投影 |
| pl-lsp | 语言服务连接、探测和协议 |
| pl-output / pl-patch / pl-skill-core | 输出算法、patch 规则、Skill 元数据与路径规则 |
| pl-remote-helper | 本地进程监督、SSH 远端文件/进程协议及统一进程创建策略 |
| pl-studio-runtime | 配置文件、项目、Profile、Mode/workflow/Plan、协调、资源租约与唯一 Thread 装配 |
| pl-studio-bridge / pl-studio-server | 同一 Studio runtime 的 FRB / HTTP 适配 |

core 不提供默认工具安装、provider 配置、MCP 目录、产品 working set 或旧引擎门面。
工具通过 `pl_core::tool::opaque::Registration` 转移实例所有权；模型通过核心 ModelSession 契约传入。
共用物理服务通过明确租约共享，不共享可变 Thread 工具实例。

## 存储与投影

Thread 内存是活动状态唯一事实源，SQLite 是显式 `sqlite` feature；默认纯内存。
通用 SessionEntry 和纯重放始终可用，存储不按业务类型分表或执行 decoder。
Studio 使用通用扩展 CAS 保存业务状态并提交其模型上下文投影；GUI 只消费 canonical 产品 DTO。
Provider 配置从 model、工具配置从 tool、业务协议从 protocol 导入，不借 core 镜像导出。

契约详见 [25](./25-session-entry-storage.md)、[27](./27-core-boundaries-and-replay.md)
与 [28](./28-tool-thread-boundary.md)。文档定义边界；验收结果必须以实际检查记录为准。
