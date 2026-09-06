# Changelog

Pure Studio release notes are generated from Conventional Commits by Release Please.

> 从采用 `pure_studio.exe` 的版本开始，请先手动卸载旧版 Pure Studio，再安装新版。
> 安装器允许直接覆盖，但不会检测或删除旧程序文件；跳过卸载可能留下旧文件。

## [3.0.0](https://github.com/ZR233/pure-lang/compare/v2.0.0...v3.0.0) (2026-09-06)


### ⚠ BREAKING CHANGES

* **model:** 替换模型协议绑定、定价和计量 API；Studio 配置仅接受 schema 18，旧格式按仓库约定备份后重建，不保留旧 API 或迁移兼容层。
* **thread-mode:** 删除 Mode Skill、旧 workflow_state/submit_plan、旧 Mode 类型及兼容路径。
* **pl-model:** 两个校验方法与 WS 通道的错误类型由 String 改为类型化错误。
* **pl-model:** pl_model::X 扁平路径迁移为 pl_model::<domain>::X； pl_core 不再转发 pl_model 与 pl-protocol 的模型类型。
* **pl-lsp:** 重组模块与公开命名空间
* **pl-core:** 删除旧 Tool、TypedTool、LocalTool 及兼容注册入口，统一使用 StaticTool、DynTool、ToolExecutor 和 ToolInstallGroup。
* **agent-runtime:** expose typed turn budgets
* **studio:** Studio 本地数据库升级为 v17，并破坏性重建旧项目、Thread、附件和 Task 历史。
* **agent-runtime:** thread schema 升级至 9，并以 typed tool attachments 和 ToolMedia 替换旧工具媒体表达。
* **studio:** 移除旧消息内容、附件命令和模型能力兼容结构，统一使用 typed parts、attachment draft 与 request profile。
* **studio:** Thread wire 升级为 v7，plan_exit 仅接受严格的 plan 字段，旧 Skill payload 不再兼容恢复。
* **skills:** 原生承载 Turn Skill 指令
* **skills:** 开放宿主原生 Skill 目录注册
* **agent-runtime:** 统一 Agent 协议状态事实源
* **tool:** 删除旧工具注册、可见性与旁路执行 API，所有工具统一通过 ToolManager、AgentToolSet 和冻结 ToolPlan 注册与调用。

### Features

* **agent-runtime:** expose typed turn budgets ([ba2b345](https://github.com/ZR233/pure-lang/commit/ba2b345bc0bba25d62f7fa5a694d865b2486f4a1))
* **agent-runtime:** 优化子代理预算暂停与会话查询 ([4139317](https://github.com/ZR233/pure-lang/commit/4139317358b2e41689fd19412e323cad8294bdf3))
* **agent-runtime:** 保留产品生命周期所需语义 ([dbcba70](https://github.com/ZR233/pure-lang/commit/dbcba70eec7102ca3fbf7c7e0b12a6be6142d437))
* **agent-runtime:** 支持代理主动读取图片 ([7f057b4](https://github.com/ZR233/pure-lang/commit/7f057b4808ed9563579ef0990f32387b97c9b8f6))
* **gui:** 增加环境预检与 Web 远程验收 ([54acafa](https://github.com/ZR233/pure-lang/commit/54acafa449ad00bbc5cbe366f68356d7d0daef48))
* **model:** 增加 DeepSeek 原生联网搜索 ([585150d](https://github.com/ZR233/pure-lang/commit/585150d10c41b4e539ff72de268479ad11b88557))
* **model:** 新增 GLM-5.3-Flash 多模态模型支持 ([83cfe18](https://github.com/ZR233/pure-lang/commit/83cfe183f50f3e9496ed5c38d6b2f2357141ac5a))
* **pl-core:** 注入动态执行环境与 shell 类型 ([767f454](https://github.com/ZR233/pure-lang/commit/767f454e78a8f78039231daf3b9b6a18bd49a942))
* **release:** 分发 SSH remote helper ([c39c1ec](https://github.com/ZR233/pure-lang/commit/c39c1ec245b853e761e136c89f0e043cb4b33e75))
* **remote:** 将 SSH helper 嵌入应用二进制 ([5d37f4d](https://github.com/ZR233/pure-lang/commit/5d37f4db6eea2e9a895bf61785426752e0986908))
* **runtime:** 统一流式提交合并语义 ([307801a](https://github.com/ZR233/pure-lang/commit/307801ac8a60ea374f3228d4d9525b99e1413858))
* **skills:** 优化名称与描述发现 ([0fc595b](https://github.com/ZR233/pure-lang/commit/0fc595be0f8d32222ea62b0b2b3f8b2db5b6980e))
* **skills:** 开放宿主原生 Skill 目录注册 ([4d26311](https://github.com/ZR233/pure-lang/commit/4d26311ed481a4ae095d9af28e944c96ff29e84e))
* **studio:** add configurable subagent workspaces ([839c0a3](https://github.com/ZR233/pure-lang/commit/839c0a36ecb861f2e79c73d8fbfcce83ec9d6e1b))
* **studio:** enforce subagent-first task prompts ([cf2be0e](https://github.com/ZR233/pure-lang/commit/cf2be0e3f2653f3c84e835cfce950ccf2521cf99))
* **studio:** 优化 Timeline 等待状态动效 ([9bdc1bc](https://github.com/ZR233/pure-lang/commit/9bdc1bc3c78294d4974ac562abc8252e37143979))
* **studio:** 优化任务模式动态并行调度 ([861a39e](https://github.com/ZR233/pure-lang/commit/861a39e9dd07982061325eab9b53f6f128505c5d))
* **studio:** 在子代理时间线显示主代理消息 ([3d06631](https://github.com/ZR233/pure-lang/commit/3d06631ff0be6f0e98016911669183d155a06a85))
* **studio:** 增加会话费用与模型性能统计 ([9473f15](https://github.com/ZR233/pure-lang/commit/9473f152f8c26be2b7b7f8450065d4741ff157ca))
* **studio:** 完善 Skill Provider 与 Linux Flutter 验收 ([caaaaa8](https://github.com/ZR233/pure-lang/commit/caaaaa87f5ae5dc83dc1233be464ab2bcfee8641))
* **studio:** 完善子代理隔离与任务编排 ([01ee7c3](https://github.com/ZR233/pure-lang/commit/01ee7c39226c883505f83e2f6281683c769753a4))
* **studio:** 完善预置 Skill 资源管理 ([8566177](https://github.com/ZR233/pure-lang/commit/8566177306ec256494449bc24e856eea683a1402))
* **studio:** 实现 Timeline 端到端真流式 ([c83c0d5](https://github.com/ZR233/pure-lang/commit/c83c0d54666c01c9610c15fe34f958d18ae4963c))
* **studio:** 展示 Skill 激活状态 ([d49686d](https://github.com/ZR233/pure-lang/commit/d49686d2427979f3abccb5608b4fdb9623a0774a))
* **studio:** 支持 SSH 远端路径直接输入 ([30a8ef7](https://github.com/ZR233/pure-lang/commit/30a8ef7a0f716af08a7ffff41e5e7a861157cc08))
* **studio:** 支持最小 SSH 远程开发 ([0f3aa23](https://github.com/ZR233/pure-lang/commit/0f3aa23fa6f6844f788342440fcea78611c64a84))
* **studio:** 支持对话 Web 超链接 ([6ccb07a](https://github.com/ZR233/pure-lang/commit/6ccb07ab0303e98c0f25efe752d1bfd0cfb51772))
* **studio:** 支持工具图片展示与安全预览 ([f22a37e](https://github.com/ZR233/pure-lang/commit/f22a37ee8a555c24b3c47e31297d79422fc87642))
* **studio:** 显示侧栏完整项目与会话名称 ([b3667fc](https://github.com/ZR233/pure-lang/commit/b3667fc95924537bb8850dbc6df05cf189b93d4f))
* **studio:** 自动生成并同步会话标题 ([58682b4](https://github.com/ZR233/pure-lang/commit/58682b4fc7e245d00aab20db1ee738e194e957e9))
* **studio:** 自由化简洁模式并统一完成工具 ([9082b58](https://github.com/ZR233/pure-lang/commit/9082b58ca17709219d0a08cfc618f1d0082c7cfe))
* **studio:** 重构多模态输入与模型自适应 ([8d323b3](https://github.com/ZR233/pure-lang/commit/8d323b3446bf18cae19a500f904774c703f84e7a))
* **studio:** 预置上游系统技能并提供 xtask 同步命令 ([5e6ddc0](https://github.com/ZR233/pure-lang/commit/5e6ddc09aaef6c163f706f80836de94011930cb4))
* **timeline:** integrate plan confirmation UI and functionality ([c1fdee9](https://github.com/ZR233/pure-lang/commit/c1fdee96f41b50fa5b72f826eafc01e657bb9dd1))


### Bug Fixes

* **agent-runtime:** initialize lazy restore thread events ([cc967ef](https://github.com/ZR233/pure-lang/commit/cc967ef382f263d2a82fd80e16c866a4aa52b077))
* **agent-runtime:** 修复工作流状态参数生成失败 ([c6987bb](https://github.com/ZR233/pure-lang/commit/c6987bb61ffdef2a3984f7c8cc9efae9371f7c8f))
* **agent-runtime:** 淘汰前等待 Thread 耐久化 ([89ef14f](https://github.com/ZR233/pure-lang/commit/89ef14f3c2b04c1245766e585fc4820293f656dd))
* **agent-runtime:** 隔离冷存储并复用任务返工上下文 ([430bb51](https://github.com/ZR233/pure-lang/commit/430bb51bdfd71eabb3b44f7d47a4946543547169))
* **build:** 避免清理被占用的发布根目录 ([c613026](https://github.com/ZR233/pure-lang/commit/c61302627648e9901f8c6808be382198a17462e4))
* **ci:** 修复 Windows 测试辅助函数告警 ([2a78096](https://github.com/ZR233/pure-lang/commit/2a78096bd34d55ae3c69749d1661c932dee70548))
* **ci:** 修复严格 clippy 检查 ([b53ebd0](https://github.com/ZR233/pure-lang/commit/b53ebd01add3897f3b13bbd7cb46e8a5c467346f))
* **ci:** 区分工作流脚本 shell ([3bbb1d4](https://github.com/ZR233/pure-lang/commit/3bbb1d4a4d6b1cd518cfeb93b32cb1350d849a18))
* **gui:** 修复 Windows Web 验收构建 ([361acab](https://github.com/ZR233/pure-lang/commit/361acab5eb4ec093f0e1bbf976791e3171a66055))
* **gui:** 修复 Windows 生成检查路径 ([0adb28c](https://github.com/ZR233/pure-lang/commit/0adb28ca7980b9a13fdb3f12e8dd9a8a37464239))
* **model:** partial WS 失败后熔断后续 Turn ([989864c](https://github.com/ZR233/pure-lang/commit/989864ca7aba15c00352d9516ed83e02aee2f31a))
* **model:** 保留 HTTP 流式供应商错误分类 ([05a6a57](https://github.com/ZR233/pure-lang/commit/05a6a57dbcd80757372ba34fb7446b0edb9000aa))
* **pl-core:** 修复 Windows 目录写边界判断 ([e9b8ac0](https://github.com/ZR233/pure-lang/commit/e9b8ac0bf2c723e913783fe06930b61b7920dc4c))
* **pl-core:** 统一目录边界的 canonical 根路径 ([59ff06c](https://github.com/ZR233/pure-lang/commit/59ff06ca73445a91253294ab351e10f301ca486b))
* **quality:** 清理重复逻辑并修复任务模式 GUI ([98beab0](https://github.com/ZR233/pure-lang/commit/98beab0728b75e203729b0843be4359d3f0fba5b))
* **remote-helper:** 修复 Windows Clippy 错误 ([d03237d](https://github.com/ZR233/pure-lang/commit/d03237d65f2c41f7e3ccda4df915a37d1b50df99))
* **remote-helper:** 修复 Windows 测试 Clippy 错误 ([91b9d71](https://github.com/ZR233/pure-lang/commit/91b9d71363ef40b357def59336124d26a984262e))
* **remote-helper:** 让协议测试脱离宿主 shell ([b4aa47f](https://github.com/ZR233/pure-lang/commit/b4aa47fe436ac8fd5c12c1290d313fc2c80ab5b9))
* **remote:** reuse loaded workspace instructions ([d984816](https://github.com/ZR233/pure-lang/commit/d9848163fc7d3855fd4bbd631edd426881ceda44))
* **remote:** 统一远端 POSIX 路径语义 ([fcbd177](https://github.com/ZR233/pure-lang/commit/fcbd177e471d9c06ad9bdc84b8beb5e6f4319f94))
* **remote:** 自动准备助手工具链并隐藏 SSH 终端 ([e438dad](https://github.com/ZR233/pure-lang/commit/e438dadf8abd2236793310653775cc49bb15c833))
* **skills:** preserve provider-owned trust roots ([3234f8a](https://github.com/ZR233/pure-lang/commit/3234f8a119fbc52e1c2742b4138d2ef859ccad89))
* **skills:** 修复标准目录发现与项目默认路径 ([8cf7f4f](https://github.com/ZR233/pure-lang/commit/8cf7f4f5d78543d68bf13e2e5055de6a9b14f11f))
* **skills:** 原生承载 Turn Skill 指令 ([df7018a](https://github.com/ZR233/pure-lang/commit/df7018a0fa3cdd3814e9855100b76893aba92561))
* **studio:** accept bound child session fallbacks ([38f6e38](https://github.com/ZR233/pure-lang/commit/38f6e3883e559580240e2c35224209a3d4f6204d))
* **studio:** accept bounded explorer step fences ([7c421d8](https://github.com/ZR233/pure-lang/commit/7c421d842f2ff60c32ecbeb7a86f230ab85e3675))
* **studio:** accept canonical agent route no-op ([aeeb579](https://github.com/ZR233/pure-lang/commit/aeeb579cec1f5ad9eb6c303787d1d904d58fdfc6))
* **studio:** adapt verifier to Rust 1.98 ([d0ddd7e](https://github.com/ZR233/pure-lang/commit/d0ddd7e6be77dee889835489922c25e5d2c7fcf8))
* **studio:** aggregate task wire evidence ([d8334f4](https://github.com/ZR233/pure-lang/commit/d8334f4cb3f35d096c279d3cccdc948edc2a6796))
* **studio:** allow reviewer progress before verdict ([88f95e1](https://github.com/ZR233/pure-lang/commit/88f95e1125a5e104d06103152e1d210cb44d0346))
* **studio:** await completed subagent workflow ([1d316f7](https://github.com/ZR233/pure-lang/commit/1d316f7972eef4846e5368a63bca8ad2aade0caa))
* **studio:** bind live explorer evidence ([c7801a1](https://github.com/ZR233/pure-lang/commit/c7801a1b83e64600eb5a07351c5f209078ac05e6))
* **studio:** bind reviewer approval evidence ([07c4b7e](https://github.com/ZR233/pure-lang/commit/07c4b7e66763bc0e20f95045467c10e27e86a8d3))
* **studio:** bind task evidence to agent receipts ([abd02b2](https://github.com/ZR233/pure-lang/commit/abd02b23f72b924b9616cdfb5a6f225185d90240))
* **studio:** bound live driver settings scroll ([a20bbe0](https://github.com/ZR233/pure-lang/commit/a20bbe0090993d4c6f586511df75c5d1eae21421))
* **studio:** bound live explorer planning scope ([ed84ed9](https://github.com/ZR233/pure-lang/commit/ed84ed911ffb7b8f81952b3fffe0a366a6a8d936))
* **studio:** complete task subagent live orchestration ([9edd1ce](https://github.com/ZR233/pure-lang/commit/9edd1cee2edd9cbbc049dbde2f2a6fd199aa6a13))
* **studio:** correct task live evidence ordering ([7a7c2a0](https://github.com/ZR233/pure-lang/commit/7a7c2a0ee4a980144e01582c39a9d8e24fc65a34))
* **studio:** count successful explorer receipts ([f41d905](https://github.com/ZR233/pure-lang/commit/f41d905a831721a34a8b2f79c3b1845504b7a8b1))
* **studio:** ignore failed spawn receipts in live verifier ([3ab7cbb](https://github.com/ZR233/pure-lang/commit/3ab7cbbf2facad6d0d26842ce5a836e85ec90523))
* **studio:** make task live fixture executable ([9c4e4f3](https://github.com/ZR233/pure-lang/commit/9c4e4f32aa71441a3e926ab46c08221c9028bfb1))
* **studio:** observe canonical agent settings revision ([f64a773](https://github.com/ZR233/pure-lang/commit/f64a773640b6154ca08c9c4b57c7c4b2023856ce))
* **studio:** require durable reviewer verdicts ([6a4d315](https://github.com/ZR233/pure-lang/commit/6a4d31541ed85517a8c82a756c1043d52eed2f74))
* **studio:** strengthen task subagent evidence ([e74de5d](https://github.com/ZR233/pure-lang/commit/e74de5d593369d539f75748af8ff259dc2e4f2d2))
* **studio:** validate every task child contract ([8cac429](https://github.com/ZR233/pure-lang/commit/8cac42933a303e6489364e9281278eaf92d09891))
* **studio:** validate live output markers ([7ce082a](https://github.com/ZR233/pure-lang/commit/7ce082a9d5fd13141ad3d75ea57c79e652a0419e))
* **studio:** validate terminal child fallbacks ([39ae11a](https://github.com/ZR233/pure-lang/commit/39ae11ab6b73ac3c203108659bdf1a3044a3e2fa))
* **studio:** 优化 Timeline 等待态布局 ([1780c2e](https://github.com/ZR233/pure-lang/commit/1780c2ec7ffd94c8b4f17b9e6557b0d4f6249d60))
* **studio:** 会话时间线文本支持选中与复制 ([746b7ac](https://github.com/ZR233/pure-lang/commit/746b7acee9f5327e21d50817a213882c27c5a9a1))
* **studio:** 使用冻结子代理配置准备工作区 ([7afc750](https://github.com/ZR233/pure-lang/commit/7afc750eea83bc7b6912b120c47e7a65d74924b0))
* **studio:** 修复 SSH 服务器添加确认流程 ([520be23](https://github.com/ZR233/pure-lang/commit/520be23ca2c308ba24572ca563724a39377c39ba))
* **studio:** 修复 SSH 项目技能目录不进入设置页 ([b025b87](https://github.com/ZR233/pure-lang/commit/b025b87b06af3fbc6d155ba0f7b38d4a78d3da71))
* **studio:** 修复 Windows worktree Git 路径 ([4b1e4d3](https://github.com/ZR233/pure-lang/commit/4b1e4d38ac16da4073171f3105a856ade8630082))
* **studio:** 修复任务工作流真实 GUI 验收 ([540a58b](https://github.com/ZR233/pure-lang/commit/540a58bbaaeb4a788b4aac819417358c552922a3))
* **studio:** 修复会话费用归集与显示 ([35e59c7](https://github.com/ZR233/pure-lang/commit/35e59c764ba93842ae873c4fd37fc13ce707bcb1))
* **studio:** 修复计划确认失败与残留交互 ([618e449](https://github.com/ZR233/pure-lang/commit/618e449952fe827d00c8c8bf9dc7efbfe5da5075))
* **studio:** 同步供应商模板默认配置 ([20d46f1](https://github.com/ZR233/pure-lang/commit/20d46f1ce27c9421bc4551edf9eb185b93ce105d))
* **studio:** 增加五次断联重试并显示重试进度 ([e8e0c93](https://github.com/ZR233/pure-lang/commit/e8e0c93007f900dbe6fa3265bd953808b59991ec))
* **studio:** 完善 Agents 设置国际化 ([4e3fa33](https://github.com/ZR233/pure-lang/commit/4e3fa33fd68c0c87b7768d74b82525b480ea1bb3))
* **studio:** 恢复不兼容配置并继续启动 ([c875be3](https://github.com/ZR233/pure-lang/commit/c875be3e86e7625e10a1360b3d858b013455bf5c))
* **studio:** 按子代理轮次验证返工整合 ([5cea504](https://github.com/ZR233/pure-lang/commit/5cea504f10ec72da6c02e3844ee6bb80ef2e0f61))
* **studio:** 明确 planning 澄清与计划提交顺序 ([6e1adab](https://github.com/ZR233/pure-lang/commit/6e1adabf08c293a2461ab835688c301a478a242b))
* **studio:** 明确工作流工具参数契约 ([e6ba2b3](https://github.com/ZR233/pure-lang/commit/e6ba2b3f785b57f13fcd4392218d0b9f415b14d2))
* **studio:** 显示会话部分未计价状态 ([468a461](https://github.com/ZR233/pure-lang/commit/468a46162d336ceac426cd8de468b47f37270bfa))
* **studio:** 绑定 worktree 交付提交与整合 ([3653c76](https://github.com/ZR233/pure-lang/commit/3653c76fb2add085bd8b3964d7a4c9171ebaa473))
* **studio:** 统一任务模式计划确认入口 ([0efc5a9](https://github.com/ZR233/pure-lang/commit/0efc5a92baf85217d9398f97ef1f60ea8e83d9fb))
* **studio:** 补全界面硬编码文案的国际化 ([3a1ffba](https://github.com/ZR233/pure-lang/commit/3a1ffbaf1ba25faa53e858d2d16a07b25eea9a9b))
* **studio:** 规范化 executor 工作区路径身份 ([9a4b1e3](https://github.com/ZR233/pure-lang/commit/9a4b1e3ab98f0dae5e92a61d6b4ed9887de78b83))
* **studio:** 解耦内存会话与异步持久化并修复轮次停滞 ([ab28dca](https://github.com/ZR233/pure-lang/commit/ab28dca491fafe3dd38f3bc133f1f7dabba944f3))
* **workflow:** allow partial transition wire replays ([ae4344f](https://github.com/ZR233/pure-lang/commit/ae4344fed7f88bbd675858279d31e1b2b4bc3c17))


### Refactoring

* **agent-runtime:** 统一 Agent 协议状态事实源 ([d5e49e5](https://github.com/ZR233/pure-lang/commit/d5e49e5aa7011069b873199100d15d8c8c9c63d7))
* **model:** 分层供应商能力并统一推理计量与分时计价 ([e332b00](https://github.com/ZR233/pure-lang/commit/e332b00eb352c34824cd80880313772f9d8e25ce))
* **pl-core:** 统一工具定义与动态执行框架 ([9f340eb](https://github.com/ZR233/pure-lang/commit/9f340eb02bfd0998b71afdee2e007d0ffa20045c))
* **pl-core:** 重构模块结构与 crate 根命名空间门面 ([e1d1f13](https://github.com/ZR233/pure-lang/commit/e1d1f134ac341f9522c585a4190012ae88de0f4b))
* **pl-lsp:** 重组模块与公开命名空间 ([c6adabf](https://github.com/ZR233/pure-lang/commit/c6adabfbc570dff10585673528d9bb508bfa724d))
* **pl-model,pl-core:** 按公共接口完整性重导出依赖类型 ([fac7250](https://github.com/ZR233/pure-lang/commit/fac725003bd6b1aea22bbc52866b2e6b3d546f5c))
* **pl-model:** 含糊布尔参数领域化 ([839db4c](https://github.com/ZR233/pure-lang/commit/839db4c956e4c2662157b2987096338d66102907))
* **pl-model:** 拆分超长模块并分解巨型函数 ([04f7c66](https://github.com/ZR233/pure-lang/commit/04f7c6692d85a6e0b1a6e04a01914fc42895f230))
* **pl-model:** 类型化模型校验错误 ([2690ecd](https://github.com/ZR233/pure-lang/commit/2690ecd0ec176a53df7c72f11f37b4316fad64d9))
* **pl-model:** 重组公开命名空间为域模块 ([345d6eb](https://github.com/ZR233/pure-lang/commit/345d6eba316936b4e48d94781f872b86cc39f793))
* **studio-runtime:** 重构模块结构、根命名空间门面与测试质量 ([f8a549a](https://github.com/ZR233/pure-lang/commit/f8a549af9f6034a61070d9759fb39fbd4de53491))
* **studio:** 合并重复简体中文资源 ([4937a80](https://github.com/ZR233/pure-lang/commit/4937a8075fabedba611fbe3429c4f3ed617b6c7f))
* **studio:** 统一模式与可编译工作流 ([e7fcae9](https://github.com/ZR233/pure-lang/commit/e7fcae91ba76d0f92652c4f46c868384c258fb2f))
* **studio:** 重构会话热状态与任务验收 ([dba1edb](https://github.com/ZR233/pure-lang/commit/dba1edbe9b8457c07498257a230b547343f7df5b))
* **thread-mode:** 统一 Mode 与 AgentSession Plan ([ddb256c](https://github.com/ZR233/pure-lang/commit/ddb256c27cada012dab288f5cb4bff7f84a38167))
* **tool:** 统一工具运行时 ([1d0aebf](https://github.com/ZR233/pure-lang/commit/1d0aebf1731a84cbfa98b92d2a9da3963bdad0ac))


### Documentation

* clarify authorization and validation boundaries ([7cf2459](https://github.com/ZR233/pure-lang/commit/7cf2459e4d50c83aea3afe50aa2e06c8477e9ca7))
* **pl-protocol:** 修正 TokenUsageSnapshot 来源注释 ([7ab28c4](https://github.com/ZR233/pure-lang/commit/7ab28c43dfee917a5cccd6c5f9c2a8f244bcce4f))
* **remote:** document remote instruction loading ([d3810b7](https://github.com/ZR233/pure-lang/commit/d3810b74aa274586fbd7eb267626ab2381880d7f))
* **skills:** 固化产品宿主注册语义 ([5475a20](https://github.com/ZR233/pure-lang/commit/5475a20ad25faed0df2ea2f982baf46e3ac4189e))
* **skills:** 泛化代码质量重构规范 ([5e8ba78](https://github.com/ZR233/pure-lang/commit/5e8ba789571f00f93e7b2bce203f307b358f28ed))
* **studio:** define subagent-first task orchestration ([3429097](https://github.com/ZR233/pure-lang/commit/3429097c99f5dcd636c63affc3308aba0c498d00))
* **studio:** require durable reviewer verdicts ([175ce44](https://github.com/ZR233/pure-lang/commit/175ce443ce8a6f60fd77214a5252a028637994c7))
* **workflow:** document partial wire replay semantics ([31ee90e](https://github.com/ZR233/pure-lang/commit/31ee90e986ab42d7f300fea7ad8e0df0c56c8103))
* 固化 Rust 测试分层规范并清理残余兄弟测试文件 ([d528984](https://github.com/ZR233/pure-lang/commit/d528984ea4be6857a23b363a7663f3a0a01c4caa))


### Maintenance

* merge latest origin/main ([3d35393](https://github.com/ZR233/pure-lang/commit/3d35393b2037ce24908ea90f949f832e660e1a4a))
* **skills:** 清理重复的代码审查技能 ([35af488](https://github.com/ZR233/pure-lang/commit/35af4888dc208f0417831fcec8481da7bd0a7727))
* **studio:** 修复 Driver 脚本格式 ([79ae863](https://github.com/ZR233/pure-lang/commit/79ae863ebbc40643cfe8e729c2447a364bee9730))
* 添加 Rust 与测试质量项目技能 ([db0435d](https://github.com/ZR233/pure-lang/commit/db0435d3b34bd3e4155455da911a02baace013d0))

## [2.0.0](https://github.com/ZR233/pure-lang/compare/v1.0.3...v2.0.0) (2026-08-25)


### ⚠ BREAKING CHANGES

* **studio:** ThreadRepository 端口删除 flush_pending（屏障统一 awaitDurable）； StudioResolveInteractionResponse 移除 threads 字段；StudioStore 目录直写方法不再对外。
* **studio:** 建立内存权威与异步持久化
* **task:** 删除旧任务状态、旧状态工具、ProjectLease 与历史数据库兼容入口，统一使用六态 TaskRun 和 task_transition。
* **runtime:** 移除旧生命周期 Rust API、状态字符串、wire JSON、Dart model 与数据库兼容读取。
* **studio:** 重建 TaskRun、WorkUnit、ReviewRound 持久化及 HTTP、SSE、FRB、Dart 状态协议，不兼容旧数据库和恢复快照。
* **studio:** 移除旧 Bridge DTO 与宽松任务派发/完成协议，Studio 适配器统一消费新的 canonical runtime API。
* **pl-model:** 重构模型目录、provider 与运行时边界
* **studio:** UI 会话创建入口由 createThread 改为 startNewThread，项目允许零 Thread。
* **studio:** Studio 状态 API 改为无副作用查询与显式领域命令，旧接口与兼容路径全部删除。
* **agent-runtime:** 删除自研 MCP wire、client、transport 和专用 backend，仅支持 MCP 2026-07-28。
* **agent-runtime:** 移除子代理→主代理及 peer push；send_message 仅允许 parent→child。 移除 tools_without_send_message、Task 专用 task_send_message_tool 与 authorize_executor_message。thread_inputs 不再承担子代理报告语义。DB schema 升至 v6（带 v5→v6 迁移，保留既有数据）。
* **agent-runtime:** 会话活动状态改由 canonical Thread 快照单向派生，并移除多轴写入 API。
* **studio:** Studio schema 升级到 v3，旧 schema 启动时按既定策略重建数据库。
* **agent-runtime:** 增量化 wait_agents 返回协议
* **studio:** Studio 会话协议和持久层统一为 Thread、Turn、Item，旧数据库仅归档不导入。
* **studio:** Studio v10 数据库将被归档并重建为 studio_state.sqlite v11 与 studio_history.sqlite v1，不导入旧数据。
* **studio:** Studio SQLite v1-v9 改为归档后重建 v10，并移除旧协作工具、wake/continuation/delivery recovery、migration 与 raw JSON 兼容路径。
* 移除旧审批策略、旧命令别名、legacy Studio JSON/FRB 字段及 --demo-fallback。
* **studio:** session event wire 升级为 typed turn state、双通道 reasoning 与 typed mailbox presentation；旧客户端不再兼容运行期 wire。

### Features

* **agent-runtime:** 增量化 wait_agents 返回协议 ([9cf987d](https://github.com/ZR233/pure-lang/commit/9cf987d53c48761a3285bd6c7e3c4c692461465d))
* **agent-runtime:** 子代理通信改为 pull 模型并强化审查修复建议 ([#50](https://github.com/ZR233/pure-lang/issues/50)) ([72570b8](https://github.com/ZR233/pure-lang/commit/72570b84b08a60073af2985f8ff81c0438648102))
* **agent-runtime:** 支持工具发现、程序化调用和编排指标 ([e32f26b](https://github.com/ZR233/pure-lang/commit/e32f26b137483f2e98041ae9d5a2e13efcffd85f))
* **gui:** 支持 Linux 桌面平台构建 ([bddd1fd](https://github.com/ZR233/pure-lang/commit/bddd1fd46ba89fb32cef49e088acb0a925a31e06))
* **model:** derive host requests from resolved routes ([efa318a](https://github.com/ZR233/pure-lang/commit/efa318ad1469d5c0a615487d7ece38e6beb1b9d6))
* **model:** 增加 GLM-5.3 模型支持 ([#53](https://github.com/ZR233/pure-lang/issues/53)) ([01dce34](https://github.com/ZR233/pure-lang/commit/01dce34f132b9e9a3d8b33e1e8baf28bd41d2768))
* **pl-core:** client tool_search 拦截记录结构化 toolCall item ([129ec89](https://github.com/ZR233/pure-lang/commit/129ec895f0c29b232c1fd46cbc8162250711fdb8))
* **pl-core:** runtime snapshot 投影 Turn 冻结工具诊断 ([d36d076](https://github.com/ZR233/pure-lang/commit/d36d07665d02cf9110d938bab0a9cd6ed7a755ea))
* **pl-protocol:** 会话消息与 durable 线程携带 typed 工具 transcript ([52bb7cd](https://github.com/ZR233/pure-lang/commit/52bb7cd2ab765c6d084140d2ffa8c2eb9f42dba8))
* **studio:** add managed Task executors and live model test ([79661cf](https://github.com/ZR233/pure-lang/commit/79661cfc853a55b50402bafc51f6089e7a813369))
* **studio:** config.toml 支持声明自定义 LSP server ([b678606](https://github.com/ZR233/pure-lang/commit/b678606a40804d669c0a713422576431dfcabb51))
* **studio:** refine reasoning activity timeline ([8cbcf06](https://github.com/ZR233/pure-lang/commit/8cbcf06fa7b6e2f420924e8bb0bddb8d039f53bb))
* **studio:** refine tool activity display ([5fc8d25](https://github.com/ZR233/pure-lang/commit/5fc8d25303f3e8acd38b3c7160f42df0668d8fe3))
* **studio:** Timeline 提供 tool search 卡片与 LSP 工具识别 ([6ac80d8](https://github.com/ZR233/pure-lang/commit/6ac80d87e6fbbb9a60797230ff998ffa15102254))
* **studio:** unify Pure Studio identity and icon ([1cceb50](https://github.com/ZR233/pure-lang/commit/1cceb50d3b3309a3a9741e7ad7036d648a39f8d1))
* **studio:** 会话内存权威存储、GUI 分页窗口与优雅关机阶段界面 ([2cb6885](https://github.com/ZR233/pure-lang/commit/2cb688563beab920cf4422f8ebb69f874b993495))
* **studio:** 升级 SeaORM 2.0 并重构双库会话持久化 ([5410572](https://github.com/ZR233/pure-lang/commit/5410572cafd1e7605110c4cec577fdf214dee968))
* **studio:** 历史分页改用 Turn id 锚点并窗口化会话快照 ([d284965](https://github.com/ZR233/pure-lang/commit/d2849651528b5370f1a6aacdb535bca36e0a7b94))
* **studio:** 固定角色显示名称本地化 ([#48](https://github.com/ZR233/pure-lang/issues/48)) ([ed8c286](https://github.com/ZR233/pure-lang/commit/ed8c28605905ea58b1e556ced72e38a118184fee))
* **studio:** 增加 LSP 索引等活动状态显示 ([#55](https://github.com/ZR233/pure-lang/issues/55)) ([f007a01](https://github.com/ZR233/pure-lang/commit/f007a01aa1aec5b92f0773c0b49cce09535a7ab4))
* **studio:** 增加配置指南系统 Skill ([#56](https://github.com/ZR233/pure-lang/issues/56)) ([9189c58](https://github.com/ZR233/pure-lang/commit/9189c5835a8aac0046373f1c83b4944c8e25f0bf))
* **studio:** 技能页切换时自动发现项目技能并替换缓存 ([#47](https://github.com/ZR233/pure-lang/issues/47)) ([53519f4](https://github.com/ZR233/pure-lang/commit/53519f4a4c95d43fd87661007ab75464d7ead997))
* **studio:** 支持 Task 会话回退与续跑验收 ([e8826b1](https://github.com/ZR233/pure-lang/commit/e8826b1282b57907bdd3da4a6399dac8e6d15f30))
* **studio:** 新会话起始页支持选择模式、模型与思考等级 ([b6c64d8](https://github.com/ZR233/pure-lang/commit/b6c64d8b67ff72b3c23a41891c1ec73bf16848be))
* **studio:** 状态栏费用与上下文明细收进详情并动态显示 LSP 索引 ([3f155cf](https://github.com/ZR233/pure-lang/commit/3f155cfac3cfcfd5f3b25e3a80c9e42296968e35))
* **studio:** 统一 Agent Workspace 与 Planner 自主合并 ([312fd00](https://github.com/ZR233/pure-lang/commit/312fd00c5cd9e22204519120e4b8359f31ce10e8))
* **studio:** 统一 Runtime 双适配与任务编排 ([dce79b7](https://github.com/ZR233/pure-lang/commit/dce79b70f05e8efccd032cfec07556213de33c1a))
* **studio:** 设置页 Skills 列表进入即读取 catalog ([#54](https://github.com/ZR233/pure-lang/issues/54)) ([f8c0532](https://github.com/ZR233/pure-lang/commit/f8c0532ee9e6bffe9b19b50e4c8ad04883e149df))
* **studio:** 重构上下文持久化与 executor 续轮 ([b1cf092](https://github.com/ZR233/pure-lang/commit/b1cf092be0c4dd9f3baaabb815aa86c100071a7d))
* **xtask:** 增加 Flutter 和 Dart 透传命令 ([a4cd530](https://github.com/ZR233/pure-lang/commit/a4cd5307b24dbc604896fce3afb0f107597a4623))


### Bug Fixes

* **agent-runtime:** retire closed thread state ([569a709](https://github.com/ZR233/pure-lang/commit/569a7095d832e4af205770bca0b70d3566140d89))
* **agent-runtime:** 修复子代理预算卡死与消息恢复 ([e209607](https://github.com/ZR233/pure-lang/commit/e2096076ad7f1ccaf9d92aba43aa9708ebf7c6b6))
* **agent-runtime:** 命令处理状态机放堆修复 debug 栈溢出 ([d49db4a](https://github.com/ZR233/pure-lang/commit/d49db4ac3cdd56a7607f098a88147e6a48ed962c))
* **agent-runtime:** 持久化阶段进度 commentary ([85b77bd](https://github.com/ZR233/pure-lang/commit/85b77bda795e10dce0f223d9f5dbb408069c9fb6))
* **agent-runtime:** 明确工具分页与补丁参数约束 ([d95a36a](https://github.com/ZR233/pure-lang/commit/d95a36a3752d968d48e36600044db610e39fd7c4))
* **agent-runtime:** 消除 release 构建的未使用导入与 mut 警告 ([0d524bc](https://github.com/ZR233/pure-lang/commit/0d524bc4d78bd82e418cc8f6d7f0524366736f5c))
* **agent-runtime:** 移除 search_files 并开放 exec ([dd2f9a6](https://github.com/ZR233/pure-lang/commit/dd2f9a6a6d3e09b5b61d88658f4ff8ba53d41cea))
* **agent-runtime:** 防止等待唤醒误停执行者 ([80dcd3b](https://github.com/ZR233/pure-lang/commit/80dcd3ba57422ad7f3d23b3d390dfb8f04b15861))
* **ci:** 修复 Rust lint 与 Dart 格式检查 ([1dffa99](https://github.com/ZR233/pure-lang/commit/1dffa99c275d2b6ae26d3efa9e75fd1ac6f584ad))
* **ci:** 忽略依赖 ripgrep 的测试 ([a94ee5e](https://github.com/ZR233/pure-lang/commit/a94ee5e7c90f39406e324c5ed614720a4eb7c430))
* **codex:** 删除不再需要的配置文件 config.toml ([9b8ef36](https://github.com/ZR233/pure-lang/commit/9b8ef36e9615de4fd6bcfab95675af80c4db325e))
* **lsp:** 修复语义查询并确保进程完整退出 ([be8f4e5](https://github.com/ZR233/pure-lang/commit/be8f4e5947d8a9303d3fa5a3dff42359d2a8b25b))
* **model:** SSE 流中断按瞬态传输错误重试并稳定目录窗口选择 ([a38769b](https://github.com/ZR233/pure-lang/commit/a38769bf668c7c4db10b2889d4b0d00bbe8f1942))
* **patch:** tolerate stale ARB context values ([ec838f4](https://github.com/ZR233/pure-lang/commit/ec838f4172ab6644ec0568f61cfd10e97f712e56))
* **path:** keep platform-specific requirements lint-clean ([c34cb47](https://github.com/ZR233/pure-lang/commit/c34cb47ac1d274dcf83174572d70bd591fc180cf))
* preserve Flutter lock across hosted mirrors ([e4b2ce9](https://github.com/ZR233/pure-lang/commit/e4b2ce917ab659784e91a71a40fa769749dd7f9b))
* rebuild incompatible Studio databases ([e768d1f](https://github.com/ZR233/pure-lang/commit/e768d1fe335189a4f144f4730e355df38c08859e))
* **studio:** deepseek-v4-pro 切换为 Responses API ([b642748](https://github.com/ZR233/pure-lang/commit/b642748abfbf0480714fec874638c51678ccbff8))
* **studio:** isolate agent sessions and recover task delivery ([ae325ad](https://github.com/ZR233/pure-lang/commit/ae325aded1eff18a4f66a513a38160020ab7412a))
* **studio:** isolate recovery failures and add safe cleanup ([5f382e4](https://github.com/ZR233/pure-lang/commit/5f382e484d03d6be48a920e8aabb395729e9bd2d))
* **studio:** LSP repair 状态标签同步为 missingServerComponent ([239b558](https://github.com/ZR233/pure-lang/commit/239b558dbf818d1c357aa16eb3702849c4951386))
* **studio:** preserve delivery across interrupted turns ([e9772a2](https://github.com/ZR233/pure-lang/commit/e9772a2e1a53cbe9b7fb81c37c774a3b07c1a594))
* **studio:** 上下文详情无花费时费用显示 '-' 占位 ([f7b7b41](https://github.com/ZR233/pure-lang/commit/f7b7b4104e070199905b0bef1e536f4892de414c))
* **studio:** 修复 CI 验收失败（Git 身份与提交竞态） ([41561b4](https://github.com/ZR233/pure-lang/commit/41561b4ae654e728f8bd1ac4450be5fb2f077c47))
* **studio:** 修复 GUI 构建与任务执行流程 ([f13ef99](https://github.com/ZR233/pure-lang/commit/f13ef99fefbdab79809a5f42432d2e1d8da1e7e0))
* **studio:** 修复 GUI 输入后的静默失败 ([e0084fe](https://github.com/ZR233/pure-lang/commit/e0084fe70ec75b72f63f1f806a4669f2c0381e57))
* **studio:** 修复 GUI 重复构建缓存失效 ([1a98f9e](https://github.com/ZR233/pure-lang/commit/1a98f9e0444cb047f2fd3bafdd3f269b9c466826))
* **studio:** 修复 MCP HTTP 协议回退 ([44ca23f](https://github.com/ZR233/pure-lang/commit/44ca23f1fcea1e36dfc94656027a45097e78d6ef))
* **studio:** 修复 Responses 工具回放与预算中止语义 ([7935793](https://github.com/ZR233/pure-lang/commit/7935793119313b0a631153cfa173660c880b1327)), closes [#39](https://github.com/ZR233/pure-lang/issues/39)
* **studio:** 修复任务执行续接与工具调用结果展示 ([bb6e9c8](https://github.com/ZR233/pure-lang/commit/bb6e9c8e210a91015fd38cc83f0e72f7abfbd6cc))
* **studio:** 修复会话归档与空会话流程 ([610d879](https://github.com/ZR233/pure-lang/commit/610d8799977207048ffab7dc8e166ac8ac301b26))
* **studio:** 修复会话模式切换与快照同步 ([2f50a2f](https://github.com/ZR233/pure-lang/commit/2f50a2f8f355f788587a6e7951f7f1bbf10898ab))
* **studio:** 修复关机卸载读取 ref 与线程切换卡在 loading ([03c49c4](https://github.com/ZR233/pure-lang/commit/03c49c45e84fc2d465e5304c1f24d59e5441514d))
* **studio:** 修复已完成待办阻止任务结束 ([6405dc8](https://github.com/ZR233/pure-lang/commit/6405dc851fa4158c5126c0f08db006eb8a4dbf1b))
* **studio:** 修复持久化子线程导致的启动失败 ([75cf09e](https://github.com/ZR233/pure-lang/commit/75cf09e68cef57893d53f818a8211463d70c4744))
* **studio:** 修复运行时测试清理超时 ([3e97a1c](https://github.com/ZR233/pure-lang/commit/3e97a1c90704553d377098ca188616b07358fe70))
* **studio:** 修复项目清理确认无效 ([001f685](https://github.com/ZR233/pure-lang/commit/001f685959a992fa3b887792a2b9e4ccafddb683))
* **studio:** 允许缺失 Windows native assets ([dc24860](https://github.com/ZR233/pure-lang/commit/dc248606c36b88a14a197621ef6eba5c9551121c))
* **studio:** 分层处理任务失败并回传验证错误 ([72ac3d8](https://github.com/ZR233/pure-lang/commit/72ac3d802f57abbea3c684509e1b21479e63f102))
* **studio:** 后台启动 MCP 并隐藏 stdio 终端 ([868058a](https://github.com/ZR233/pure-lang/commit/868058a1deb48fb32ad02e6b22d5326847bcceb2))
* **studio:** 复用 workspace Rust bridge 产物 ([c1c4a17](https://github.com/ZR233/pure-lang/commit/c1c4a17b06fd947b4bc6997a8fdeea49e73deeb3))
* **studio:** 安全清理失败项目并防止 Planner 误打断 ([5e3a87e](https://github.com/ZR233/pure-lang/commit/5e3a87e30ed9e2219041213288cbeaada72beae2))
* **studio:** 完善任务审查与角色门禁 ([860fb60](https://github.com/ZR233/pure-lang/commit/860fb60c3a6ee199969563013d040c1c0d062668))
* **studio:** 展示流式 reasoning 内容 ([#30](https://github.com/ZR233/pure-lang/issues/30)) ([7acd2a1](https://github.com/ZR233/pure-lang/commit/7acd2a1c6d15c06b9aef87acc0b6ca1cebe93156))
* **studio:** 恢复任务计划时间线展示 ([0fe14a5](https://github.com/ZR233/pure-lang/commit/0fe14a505c7931a38deedcaa475634aa3b539d39))
* **studio:** 恢复新会话与会话归档功能 ([443acdf](https://github.com/ZR233/pure-lang/commit/443acdfe7d3164876f3fbcd4d9d25b197e619115))
* **studio:** 按模型配置 API 协议与传输 ([ed7891d](https://github.com/ZR233/pure-lang/commit/ed7891d108215baef462cb29430784ced8e22259))
* **studio:** 提交计划调整后继续规划 ([6f87a83](https://github.com/ZR233/pure-lang/commit/6f87a832825ec22ceba2dacc082e8a80cd92ebef))
* **studio:** 提升 DeepSeek 缓存命中并持久化计费 ([125a10d](https://github.com/ZR233/pure-lang/commit/125a10d6421f640abb32a311879a85133b2f252b))
* **studio:** 用户交互后从持久化新 Turn 继续 ([bfe9afa](https://github.com/ZR233/pure-lang/commit/bfe9afad26ecea9bca2b21a2be0745c15d099014))
* **studio:** 移除任务完成阶段的项目验证 ([8fea360](https://github.com/ZR233/pure-lang/commit/8fea360a06881ea87e746bd35313251162fce86b))
* **studio:** 稳定 Task 续轮、交接与工具重试 ([b7d39a6](https://github.com/ZR233/pure-lang/commit/b7d39a655228f42614f83d0b3181a8263cb84266))
* **studio:** 简化模式架构并优化任务完成条件 ([524229b](https://github.com/ZR233/pure-lang/commit/524229bcd4f361f2861b7da32ab0a9ec7b47d78d))
* **studio:** 精确钉版 file_picker 修复 CI 锁文件漂移 ([7796f87](https://github.com/ZR233/pure-lang/commit/7796f8707561bacee247e5fcf3b7cae5cf7446e7))
* **studio:** 统一后台进程配置并禁止 Windows 弹窗 ([e272677](https://github.com/ZR233/pure-lang/commit/e272677e4b684c17d9f522d844808f285a0be210))
* **studio:** 补全 zh_Hans LSP 状态翻译消除 l10n 构建警告 ([8c5cde6](https://github.com/ZR233/pure-lang/commit/8c5cde6cc44c7583819050b27dfaead65b55a19b))
* **studio:** 补齐 test_driver 目录的 dart format ([79b8f92](https://github.com/ZR233/pure-lang/commit/79b8f92239cd4396b6ec0dbc8f7cab971474ec58))
* **studio:** 规范化 pubspec.lock 为 pub.dev 源并同步解析版本 ([1620c38](https://github.com/ZR233/pure-lang/commit/1620c38db17f60c76b04a84eb473eb04fd9dcc61))
* **studio:** 避免 GUI 构建改写源码 ([d81694e](https://github.com/ZR233/pure-lang/commit/d81694e86a1f0f6071711fa6e933f6b6a842b3cb))
* **studio:** 避免 npx MCP 弹出终端 ([c0ed01f](https://github.com/ZR233/pure-lang/commit/c0ed01f918b2a37967a1c4f494d169f614b3ccc7))
* **studio:** 避免异步提交在销毁后访问状态 ([f3e7aeb](https://github.com/ZR233/pure-lang/commit/f3e7aeb463b36f64fb3ad2207c18206fd34a8791))
* **studio:** 防止 Job Object 覆盖无窗口标志 ([f209e97](https://github.com/ZR233/pure-lang/commit/f209e979bdac7493971f62470e7313300cc7f904))
* **studio:** 隐藏 npm MCP 后代进程终端 ([dacf442](https://github.com/ZR233/pure-lang/commit/dacf4422f578e127d5c7bc69b9bc070688372f39))
* **studio:** 隔离 MCP 终端并兼容旧协议 ([6ece53b](https://github.com/ZR233/pure-lang/commit/6ece53bdb2deb539efeac1c28eaffe49a2e4821a))
* **studio:** 隔离不可用项目并恢复空项目状态 ([09e19a9](https://github.com/ZR233/pure-lang/commit/09e19a93d0731a57036d08d33828aac6e992ef6f))
* **studio:** 隔离测试运行时系统凭据 ([e94f691](https://github.com/ZR233/pure-lang/commit/e94f6918e7175f7357bc7a2368c3fc8fe5adb590))
* **tool:** expose output artifact receipts to models ([7bb6999](https://github.com/ZR233/pure-lang/commit/7bb6999c8215b62f6e2071df77e3cd2fe0f0363f))
* 统一 Linux 路径与工具执行边界 ([b150194](https://github.com/ZR233/pure-lang/commit/b1501941fa467c160866dce6da070cdec254693b))


### Performance

* **agent-runtime:** 并行安全工具并限制批次结果预算 ([d760eb7](https://github.com/ZR233/pure-lang/commit/d760eb7ec3ded6f913d1fa35ed1aaa3d4d726162))
* **studio:** 优化提示词缓存与计费可观测性 ([#45](https://github.com/ZR233/pure-lang/issues/45)) ([d70ec43](https://github.com/ZR233/pure-lang/commit/d70ec435d1de900b20b798605ccd6a8d506353e4))
* **studio:** 启动异步化 MCP/LSP 探测，只等待主界面必要内容 ([b54265a](https://github.com/ZR233/pure-lang/commit/b54265a017b085bc4048a4e4609b43db7eb9c23e))


### Refactoring

* **agent-runtime:** use subscription-based planner wakeups ([1aa0589](https://github.com/ZR233/pure-lang/commit/1aa0589dd268ddccc19db972a9bc3006b4e6aeef))
* **agent-runtime:** use subscription-based planner wakeups ([b4e4c9c](https://github.com/ZR233/pure-lang/commit/b4e4c9c0119085eecd0874bcbb64fe5b1f3ac9a3))
* **agent-runtime:** 使用 rmcp 统一 MCP 工具运行时 ([d21b09c](https://github.com/ZR233/pure-lang/commit/d21b09cf000371041bfb1f820ea3b3235c042b35))
* **agent-runtime:** 使用 typed schema 定义静态工具 ([9998f45](https://github.com/ZR233/pure-lang/commit/9998f450c5762f56bdb34581e4bcb0d6e5b19cbe))
* **agent-runtime:** 收敛会话状态为单事实源 ([eeae730](https://github.com/ZR233/pure-lang/commit/eeae73047f7640875425e40a27a660940ce783db))
* **core:** ThreadEventBus 成为 timeline ordinal 唯一分配者并稳定会话选择 ([61a8fe3](https://github.com/ZR233/pure-lang/commit/61a8fe37ff9af623d771d04ed1db3fc74deb1a93))
* **core:** 复用领域结构并优化内存布局 ([256e51c](https://github.com/ZR233/pure-lang/commit/256e51c2968f4c933cc885bdf5f77ffeb485c0de))
* **core:** 模块化核心编排并复用共享结构 ([f75bfff](https://github.com/ZR233/pure-lang/commit/f75bfff51f31f6b5cff4d233a19a79df39a0abe1))
* **core:** 用 From 和 TryFrom 收口类型转换 ([14f0c37](https://github.com/ZR233/pure-lang/commit/14f0c37288e9a4da6227461fc70f94fe706b988d))
* **error:** 使用 thiserror 简化库错误定义 ([#52](https://github.com/ZR233/pure-lang/issues/52)) ([6e20b11](https://github.com/ZR233/pure-lang/commit/6e20b11f633866d34f92c00c36c6588ec0ee73fc))
* **infra:** 收敛凭据、LSP 与文件基础设施 ([76ac72e](https://github.com/ZR233/pure-lang/commit/76ac72e39441ea6942e20b74ddf71a8313b80190))
* **pl-core:** 工具注册表改为来源代际发布并统一调度 lease ([3dbc7fd](https://github.com/ZR233/pure-lang/commit/3dbc7fd8d60fc0724c9314e1baa5447b966ea1f4))
* **pl-lsp:** server 定义与生命周期迁移到数据 catalog 和 driver ([0002f4b](https://github.com/ZR233/pure-lang/commit/0002f4b6cb51caf0b7279c11ad71638b56bf7e29))
* **pl-model:** ToolCall 携带必填 call_id 与 typed 身份 ([75044e7](https://github.com/ZR233/pure-lang/commit/75044e745b62815fe028d4048b92b373c4330f23))
* **pl-model:** 移除 hosted tool search 与 Namespace 工具 wire 变体 ([5ba796b](https://github.com/ZR233/pure-lang/commit/5ba796b110ba364249c1ef743b743d2cde5c940a))
* **pl-model:** 重构模型目录、provider 与运行时边界 ([f5ff660](https://github.com/ZR233/pure-lang/commit/f5ff6608a0fecbe70b4b11c787f6db9a89e5af0b))
* **runtime:** 全仓生命周期切换为强类型状态机 ([0412d5d](https://github.com/ZR233/pure-lang/commit/0412d5d7261187905bb46f4b31bd5e2165881ddb))
* **runtime:** 收敛 Thread 状态与重复工具入口 ([2b5b595](https://github.com/ZR233/pure-lang/commit/2b5b5952fe74de677b28a01ed7f9c45037a97668))
* **studio:** active_turns 改派生，移除 runtime state 手工维护 ([c3bf8e0](https://github.com/ZR233/pure-lang/commit/c3bf8e074fa0a273bec99a4894120f9de0b29a22))
* **studio:** recovery_issues 拆为独立 StudioRecoveryRegistry ([59b2801](https://github.com/ZR233/pure-lang/commit/59b2801fb9b294563833bc7cac702c323938f2fc))
* **studio:** schema v8 精确重建替代跨版本迁移链 ([3997994](https://github.com/ZR233/pure-lang/commit/3997994827ce581d4e9bb766167a014b1c5706de))
* **studio:** turn 装配改用共享工具注册表并按来源发布任务工具 ([b58c339](https://github.com/ZR233/pure-lang/commit/b58c3398d4754b9d936ad219ab6ec6dff1e289b4))
* **studio:** 使用 futures crate 简化 boxed future/stream 类型 ([b7ec7ea](https://github.com/ZR233/pure-lang/commit/b7ec7ea5ffb5337e5fdcb2090183ae0dc203b45b))
* **studio:** 建立内存权威与异步持久化 ([a13dcd7](https://github.com/ZR233/pure-lang/commit/a13dcd7d10aa580228f365d760226cbadd2e059e))
* **studio:** 抽出 LabeledEnum trait 与 repository labels 模块 ([ca2c995](https://github.com/ZR233/pure-lang/commit/ca2c995def9f45d481d38b9e62fc931fefb5b1ca))
* **studio:** 移除 Task Git 门禁并完善 executor 创建错误 ([db687f4](https://github.com/ZR233/pure-lang/commit/db687f42bff9a3a55c4182cc6e1ce759381834d5))
* **studio:** 精简 Turn 执行器与任务审查闭环 ([3d03ec9](https://github.com/ZR233/pure-lang/commit/3d03ec9a54154c6efea416dc37608b2749576daa))
* **studio:** 统一 Thread Turn Item 会话架构 ([9743170](https://github.com/ZR233/pure-lang/commit/97431709d98ccd03a17be8945d049803f3c516e0))
* **studio:** 统一历史窗口模型修复多会话切换历史错乱 ([34d4c26](https://github.com/ZR233/pure-lang/commit/34d4c263fdabc2087bc02949cd8851183c00e128))
* **studio:** 统一目录与任务持久化为内存唯一真源 ([f097269](https://github.com/ZR233/pure-lang/commit/f097269594052b320786265143720efeee46fbc7))
* **studio:** 重构 FRB Riverpod 与 Driver 边界 ([534fd78](https://github.com/ZR233/pure-lang/commit/534fd780a75aa90a39ae255c1606d14bb60af7d7))
* **studio:** 重构 Task 数据承载状态机 ([6598c59](https://github.com/ZR233/pure-lang/commit/6598c59c68fbd4ba4beadbbb568ab560440670e0))
* **studio:** 重构 Timeline 当前 Turn 状态机 ([#34](https://github.com/ZR233/pure-lang/issues/34)) ([900841f](https://github.com/ZR233/pure-lang/commit/900841f3eefe3fcbeb9d841fbcf7aabad757a2df))
* **studio:** 重构状态查询与领域生命周期 ([5c29b12](https://github.com/ZR233/pure-lang/commit/5c29b126fdc7b0943c8a482bc454f37c0a179011))
* **task:** 按六态编排彻底重构任务模式 ([d094784](https://github.com/ZR233/pure-lang/commit/d09478433fee47d767b27f580eb1ef339f158c7d))
* 使用 From/TryFrom 简化类型转换 ([7b635ac](https://github.com/ZR233/pure-lang/commit/7b635acd054fe52d384e7b4e4b40a74a4ae0a252))
* 拆分超大模块并规范化目录结构 ([9caecb3](https://github.com/ZR233/pure-lang/commit/9caecb31fea0766e5c650b7306a254a3a5ef7bab))
* 清理旧兼容层并收敛现役架构 ([fa4b6e0](https://github.com/ZR233/pure-lang/commit/fa4b6e0dd1f5a4d6409701f845fabb16b6924739))
* 清理重复实现并收敛测试边界 ([8cf27f9](https://github.com/ZR233/pure-lang/commit/8cf27f919021e70119feae88647e075abf91143b))
* 简化 Rust 模块导入 ([8284947](https://github.com/ZR233/pure-lang/commit/8284947b1a4074708e8fe346b882179c63a25203))
* 统一内部实现并清理兼容层 ([0608907](https://github.com/ZR233/pure-lang/commit/0608907e155feb95193ecf4aca6c8b4bb9f79b70))


### Documentation

* **release:** 规范提交与 PR 标题 ([#29](https://github.com/ZR233/pure-lang/issues/29)) ([12a64a6](https://github.com/ZR233/pure-lang/commit/12a64a68f5608f0469b2b9b2db2cda6ed069e929))
* **studio:** 上下文详情费用行固定展示约定 ([777a6da](https://github.com/ZR233/pure-lang/commit/777a6da3aea1fb9f95045699d7a9dadfa05eb503))
* **studio:** 完善任务模式状态转换说明 ([5637ad6](https://github.com/ZR233/pure-lang/commit/5637ad632e5e5be8eb49f81e5eb4a87e109ad490))
* **task:** 统一任务生命周期为六状态 ([f7703e4](https://github.com/ZR233/pure-lang/commit/f7703e407cae6c1e098178d4b037ea86abe611ad))
* 同步 MCP 统一来源与命名空间发布描述 ([f629d6b](https://github.com/ZR233/pure-lang/commit/f629d6bae40ebdf3283614d3e5a064701f81719e))
* 同步工具诊断分层与 tool search 展示设计 ([7fe9f7a](https://github.com/ZR233/pure-lang/commit/7fe9f7a24abcf6a82dde1258417dedde66f3f66a))
* 同步提交前本地检查清单 ([ac7adac](https://github.com/ZR233/pure-lang/commit/ac7adac77515f92859fe7b1121dedba944a43122))
* 收敛会话与任务状态机设计 ([05f3c83](https://github.com/ZR233/pure-lang/commit/05f3c835a9764de5e2578870e784ca760ff3815f))
* 明确工具来源代际发布与 LSP/MCP 统一工具模型 ([72ea13d](https://github.com/ZR233/pure-lang/commit/72ea13dc47b21c58547e7054cb5a3352880fa9e8))
* 更新 TurnPhase 与 interaction completion 语义 ([1563780](https://github.com/ZR233/pure-lang/commit/15637807aa056ce28597c94736c32c9c28e62d19))
* 精简项目协作与命令规范 ([3b388eb](https://github.com/ZR233/pure-lang/commit/3b388ebc58edded01846328dff6f54dd3dab6448))


### Maintenance

* **repo:** 统一生成文件行尾 ([cb7fb4a](https://github.com/ZR233/pure-lang/commit/cb7fb4aeb32d42c03bca1b04b173c2201af71ca5))
* **repo:** 统一跨平台换行规则 ([93c16b8](https://github.com/ZR233/pure-lang/commit/93c16b835f04f7e9d811a782a89d49c69152578f))
* **studio:** 合并最新远端主线 ([21e4a9f](https://github.com/ZR233/pure-lang/commit/21e4a9fbf4fd867cc7f3eca7d9d58c9263cd7ef5))
* **studio:** 更新 Flutter 依赖锁定 ([7e68f8f](https://github.com/ZR233/pure-lang/commit/7e68f8f791347ce9211b6159aed0d4830319fa61))
* 忽略 pure-studio 构建产物目录 ([7c81f73](https://github.com/ZR233/pure-lang/commit/7c81f7362dd1e31f4f28772616305ec991eecc81))

## [1.0.3](https://github.com/ZR233/pure-lang/compare/v1.0.2...v1.0.3) (2026-07-23)


### Bug Fixes

* **model:** 修复 Responses WebSocket 地址族超时 ([#22](https://github.com/ZR233/pure-lang/issues/22)) ([73e5714](https://github.com/ZR233/pure-lang/commit/73e5714cac3436375d96d1147d673eeee3d38e1d))

## [1.0.2](https://github.com/ZR233/pure-lang/compare/v1.0.1...v1.0.2) (2026-07-23)


### Bug Fixes

* **filesystem:** unify linked path safety ([#20](https://github.com/ZR233/pure-lang/issues/20)) ([7b98543](https://github.com/ZR233/pure-lang/commit/7b98543de0b5c40925fda75898820ef4d64104e1))
* **studio:** skip linked paths in file tools ([#18](https://github.com/ZR233/pure-lang/issues/18)) ([0e33b7a](https://github.com/ZR233/pure-lang/commit/0e33b7af7b1c0cbd35e521ea83a3d5b5932ef842))

## 1.0.1 (2026-07-22)

- First signed Windows x64 release with in-app update support.
