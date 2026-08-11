# Changelog

Pure Studio release notes are generated from Conventional Commits by Release Please.

> 从采用 `pure_studio.exe` 的版本开始，请先手动卸载旧版 Pure Studio，再安装新版。
> 安装器允许直接覆盖，但不会检测或删除旧程序文件；跳过卸载可能留下旧文件。

## [2.0.0](https://github.com/ZR233/pure-lang/compare/v1.0.3...v2.0.0) (2026-08-11)


### ⚠ BREAKING CHANGES

* **studio:** Studio schema 升级到 v3，旧 schema 启动时按既定策略重建数据库。
* **agent-runtime:** 增量化 wait_agents 返回协议
* **studio:** Studio 会话协议和持久层统一为 Thread、Turn、Item，旧数据库仅归档不导入。
* **studio:** Studio v10 数据库将被归档并重建为 studio_state.sqlite v11 与 studio_history.sqlite v1，不导入旧数据。
* **studio:** Studio SQLite v1-v9 改为归档后重建 v10，并移除旧协作工具、wake/continuation/delivery recovery、migration 与 raw JSON 兼容路径。
* 移除旧审批策略、旧命令别名、legacy Studio JSON/FRB 字段及 --demo-fallback。
* **studio:** session event wire 升级为 typed turn state、双通道 reasoning 与 typed mailbox presentation；旧客户端不再兼容运行期 wire。

### Features

* **agent-runtime:** 增量化 wait_agents 返回协议 ([9cf987d](https://github.com/ZR233/pure-lang/commit/9cf987d53c48761a3285bd6c7e3c4c692461465d))
* **agent-runtime:** 支持工具发现、程序化调用和编排指标 ([e32f26b](https://github.com/ZR233/pure-lang/commit/e32f26b137483f2e98041ae9d5a2e13efcffd85f))
* **studio:** add managed Task executors and live model test ([79661cf](https://github.com/ZR233/pure-lang/commit/79661cfc853a55b50402bafc51f6089e7a813369))
* **studio:** refine reasoning activity timeline ([8cbcf06](https://github.com/ZR233/pure-lang/commit/8cbcf06fa7b6e2f420924e8bb0bddb8d039f53bb))
* **studio:** refine tool activity display ([5fc8d25](https://github.com/ZR233/pure-lang/commit/5fc8d25303f3e8acd38b3c7160f42df0668d8fe3))
* **studio:** unify Pure Studio identity and icon ([1cceb50](https://github.com/ZR233/pure-lang/commit/1cceb50d3b3309a3a9741e7ad7036d648a39f8d1))
* **studio:** 升级 SeaORM 2.0 并重构双库会话持久化 ([5410572](https://github.com/ZR233/pure-lang/commit/5410572cafd1e7605110c4cec577fdf214dee968))
* **studio:** 固定角色显示名称本地化 ([#48](https://github.com/ZR233/pure-lang/issues/48)) ([ed8c286](https://github.com/ZR233/pure-lang/commit/ed8c28605905ea58b1e556ced72e38a118184fee))
* **studio:** 技能页切换时自动发现项目技能并替换缓存 ([#47](https://github.com/ZR233/pure-lang/issues/47)) ([53519f4](https://github.com/ZR233/pure-lang/commit/53519f4a4c95d43fd87661007ab75464d7ead997))
* **studio:** 支持 Task 会话回退与续跑验收 ([e8826b1](https://github.com/ZR233/pure-lang/commit/e8826b1282b57907bdd3da4a6399dac8e6d15f30))
* **studio:** 统一 Agent Workspace 与 Planner 自主合并 ([312fd00](https://github.com/ZR233/pure-lang/commit/312fd00c5cd9e22204519120e4b8359f31ce10e8))
* **studio:** 重构上下文持久化与 executor 续轮 ([b1cf092](https://github.com/ZR233/pure-lang/commit/b1cf092be0c4dd9f3baaabb815aa86c100071a7d))


### Bug Fixes

* **agent-runtime:** 防止等待唤醒误停执行者 ([80dcd3b](https://github.com/ZR233/pure-lang/commit/80dcd3ba57422ad7f3d23b3d390dfb8f04b15861))
* **ci:** 修复 Rust lint 与 Dart 格式检查 ([1dffa99](https://github.com/ZR233/pure-lang/commit/1dffa99c275d2b6ae26d3efa9e75fd1ac6f584ad))
* **patch:** tolerate stale ARB context values ([ec838f4](https://github.com/ZR233/pure-lang/commit/ec838f4172ab6644ec0568f61cfd10e97f712e56))
* **path:** keep platform-specific requirements lint-clean ([c34cb47](https://github.com/ZR233/pure-lang/commit/c34cb47ac1d274dcf83174572d70bd591fc180cf))
* preserve Flutter lock across hosted mirrors ([e4b2ce9](https://github.com/ZR233/pure-lang/commit/e4b2ce917ab659784e91a71a40fa769749dd7f9b))
* rebuild incompatible Studio databases ([e768d1f](https://github.com/ZR233/pure-lang/commit/e768d1fe335189a4f144f4730e355df38c08859e))
* **studio:** isolate agent sessions and recover task delivery ([ae325ad](https://github.com/ZR233/pure-lang/commit/ae325aded1eff18a4f66a513a38160020ab7412a))
* **studio:** isolate recovery failures and add safe cleanup ([5f382e4](https://github.com/ZR233/pure-lang/commit/5f382e484d03d6be48a920e8aabb395729e9bd2d))
* **studio:** preserve delivery across interrupted turns ([e9772a2](https://github.com/ZR233/pure-lang/commit/e9772a2e1a53cbe9b7fb81c37c774a3b07c1a594))
* **studio:** 修复 GUI 重复构建缓存失效 ([1a98f9e](https://github.com/ZR233/pure-lang/commit/1a98f9e0444cb047f2fd3bafdd3f269b9c466826))
* **studio:** 修复 Responses 工具回放与预算中止语义 ([7935793](https://github.com/ZR233/pure-lang/commit/7935793119313b0a631153cfa173660c880b1327)), closes [#39](https://github.com/ZR233/pure-lang/issues/39)
* **studio:** 修复会话模式切换与快照同步 ([2f50a2f](https://github.com/ZR233/pure-lang/commit/2f50a2f8f355f788587a6e7951f7f1bbf10898ab))
* **studio:** 允许缺失 Windows native assets ([dc24860](https://github.com/ZR233/pure-lang/commit/dc248606c36b88a14a197621ef6eba5c9551121c))
* **studio:** 分层处理任务失败并回传验证错误 ([72ac3d8](https://github.com/ZR233/pure-lang/commit/72ac3d802f57abbea3c684509e1b21479e63f102))
* **studio:** 复用 workspace Rust bridge 产物 ([c1c4a17](https://github.com/ZR233/pure-lang/commit/c1c4a17b06fd947b4bc6997a8fdeea49e73deeb3))
* **studio:** 安全清理失败项目并防止 Planner 误打断 ([5e3a87e](https://github.com/ZR233/pure-lang/commit/5e3a87e30ed9e2219041213288cbeaada72beae2))
* **studio:** 展示流式 reasoning 内容 ([#30](https://github.com/ZR233/pure-lang/issues/30)) ([7acd2a1](https://github.com/ZR233/pure-lang/commit/7acd2a1c6d15c06b9aef87acc0b6ca1cebe93156))
* **studio:** 恢复新会话与会话归档功能 ([443acdf](https://github.com/ZR233/pure-lang/commit/443acdfe7d3164876f3fbcd4d9d25b197e619115))
* **studio:** 按模型配置 API 协议与传输 ([ed7891d](https://github.com/ZR233/pure-lang/commit/ed7891d108215baef462cb29430784ced8e22259))
* **studio:** 提交计划调整后继续规划 ([6f87a83](https://github.com/ZR233/pure-lang/commit/6f87a832825ec22ceba2dacc082e8a80cd92ebef))
* **studio:** 提升 DeepSeek 缓存命中并持久化计费 ([125a10d](https://github.com/ZR233/pure-lang/commit/125a10d6421f640abb32a311879a85133b2f252b))
* **studio:** 用户交互后从持久化新 Turn 继续 ([bfe9afa](https://github.com/ZR233/pure-lang/commit/bfe9afad26ecea9bca2b21a2be0745c15d099014))
* **studio:** 移除任务完成阶段的项目验证 ([8fea360](https://github.com/ZR233/pure-lang/commit/8fea360a06881ea87e746bd35313251162fce86b))
* **studio:** 稳定 Task 续轮、交接与工具重试 ([b7d39a6](https://github.com/ZR233/pure-lang/commit/b7d39a655228f42614f83d0b3181a8263cb84266))
* **studio:** 避免异步提交在销毁后访问状态 ([f3e7aeb](https://github.com/ZR233/pure-lang/commit/f3e7aeb463b36f64fb3ad2207c18206fd34a8791))
* **studio:** 隔离不可用项目并恢复空项目状态 ([09e19a9](https://github.com/ZR233/pure-lang/commit/09e19a93d0731a57036d08d33828aac6e992ef6f))
* **tool:** expose output artifact receipts to models ([7bb6999](https://github.com/ZR233/pure-lang/commit/7bb6999c8215b62f6e2071df77e3cd2fe0f0363f))


### Performance

* **agent-runtime:** 并行安全工具并限制批次结果预算 ([d760eb7](https://github.com/ZR233/pure-lang/commit/d760eb7ec3ded6f913d1fa35ed1aaa3d4d726162))
* **studio:** 优化提示词缓存与计费可观测性 ([#45](https://github.com/ZR233/pure-lang/issues/45)) ([d70ec43](https://github.com/ZR233/pure-lang/commit/d70ec435d1de900b20b798605ccd6a8d506353e4))


### Refactoring

* **agent-runtime:** use subscription-based planner wakeups ([1aa0589](https://github.com/ZR233/pure-lang/commit/1aa0589dd268ddccc19db972a9bc3006b4e6aeef))
* **agent-runtime:** use subscription-based planner wakeups ([b4e4c9c](https://github.com/ZR233/pure-lang/commit/b4e4c9c0119085eecd0874bcbb64fe5b1f3ac9a3))
* **studio:** 精简 Turn 执行器与任务审查闭环 ([3d03ec9](https://github.com/ZR233/pure-lang/commit/3d03ec9a54154c6efea416dc37608b2749576daa))
* **studio:** 统一 Thread Turn Item 会话架构 ([9743170](https://github.com/ZR233/pure-lang/commit/97431709d98ccd03a17be8945d049803f3c516e0))
* **studio:** 重构 FRB Riverpod 与 Driver 边界 ([534fd78](https://github.com/ZR233/pure-lang/commit/534fd780a75aa90a39ae255c1606d14bb60af7d7))
* **studio:** 重构 Timeline 当前 Turn 状态机 ([#34](https://github.com/ZR233/pure-lang/issues/34)) ([900841f](https://github.com/ZR233/pure-lang/commit/900841f3eefe3fcbeb9d841fbcf7aabad757a2df))
* 清理旧兼容层并收敛现役架构 ([fa4b6e0](https://github.com/ZR233/pure-lang/commit/fa4b6e0dd1f5a4d6409701f845fabb16b6924739))


### Documentation

* **release:** 规范提交与 PR 标题 ([#29](https://github.com/ZR233/pure-lang/issues/29)) ([12a64a6](https://github.com/ZR233/pure-lang/commit/12a64a68f5608f0469b2b9b2db2cda6ed069e929))

## [1.0.3](https://github.com/ZR233/pure-lang/compare/v1.0.2...v1.0.3) (2026-07-23)


### Bug Fixes

* **model:** 修复 Responses WebSocket 地址族超时 ([#22](https://github.com/ZR233/pure-lang/issues/22)) ([73e5714](https://github.com/ZR233/pure-lang/commit/73e5714cac3436375d96d1147d673eeee3d38e1d))

## [1.0.2](https://github.com/ZR233/pure-lang/compare/v1.0.1...v1.0.2) (2026-07-23)


### Bug Fixes

* **filesystem:** unify linked path safety ([#20](https://github.com/ZR233/pure-lang/issues/20)) ([7b98543](https://github.com/ZR233/pure-lang/commit/7b98543de0b5c40925fda75898820ef4d64104e1))
* **studio:** skip linked paths in file tools ([#18](https://github.com/ZR233/pure-lang/issues/18)) ([0e33b7a](https://github.com/ZR233/pure-lang/commit/0e33b7af7b1c0cbd35e521ea83a3d5b5932ef842))

## 1.0.1 (2026-07-22)

- First signed Windows x64 release with in-app update support.
