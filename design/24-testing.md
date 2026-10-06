# 24 - 测试与人工验收

## 24.1 自动化责任

项目自有的常规自动化行为测试仅验证 `pl-model` 和 `pl-core` 的公开 API。测试从宿主可调用
接口进入，断言完成结果、状态、效果与明确的错误；不读取私有实现，也不通过测试专用
接口扩大生产 API。例外仅限 `pl-studio-runtime` 的历史保存专项故障测试：使用临时真实
SQLite 与私有、测试条件编译的可控故障点，不为测试扩展生产公共接口。其他 crate、Flutter UI、
发布工作流不保留旧自动化行为测例；格式、
静态分析、构建、生成一致性和发布配置的检查仍独立运行。上游技能随源码携带的测例
保持原样，不参加项目门禁。

`pl-core` 仅使用测试侧的脚本化模型、工具和按锚点查询的临时历史后端，证明 Thread 的执行、
并发、取消、恢复、effect、有界 ChatView 窗口及可选持久化契约，不依赖模型供应商适配。
ChatView 测试覆盖新旧 revision 保存确认竞争（旧确认不得把较新的内容标记为已保存、确认对已
打开窗口立即可见）、缓存/未保存/历史合并无空洞、被合并的多个窗口版本以单个连续 Patch 交付、
多个聚焦位置、慢消费者 Reset、查询失败传播及被淘汰窗口数据的释放。effect window 只按 durable
水位释放，而水位只能越过该 Thread 唯一实时投影 owner 已接管并交回可靠保存通道的提交（见
[15](./15-session-storage.md) §15.3）。该水位由存储后端在自己的 pressure 报告里以 typed
durable receipt 给出，owner 在每次提交后的内存安全边界折入并释放已覆盖批次，core 侧因此不需要
额外的 lease、显式 flush 或独立上报状态。`pl-model` 从单模型运行时与 Thread model
适配公开入口经过本地模拟供应商测试请求、协议流、会话、计量与失败语义，并在适用
场景中贯通真实 core。SQLite 测试使用临时真实数据库，不能由模拟模型代替。

存储暂停用例区分两类收束：队列/字节预算的短暂压力在安全间隙自动恢复；保存真正失败后同一个
Turn 停在准入点，没有显式 `resume_storage(generation)` 就不会开始下一次模型/工具工作，代数过期
被拒绝；恢复后原模型步骤与工具调用不重放。重试保存成功不等于恢复执行。

Studio 专项场景应从真实会话 writer 与数据库边界证明：队列满时返还并保留原批次、取批后退出、
提交后确认前退出的幂等性、并发在途结果、持续故障和代次恢复、checkpoint/blob 失败、
统计消费者失效独立性、关页保留及旧会话库隔离。故障点只控制交错，断言实际历史事务、
查询和进程内状态，不以模拟队列自测代替集成结果。当前专项用例已验证 SQLite 拒写后
水位不前进与原事务重试、真实 writer 的容量拒绝及按代次恢复、事务提交后确认前的原批次
重试、checkpoint/blob 发布失败后的 history fence、任务权威读取、最近 100 条窗口、
关页保留队列、统计消费者停止后不阻塞保存、旧故障代次不能解除新故障及旧会话隔离。
写者确认不再回读正文：专项用例以真实 SQLite 证明 writer 只按本次事务提交的 identity/revision
确认共享会话，投影先发布与保存先落地两种交错都不会把已提交正文降级为未保存预览。
同一组用例还证明可靠交接的前置条件：没有被该 Thread 投影 owner 交回的提交不落库、不推进水位，
交回后才保存，因此"effect 已离开窗口"蕴含"它已被投影并保存"；投影 owner 自己的 durable 屏障
等待的是它的接管票据（已交回的最新提交），新激活 Thread 的全新通道不会让屏障等一个无法移动的
绝对水位。同一组用例固定两处预算事实：可靠字节计量覆盖工具参数/结果/opaque 载荷等全部可变正文，
只有不驻留的附件 blob 除外；投影 owner 自己的绝对驻留字节水位（含报告累积器）计入 Thread 预算，
释放即下调。投影无法完成的事实在没有新的用户可见帧之前也必须让保存屏障带真实原因 fail-closed，
不能挂起到超时。
`pl-core` 公开接口用例验证两个后台工具同时运行、保存拒绝后各自保留结果且恢复时不重复执行；
Studio 专项场景还通过真实 SQLite 写库拒绝验证两个在途工具结果按序写入、恢复后按身份可读；
停止统计消费者和注入消费者 panic 后，历史保存独立完成，统计缺口可见，supervisor 恢复消费。

## 24.2 模拟供应商

同一个本地服务支持自动化和 GUI 人工验收：仅监听 loopback 动态端口，按固定提示词、
协议及期望轮次返回固定 HTTP/SSE 或 WebSocket 事件流。它验证实际请求的重要字段；
未登记的请求、错序、多余调用和错误协议明确失败，不回退到成功文本，也不访问真实
供应商。OpenAI 已实现的 Responses HTTP/WS 与兼容 Chat 使用共享协议样本；DeepSeek
和智谱只为原生选项、媒体、上传、搜索方言及使用量差异添加独立样本。DeepSeek 原生独立搜索
（Anthropic 兼容 Messages）不进入会话 transport，由独立的 loopback HTTP 样本证明其请求、
鉴权与解析。

模拟只证明当前实现与约定样本的行为，不证明供应商线上接口的最新兼容性。真实接口
保持显式人工观察，采集证据后由人判断，不参与确定性提交门禁。

worktree 协作写入的真实 API 观察使用 `collaboration_observe` 的隔离 Git 项目与根会话
worktree 模式，经正常 Studio 创建命令派发 directory/worktree 执行者。观察者核对工具
调用终态、实际 checkout、文件内容、目录越界拒绝及进程回收；不修改真实项目或凭据，
不以模型最终文字或采集器零退出码替代功能判断。

Plan 恢复使用显式 `plan-recovery` 原生 GUI 人工场景：在同一隔离数据根中提交真实计划
确认、正常关闭并等待进程回收，再重启并打开原会话。分别观察批准与修改路径，比较恢复
前后的计划正文与交互身份，并核对回答前无新增模型请求、回答后无重复 continuation、
已回答计划不重新弹出。场景保存截图、快照、请求和生命周期日志，人工结论独立于脚本
退出状态记录；不以直接写入数据库或伪造 UI 状态代替真实 `plan_submit`。

应用关闭使用显式 `shutdown` 原生 GUI 人工场景（当前为 Linux 原生）。场景复用正常 bridge、
隔离 `ANYWORK_HOME`、Flutter Driver 与进程所有权，按 phase 保存互不覆盖的证据，并由
`coverage.json` 逐项记录已执行与未覆盖的验收要求，未覆盖分支不得写成通过。phase 至少覆盖：
真实保存后正常退出并等待 OS 进程回收、进程退出码 0；同一隔离数据根重开并核对原会话与已提交
历史；在同一数据根启动第二实例记录实例锁拒绝（真实 widget 文案“已有实例在运行”加日志、
无 `ArgumentError`、有 bridge 错误关联），真正关闭第二实例（以真实 OS exit 证明）后第一实例
仍能在同一 Thread 继续提交并保存，再重开该新 Turn；重复退出请求不重置 30 秒总期限（首次
arm 后延迟数秒再重复同一 close/request，总耗时从首次 arm 起算并在 30 秒内结束；重复请求由
Driver-only `request-app-exit-twice` 在同一 shared future 上再次 `beginExit`，其回报的剩余只
减不增即证明首次期限未被刷新；该 phase 复用既有 Driver-only 订阅故障入口
`shutdown_fault_driver.dart`——其 progress 订阅报错、cancel 挂起直到协调器的有界 2 秒截止，
使唯一一次中央清理真实跨过两次请求之间约 500ms 的间隔，第二次请求确实送达同一预算；以
Driver-only `exit-status` 的 `nativeExit.requests` 只读投影轮询到 `requests>=2` 并作为硬判定
（缺读即失败，不得当通过），同时按该故障真实要求断言本 phase 的实际退出码为 Degraded 的 1、
并记录 typed report；正常 exit 0 由 normal/reopen/busy 独立覆盖）；Driver 挂起时由 native 在
30 秒到期强制退出码 1
并保留 PID/阶段日志；runtime 真实初始化失败（隔离数据根不可创建）时渲染真实致命 widget 并
仍能关闭留日志；持久化拒写/SQLite 锁故障复用既有 history-fault/history-lock 机制，强制退出
后重开核对已提交记录、`PRAGMA quick_check` 与可打开的数据库；外部模拟服务被真实冻结（不响应）
时不阻塞退出。退出判定以 `os process wait` 与实际退出码为准：Driver 请求返回或 VM 断开只表示
“已请求退出”，超时只用于防止永久挂起，不作为成功判据。
严格 fixture 的完成校验以“所有 required 脚本步骤都被消费”为准（服务端 `remaining_optional` 为真
且无拒绝请求，否则以非零退出结束），不得用“已接受请求数达到下限”的计数代理掩盖未消费的必需步骤
（如并发 Turn 的 receipt continuation）。

原生退出必须由真实 OS 进程证据确认，而不是 `cargo`/Flutter launcher 的退出码。平台 launch tool
对 app 退出统一上报 `0`（`resident_runner.dart` 的 `appFinished`/`_serviceDisconnected` 都以 `0`
完成），因此 `run-gui --driver` 增加 Driver-only 的 `--native-launch`：它构建 debug/profile
Driver bundle 并直接启动原生 artifact，使验收入口成为其直接父进程，`--native-exit-report`
写出结构化退出报告（真实 `waitpid` 状态、原生 PID 与未被产品自身回收的孙进程 PID 集合），
并通过 `FLUTTER_ENGINE_SWITCH_*` 保持 VM service 可用。验收侧再用 Driver `pid` 请求取得原生 PID，
自读 `/proc/<pid>/stat` 的 starttime 作为 PID 复用防护并轮询其真实退出；规范化诊断从
`ANYWORK_HOME/studio/logs` 复制进证据，核对 PID/阶段/耗时/错误码/correlation/持久化事实/未回收
owner。pl-dev-support 的 resident 运行新增结构化退出报告（`run_resident_reporting`），
`cargo xtask run-gui --driver --native-launch --native-exit-report <PATH>` 是对外接口。

MCP/工具子树回收由场景自有的 stdio MCP 服务证明：隔离数据根的 `[mcp.servers.shutdown-fixture]`
指向 `pl-provider-fixture --mcp-stdio`，该服务完成真实 `initialize`/`tools/list` 握手并 `spawn`
一个忽略 `SIGTERM`、独立进程组的孙进程，把自己与孙进程的 pid/starttime 记入协调文件。真实
turn 使线程取得 MCP lease 后产品以自身监督 worker 启动它。宿主先解析真实原生 PID 与该协调文件里的
业务 pid/starttime，实证两个业务 PID 在**任何关闭请求之前**确实存活，并预先启动独立监测线程，随后
才通过私有信号文件放行 Driver 请求；Driver 侧用既有 `shutdown-hang` 武装单一 30 秒期限并阻塞
isolate，由原生宿主在 30 秒到期强制退出码 1（首次 deadline 固定，实测 delta ≤1 秒）。强制退出后
由产品经 control-EOF 先 TERM/KILL 回收整棵子树，验收轮询每个业务 PID 真实消失，再以
`--native-exit-report` 的空 `reclaimedDescendants` 证明产品先于 harness 完成回收（harness 的强杀
只作最后兜底并单独记录），不使用全局 `pkill`、宽泛 PID 扫描或伪造计数。`run_resident_reporting`
在 root 被 reap 后先给产品自身 supervisor 一个有界自然回收窗口，再升级自身 TERM/KILL，因此严格
判定不依赖 harness 强杀；普通 `run_resident` 的默认清理语义不变。

LSP 子树回收由同一个 fixture bin 以 `--lsp-stdio` 模式证明：隔离数据根 `[lsp.servers.shutdown-lsp-fixture]`
指向它（`language_ids` 用非内置的唯一 id，避免与内置 rust-analyzer 冲突），真实 `lsp_query`
工具 turn 让 runtime 经自身监督 worker 启动该 server；server 完成真实
`initialize`/`initialized`/`shutdown`/`exit` 握手并 `spawn` 同样忽略 `SIGTERM`、独立进程组的孙进程，
记录自身与孙进程 pid/starttime；顺序、强制退出方式（阻塞 isolate → 原生 30 秒退出码 1）与判据均与
MCP 相同：先实证业务 PID 存活并启动监测，再放行 Driver 强制退出，最后核对业务 PID 全部消失且
产品先于 harness 回收整棵子树。
工具子树回收由真实后台 `exec` turn 证明：脚本命令运行同一 fixture bin 的 `--tool-peer`，它同样
`spawn` 忽略 `SIGTERM` 的独立进程组孙进程并记录 pid；命令刻意长于前台窗口，使 Turn 停留在后台
任务而工具子树仍然存活；与 MCP 相同的“先实证业务 PID 存活并启动监测、再放行 Driver 阻塞并等原生
30 秒强制退出码 1”顺序保证后，产品须回收整棵子树。
资源并发关闭由 `concurrent-stop` phase 证明：隔离数据根同时声明场景自有 stdio MCP 服务，并由脚本
让单个后台 `exec` turn 启动 `--tool-peer`，因此一个 Turn 同时启动两个独立监督资源（MCP 经线程服务
租约、工具经真实工具 worker）。宿主等到两个 peer 各自写入 pid/starttime 后，先实证两组业务 PID
（各自进程及其忽略 `SIGTERM` 的孙进程）都存活，再等严格 fixture 的实时计数器确认该 Turn 必需的
receipt continuation 已被真正接受（`remaining_optional=true` 且无拒绝请求，此时 Turn 仍处于其
paced 流式窗口），才获取该数据根的 SQLite 排他写锁并放行 Driver 的真实退出请求；只看到已启动的
peer 就放行会让 Turn 在 receipt 处被取消、脚本缺步却被“已接受请求数”掩盖，因此 continuation 的接受
事实先于写锁与关闭。持锁期间该 Turn 的末期保存无法提交，是有意的“人为阻塞的独立关闭链”。判定不看
总耗时，而是看真实 OS pid 时序：两组业务 PID 必须在原生进程真正退出之前、且在写锁
仍持有的窗口内由产品自身回收；每棵资源的完成时刻取其进程与忽略 `SIGTERM` 的孙进程两者停止时刻的
较晚者（逐 PID 记录自退出请求前的独立监测时钟与原始时序，且都显著早于单一 30 秒期限），任一 PID
未被观测到即不能确认完整回收、phase 失败；宿主强杀集合为空、`reclaimedDescendants` 为空。被阻塞的
保存链使关闭在 28 秒清理预算处协调降级并以 `finishExit(1)` 立即结束：原生硬期限 30 秒只是上界，
30 秒 watchdog 仅用于 Dart/桥/引擎无响应，故判据是 exit1、无宿主强杀、耗时不超过 30 秒加既有测量
容差且确实到达 28 秒清理预算附近，而非要求恰好 30 秒。原生 canonical 诊断
`anywork-exit-diagnostics.log` 必须证明这是一次非 Clean、`pending` 未知或仍待写的终态
（`cleanExit` 或 `pending=0` 即判锁未生效），并区分 28 秒协调降级完成（`finish`）与 30 秒 watchdog
强退（`final` + `exitDeadlineImminent`）；Driver 挂起专用 phase 保留其 30 秒原生强退的严格判据。
重开该数据根后 `PRAGMA quick_check` 为 `ok`、实例锁时序保持。任一资源只能随原生退出一起消失，即为
串行停止被误记为并发，phase 失败而非记为通过。第二实例的原生退出改用真实窗口管理器协议：
第二实例被钉在场景自有 `Xvfb` 上（`DISPLAY` 指向它，并强制 `GDK_BACKEND=x11`、移除继承的
`WAYLAND_DISPLAY`，避免 GTK 改走 Wayland 而在该 display 上没有窗口），随后以 ctypes/libX11
发送 `WM_DELETE_WINDOW`：工程脚本读取第二实例真实原生 PID（Driver `pid` 请求写入的身份文件）
并优先使用该主进程 `/proc/<pid>/environ` 中的真实 `DISPLAY`，只向 `_NET_WM_PID` 等于该 PID 且
声明 `WM_DELETE_WINDOW` 的窗口投递，读取的 `_NET_WM_PID` 与 launcher 记录一致并以 starttime
防护，配合 `--native-exit-report` 的退出码 0 判定；失败时脚本把候选窗口（pid/协议/名称/几何）
与所用 display、原因写入 `shutdown-busy-second-x11.json`。第一实例随后仍在同一 Thread 继续提交
保存；Flutter Driver 无法覆盖窗口管理器协议，故该步由工程脚本触发，不影响用户桌面。

init 中关闭、bridge 不可用与订阅故障由 Driver-only 注入执行：`ANYWORK_DRIVER_SHUTDOWN_FAULT`
取值 `pending-init`（初始化永久挂起，关闭须报 Degraded + Unknown 且强制退出 1）或
`bridge-load-error`（初始化显式失败、无 runtime owner，关闭须报 NotStarted 且退出 0）；二者
复用既有 `FrbStudioApi.debugOverrideInitialization`，不新增生产 fault API，证据中明确标注这是
Driver-only 注入而非真实 `dlopen` 失败。订阅故障由专用 Driver 入口（`run-gui --native-launch
--native-launch-driver-target test_driver/shutdown_fault_driver.dart`）在首帧后把同一
`StudioExitCoordinator` 安装在只覆写 `subscribeShutdownProgress` 的真实 `FrbStudioApi` 子类上：
进度流错误与取消挂起必须成为 typed `progress` diagnostic 并使退出为非零，绝不伪 `Stopped`。
`ANYWORK_HOME/studio` 不可写时（普通文件占位）`recordDartError` 须回落到隔离 `TMPDIR`，证据
复制并机器校验 stage/correlation/stack（Dart logger 无 pid 字段，PID 取场景记录的 OS PID）。
人工验收结论独立于脚本退出状态，默认待评审；模拟供应商、Linux 单平台结果不外推为真实供应商
兼容或跨平台通过；Windows 以 `notrun` 记录，不是 Linux 缺口。

现有 `pl-model/tests/provider_wire.rs` 从公开入口核对 Responses HTTP/WS、Chat、原生
OpenAI cache 选项、文本与推理流、function/custom/programmatic 工具、托管搜索、图片、
DeepSeek 上传与搜索方言、智谱 Chat 与 Coding Plan、远程压缩、使用量/价格、错误与取消、
重试及 `pl-core` Thread 装配。`pl-core/tests/` 核对模型工具轮次、取消、checkpoint
恢复及 SQLite 持久化。独立的 Studio `/alpha/search` 编排、GUI 操作、非图片媒体、
真实供应商后端和其他 crate 的权限/进程/迁移不在这两库的模拟自动测试证明范围内；
增减协议能力时应同步维护场景和缺口。

`pl-model/tests/deepseek_search.rs` 通过公开的 `pl_model::provider::deepseek::search` 域和真实
loopback HTTP 服务核对 DeepSeek 原生独立搜索：native Messages 请求形状与 `x-api-key` /
`anthropic-version` 鉴权、base_url path 前缀保真与 `/v1` 别名归一化、非法 base_url 拒绝、
`text.citations` 与 `web_search_tool_result` 的 URL 首现去重与片段合并、合法空结果、
`web_search_tool_result_error`、缺失结果块、非 JSON / 畸形 JSON / 非 UTF-8 正文的无损留存与
有界 JSON 错误、重定向不被跟随、取消与
整体超时，以及缺凭据 / 非法参数 / 空 query 时不发出请求；能力门控用例核对 canonical preset
声明、非 canonical PresetDefaults 撤销与 Explicit opt-in。完整能力用例
`deepseek_search_uses_canonical_provider_and_native_sources` 消费 canonical DeepSeek preset 的
standalone capability 并对临时 HTTP 环境执行 native 请求。确定性样本只证明客户端请求与解析
行为，不证明供应商线上接口的最新兼容性；真实供应商保持显式人工观察。

完整回放用例通过无数据库的真实 Thread 和模拟供应商验证 commentary、final、原生 phase、
工具原文、HTTP/WS/Chat 前缀以及 checkpoint 恢复；摘要和原生压缩必须消费相同材料。
存储专项用例验证冻结回放的事务保存、失败重试及恢复，不用写入后的展示状态补造历史。

`context-replay-recovery` 原生 GUI 场景在同一隔离数据根中完成工具循环与后续输入，等待
保存后正常退出，再用全新程序打开会话并续接，最后再次重开。分别覆盖 Responses HTTP、
WebSocket 和 Chat，核对实际请求、消息身份与顺序、无重复工具、首次完整请求及之后增量；
另覆盖失败或取消后重开，确认候选观察没有混入模型历史。采集截图、快照、保存水位和
进程回收证据，人工判定独立记录；模拟 usage 不证明真实缓存命中率。

Responses WebSocket 的确定性恢复验证从两库公开入口贯通实际网络与 Thread 装配：模拟
供应商提供可控断开、关闭码、错误事件、响应挂起及握手拒绝，不自行决定客户端恢复。
场景验证完整的逻辑请求预算、HTTP 支持约束与粘性回退、完整输入重放、continuation 隔离、
已报告 usage/远端副作用保护，以及取消与收包任务收束。idle 和退避使用受控时间与明确同步
点，避免数分钟真实等待或概率性交错。

`cargo xtask manual-gui --scenario websocket-recovery` 使用隔离配置的真实 Windows bridge 和
Flutter Driver，模型明确选择 Responses WebSocket。人工证据包含失败展示片段的独立终态、
同一轮次的重试与恢复提示、退避期间停止、下一轮正常执行及历史重开；请求记录核对冻结输入、
取消后无额外恢复请求与失败观察未进入后续模型输入。脚本检查和人工截图结论分别记录，未观察
的证据保持待评审，不以普通 HTTP 场景或演示 UI 代替 WebSocket 验收。
这些证据只证明实际运行宿主及脚本协议，不等同线上供应商或跨平台通过。

HTTP 生命周期公开 API 回归使用真实分片 TCP/SSE 和受控时钟，验证持续读取超过 300 秒、
大事件尚未解码时按字节读取更新空闲时限、真正空闲超时、超限事件、错误响应读取上限和
关闭主动取消。附件准备与推理共享预算，成功上传只使用一次；已观察 usage 和托管工具
副作用阻止不安全重放。压缩准备失败的结构化错误及计量通过 Core Turn 持久化，旧 v2
记录缺少新增可选字段时保留原描述；重开不重新执行推理。

`cargo xtask manual-gui --scenario call-lifecycle-recovery` 通过原生 bridge、工具发现和
Flutter Driver 验证工具目录刷新失败保留有效工具、历史仍可打开、压缩耗尽后继续、真实
`wait` 期间重试目录与停止、下一轮以及历史重开。显式目录重试不依赖配置 revision 改变。
准备失败前未受理的输入保持排队，恢复后与新输入只受理一次；失败压缩不提前提交输入。
取消后的 31 秒和重开后的静默窗口核对无多余请求，保存上下文与历史身份精确比较。
真实会话恢复另使用隔离数据库副本验证；损坏副本仍必须拒绝，不能清空历史换取继续。

部分文本、已完成普通思考/文本 item 和本地工具参数之后的中断，必须证明仅成功子尝试
推进上下文且工具只执行一次。观察回归覆盖服务端复用 item id、慢消费者跳过中间 progress、
失败观察与恢复提示在最终回执中保留；恢复展示不能污染成功模型输入。已有正常 WS 复用与
HTTP 失败样本继续复用，WS/HTTP 失败终态对相同 usage 与错误样本语义一致。

## 24.3 GUI 人工证据

GUI 验收编排由独立工程工具 `pl-studio-acceptance` 拥有；xtask 只在选择验收命令时
按需编译并启动该工具，原样透传参数，不维护场景或选项的第二份定义。验收工具直接使用
Studio canonical 配置及现有模拟供应商场景，数据库锁与证据检查不进入普通 xtask 依赖图。
两工具共用 `pl-dev-support` 的路径及进程支持；迁移不得改变交互输入、退出错误传播、
取消或进程树回收语义。验收工具使用正常 workspace 构建缓存并尊重显式目标目录配置，
不继承 xtask 入口专用的构建目录参数。场景仍保持全部现有能力，不通过删场景精简依赖。

GUI 通过非 demo 原生 bridge 连接隔离的 Studio home 与本地模拟服务。人工流程记录
用户操作、请求、可见截图、运行时快照与日志；启动、退出、故障和取消均须回收子进程。
当前固定场景需要发送 `Reply with exactly: fixture ready`；终端按操作输入动作代码，
输入 `done` 后关闭并保存证据。未发送固定提示词时记录为待评审，不视为场景通过。
脚本只能报告采集状态，人工结论单独记为通过、不通过或待评审，默认待评审。
Windows 与 Linux 的验收启动均使用同一平台命令解析规则。窗口布局验收只调整已核实属于
隔离 GUI 进程的唯一可见主窗口，不激活其他窗口；成功、失败与中途退出均恢复原始尺寸。
脚本点击以目标实际可命中且位置稳定为前置条件，不能把控件已构建当作对话框遮罩已退场；
输入命令在点击后的焦点更新完成后执行，超时后不继续派发迟到点击。
需要真实供应商时直接使用用户已有配置启动原生 GUI，不覆盖或复制用户凭据。

统计场景复用隔离的本地虚拟供应商与真实 Bridge，通过 Flutter Driver 提交固定的快速
响应和定速流式响应，并从设置页检查已记录的成功调用与可计算的性能汇总。在隔离的
`calls.sqlite` 上短暂持有写锁，检查写入未完成时的状态及释放后的自动刷新；同一隔离
home 重启后再次读取。取证保存设置页截图、快照、调用库计数和脱敏日志，结论仍由
人工填写。统计读写缺口不得误判为没有调用；不把模拟服务商验收外推为线上供应商兼容。

压力场景在 Flutter profile/AOT 模式及优化 Rust bridge 下按固定提示词从本地供应商以约 5,000 token/s 返回约 20,000 个混合消息事件；
采集请求耗时、事件完成数、GUI 帧/操作探针、窗口大小、截图、快照、未处理错误和进程回收；
当前不记录 GUI 进程 RSS，不据此宣称已验证内存峰值。
事件数、独立 `item_id` 数与实际 token 数须分别记录：同三个条目上的两万次 delta
只能检验持续流和长正文，不能代替约两万条不同信息的窗口淘汰、深度分页与 GUI 压测。
压力入口还要在同一隔离项目中通过 Flutter Driver 创建 16 个独立会话，给第一个新会话
写入长正文，并重新打开原始长历史会话；逐会话核对目录身份、窗口上限、保存完成及
模型请求的严格匹配。长历史、长正文、长 Turn 和多会话分别留存证据，不以单次流速
代替四者的性能证据。场景脚本完成与人工验收结论分开：未观察到屏幕更新、卡死、未退出
的子进程或未核查截图时，不得判为 GUI 压测通过。

GUI 人工验收不能代替已移除的 Studio、工具、远端进程、HTTP/FRB 和发布流程自动回归；
交付报告须明确这些未受新门禁保护的风险。

滚动验收使用真实拖动与滚轮信号，核对持续输出时自动跟随、小幅上翻即脱离、正文增长后
锚点及偏移不变、手动回到最新和显式跳最新后继续跟随，以及再次拖动不被贴底打断。
长正文场景同时保留会话切换后的阅读位置证据；不足一屏的折叠历史需另核对最早消息可达、
双向分页到端停止及窗口有界。滚动命令执行成功不能代替可见几何、截图与窗口身份的判定。
历史往返还需覆盖连续小幅滚轮跨页后回到最新，不能被空闲恢复反复拉回旧锚点。
分页后的边缘状态：布局改变把位置钳位到末端时，界面先保留手动阅读；
随后向下滚轮应恢复最新窗口与跟随。分别观察仍有滚动范围和整窗内容不足一屏的情况。

工具历史的人工观察覆盖短结果、跨屏结果、超长参数及多个连续工具；通过真实工具执行建立
会话，再在历史中展开、收起和滚动。判定同时观察外层阅读锚点、工具标题的屏幕位置与内部
结果滚动位置，避免仅凭正文存在或操作未报错推断滚动正确。

条目布局协调的人工观察覆盖图片摘要、工具组、工具详情、推理组与计划入口，在最新端、
历史中及不足一屏窗口分别展开/收起，记录操作入口位置、可见锚点与 canonical 窗口身份。
查看当前条目只暂停贴底，不产生连续旧页请求或跳到最早消息；随后真实外层滚动仍可翻页，
返回最新后继续跟随。异步正文/图片到达保持原阅读意图，快速连续操作、滚动接管与会话
切换取消旧校正；工具内部滚轮不改外层锚点。图片另核对数量/状态文案、按需读取、圆角
小缩略图、多图换行、不同调用同资源身份、重试、放大/缩放/关闭、行回收重建与历史重开。
现有 fixture 未覆盖的入口通过同一隔离 harness 的定向 Driver 操作补充，无法动态覆盖时
明确静态证据和剩余缺口，不把场景命令退出成功等同于交互全部通过。

realtime 场景同时观察有效模型绑定在调用中及全部调用结束后保持可用，覆盖供应商错误、
取消和继续调用，防止持久化检查点中的非持久模型状态覆盖实时路由。真实供应商的同类
问题另记录成功响应、调用前后路由状态与 GUI 截图，不能以模拟供应商通过代替真实 API 验证。

在线模型目录的 pl-model 公开 API 证明覆盖查询地址/鉴权、两种 typed envelope、缺失与显式空
参数语义、富声明驱动实际请求、完整名单替换以及 provider 内精确 ID 的价格关联；默认文件只
验证声明与引用规则，不镜像生产名单。网络错误、限额、取消和条件响应必须有实际终态。
Studio 人工场景另观察每实例启动一次、并发一家失败另一家成功、默认回退、同身份成功缓存的
离线重启与 cached-only Profile、损坏缓存不全局重置、迟到结果/事件拒绝、普通设置保存不泄漏
发现模型、选中模型撤下的 unavailable 及在途请求冻结。取证保存模拟服务请求、成功缓存文件、
配置前后、typed 快照、原生截图和进程回收；不新增 Studio 常规自动测例或将启动探针等同完整验收。
隔离 harness 可注入显式 loopback 模型目录服务，为 OpenAI 两个独立实例与 DeepSeek 实例
提供可控声明；推理仍使用原有严格 fixture。目录故障、空名单和刷新不得改变推理 fixture
或用户配置，人工观察与其他 GUI 场景采用相同的取证及进程回收边界。

## 存储职责重构验收

长任务必须证明常驻状态不保留历史请求输入，最新上下文增量写入而不反复序列化历史前缀。
同一确定性回归先红后绿；补充记录内存、数据库/WAL、日志大小和写入量。覆盖压缩后的
工具身份去重、最新请求重试、重启不重跑工具、历史与 checkpoint 原子提交、确认丢失、
迁移中断重试及日志时间/容量淘汰。日志故障或清理不能改变累计用量和会话恢复结果。
原生 GUI 人工验收覆盖历史回看、重开、压缩后继续及统计展示，并明确实际宿主平台。

存储压缩人工场景通过真实 bridge 和模拟供应商连续完成两次自动压缩，检查原始用户输入与
两次回复仍可完整回看，当前上下文与压缩回执只保留最新有效集合。该场景与计划跨重启恢复、
统计重启场景组合验证保存边界，不将模拟结果外推为真实供应商兼容性。
