# 24 - 测试与人工验收

## 24.1 自动化责任

Rust 常规自动化行为测试验证 `pl-model` 和 `pl-core` 的公开 API，从宿主可调用接口进入，
断言完成结果、状态、效果与明确错误；不读取私有实现，也不通过测试专用接口扩大生产 API。
`pl-studio-runtime` 的历史保存专项故障测试使用临时真实 SQLite 与私有、测试条件编译的可控
故障点，同样不扩大生产公共接口。其他 Rust crate 和发布工作流不新增常规行为测试门禁；
格式、静态分析、构建、生成一致性和发布配置检查独立运行。上游技能自带测例保持原样，不参加
项目门禁。

前端新增两类正式自动门禁：SDK-only `anywork_client` 的公开 Client 契约测试，以及应用
`test/client` 中的 Client/FRB adapter/Riverpod 适配契约测试。它们验证资源、命令、草稿与观察
边界，包括注入受控 transport 与宿主通道后的实际应用路由；受控路由测试不能替代真实宿主的
Driver 旅程、native FRB 或平台 UI 验收。其他 Flutter widget/integration
测试与 Driver 场景不因本次重构加入自动门禁。Client 所有权与接口见
[25](./25-frontend-state.md)，内容传输见 [07](./07-streaming.md)。

Client 的纯 reducer 与查询回归放在 SDK-only 测试集，AppGraph 与 Riverpod 的 canonical
事实收敛放在应用 adapter 测试集；不在 Flutter 应用侧重复维护这些纯 Client 契约，
也不以仅供旧测例调用的全局状态辅助入口代替当前 Client 与 adapter。

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

Turn 准入 gate 的公开 API 用例证明它与输入队列暂停相互独立：gate 关闭后下一条 Turn 停在准入点，
输入仍 pending、没有 Turn 记录也没有模型请求，等待期间 flush 仍收束；多个 hold 的原因相互独立，
释放旧原因不重开新 hold；已准入 Turn 不受新 hold 影响，只有下一条 Turn 等待；等待期间的 close
与取消给出确定性结果；从 checkpoint 恢复并续跑的 Turn 同样先经过装配期安装的 gate。

Studio 专项场景应从真实会话 writer 与数据库边界证明：队列满时返还并保留原批次、取批后退出、
提交后确认前退出的幂等性、并发在途结果、持续故障和代次恢复、checkpoint/blob 失败、
统计消费者失效独立性、关页保留及旧会话库隔离。故障点只控制交错，断言实际历史事务、
查询和进程内状态，不以模拟队列自测代替集成结果。当前专项用例已验证 SQLite 拒写后
水位不前进与原事务重试、真实 writer 的容量拒绝及按代次恢复、事务提交后确认前的原批次
重试、checkpoint/blob 发布失败后的 history fence、任务权威读取、最近 100 条窗口、
关页保留队列、统计消费者停止后不阻塞保存、旧故障代次不能解除新故障及旧会话隔离。
写者确认不再回读正文：专项用例以真实 SQLite 证明 writer 只按本次事务提交的 identity/revision
确认共享会话，保存确认不构造第二份正文投影，也不会把已提交正文降级为未保存预览。
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

### 24.1.1 Client 五层证明与门禁

不同层级分别交付证据，不以底层 fake、静态扫描或一次 UI 截图代替其他层：

| 层级 | 被测边界 | 应证明的行为 | 证据与门禁范围 |
| --- | --- | --- | --- |
| 纯 Dart Client | 公共 Client API 与可控 transport | 唯一 reducer、attempt 所有权、receipt/观察顺序、草稿冻结、预算、原子观察与幂等 close | `anywork_client` 测试进入正式门禁；不加载 Flutter/native |
| 应用 adapter | FRB DTO/句柄适配、Riverpod 只读观察、AppGraph 注入 | 首 Reset、Patch 缺口、typed control receipt、晚到句柄、取消/关闭、双实例隔离、provider 只取消自己的观察 | 仅应用 `test/client` 进入正式门禁；使用 Flutter runner，因为应用依赖 `dart:ui`；fake 不能记为真实 native 通过 |
| 真实宿主 Flutter 路由 | 启动、Settings/标签/返回、Connector/View | 当前 ChatView 与草稿身份保留、提交期间继续输入、旧 intent 不覆盖新值 | 通过 Driver 与人工旅程取证；受控宿主的自动路由契约测试归应用 adapter 层 |
| FRB/native | 实际 Rust 动态库、sink、句柄与退出 | 真实 open→initial→next、控制回执、单一内容出口、关闭唤醒、join 与进程回收 | 原生 fixture 与 typed 诊断单独记录；纯 Dart fake 不覆盖此层 |
| 平台与可见交互 | 实际宿主窗口、滚动/菜单/输入及退出 | Timeline 锚点、键盘/ESC/Tab、流式逐帧观感、原生资源收束 | 人工验收按 Linux/Windows 分开；未运行的平台标记未验证 |

`cargo xtask verify-client` 是 CI 的正式门禁，与 Rust 检查、`verify-gui` 和生成一致性各负其责。
它锁定解析两份依赖快照，检查 Client SDK-only 依赖、import 与 provider 生命周期边界，运行
Client 格式/静态检查和纯 Dart tests，再以 `flutter test --no-pub` 执行应用 `test/client`。
缺少 package、lockfile 或任一必需测试集必须失败，不能跳过后记为通过；统一入口与工作目录
约束由 [AGENTS.md](../AGENTS.md) 和仓库工具维护，不从 SDK 安装目录运行项目命令。

Client 自动用例使用屏障、可控 Future 和 fake clock 组织交错，不延长 sleep 等概率碰撞。
必须覆盖以下缺陷敏感的行为；交付记录区分用例已存在、已执行与实际通过：

- 草稿经 Settings/多标签往返保留身份和值；revision 8 提交后继续编辑到 9，旧 ACK 不清新文本、
  route 或附件，只处理该 `DraftStamp + OperationId` 冻结的所有权。
- A→B→A 选择时旧 open 晚到句柄由原 attempt 关闭；句柄已创建但首 Reset 失败仍须关闭并可
  retry；在 open、initial、next、control 与 close 的各个 await 交错退出，无死锁或预算提前释放。
- 新 view 首 Reset 使用自己的版本域，旧 `lastGood` 只显示；Patch `from` 不匹配仅重同步当前
  view，`to <= from` 拒收；短窗补页、load/extend 与 `NoChange` 不形成自动循环。
- 回执先到、帧先到，以及 Applied 帧已观察且回执版本相等，均按 operation 与 effect point
  结算；版本相等不能推断 `NoChange`。快速 X→Y 中 X 的晚结果不清 Y 的 overlay。
- `UnknownOutcome`、`AwaitingObservation`、`SyncFailed` 与 `Rejected` 分开；未知非幂等提交
  不自动重放，已受理但未 durable 的附件仍由明确 owner 保留。
- 两个 Client/AppGraph 不共享业务 Map、句柄或草稿；读取快照与注册观察之间插入更新不漏帧；
  观察者暂停只保留最新显示快照，有序可靠事件不被压成 latest-only。
- 重复退出共用首次 deadline；opening/active/retiring/unresolved 均计费，关闭失败保留
  unresolved 报告，不能以清空 Map 冒充句柄和任务已回收。

Rust 可靠历史、SQLite 故障与 writer 的已有公开 API/专项测试继续复用，不因 Client fake
通过而重复声明已覆盖 native 保存。界面几何、真实系统剪贴板、真实供应商和跨平台行为仍按
后续人工验收取证。

## 24.2 模拟供应商

同一个本地服务支持自动化和 GUI 人工验收：仅监听 loopback 动态端口，按固定提示词、
协议及期望轮次返回固定 HTTP/SSE 或 WebSocket 事件流。它验证实际请求的重要字段；
未登记的请求、错序、多余调用和错误协议明确失败，不回退到成功文本，也不访问真实
供应商。OpenAI 已实现的 Responses HTTP/WS 与兼容 Chat 使用共享协议样本；DeepSeek
和智谱只为原生选项、媒体、上传、搜索方言及使用量差异添加独立样本。DeepSeek Search & Fetch
工具接口（`/v1/tools/search` 与 `/v1/tools/fetch`）不进入会话 transport，由独立端点样本证明
其请求、鉴权与解析，并供 GUI 人工验收观察 `queries` 扇出与抓取。

模拟只证明当前实现与约定样本的行为，不证明供应商线上接口的最新兼容性。真实接口
保持显式人工观察，采集证据后由人判断，不参与确定性提交门禁。

worktree 协作写入的真实 API 观察使用 `collaboration_observe` 的隔离 Git 项目与根会话
worktree 模式，经正常 Studio 创建命令派发 directory/worktree 执行者。观察者核对工具
调用终态、实际 checkout、文件内容、目录越界拒绝及进程回收；不修改真实项目或凭据，
不以模型最终文字或采集器零退出码替代功能判断。

Plan 恢复使用显式 `plan-recovery` 原生 GUI 人工场景：在同一隔离数据根中提交真实计划
确认、正常关闭并等待进程回收，再重启并打开原会话。分别观察批准与修改路径，比较恢复
前后的计划正文与交互身份，并核对回答前无新增模型请求、回答后无重复 continuation、
已回答计划不重新弹出。批准路径还须在 Interaction resolved 后通过状态栏「更多」菜单的计划
入口打开同一份当前计划文档，重启并打开原会话后仍可读到同一正文，且面板宽度可在会话可用
宽度一半以内调整；未批准、无文档或 restart 清空后不得出现该入口。场景保存截图、快照、
请求和生命周期日志，人工结论独立于脚本退出状态记录；不以直接写入数据库或伪造 UI 状态
代替真实 `plan_submit`。

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
复用既有 `FrbStudioTransport.debugOverrideInitialization`，不新增生产 fault API，证据中明确标注这是
Driver-only 注入而非真实 `dlopen` 失败。订阅故障由专用 Driver 入口（`run-gui --native-launch
--native-launch-driver-target test_driver/shutdown_fault_driver.dart`）在首帧后把同一
`StudioExitCoordinator` 安装在只覆写 `subscribeShutdownProgress` 的真实 `FrbStudioTransport` 子类上：
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

`pl-model/tests/deepseek_tools.rs` 通过公开的 `pl_model::provider::deepseek::tools` 域和真实
loopback HTTP 服务核对 DeepSeek Search & Fetch 工具接口：两个端点的 base_url path 前缀保真、
`/v1` 别名归一化与 `/anthropic` 结尾的明确拒绝、非法 base_url 拒绝、`Authorization: Bearer`
鉴权且不再发送 `x-api-key` / `anthropic-version`、请求体只含 `query`/`count`/`need_content`
或 `url`；`results` 的 URL 首现去重、`date` 到 `published_at` 映射、空串归一为缺失、不消费
`summary` 等未使用字段、合法空结果成功、`MalformedResults`、fetch 正文成功与 `MissingContent`；
非 JSON / 畸形 JSON / 非 UTF-8 正文的无损留存、`error.message` 有界截断、重定向不被跟随、
取消与整体超时，以及缺凭据 / 非法参数 / 空 query / 超长 query / 超范围 count / 非法 fetch url
时不发出请求；能力门控用例核对 canonical preset 声明 `DeepSeekToolsApi`、非 canonical
PresetDefaults 撤销与 Explicit opt-in。完整能力用例消费 canonical DeepSeek preset 的 standalone
capability 并对临时 HTTP 环境执行 search 与 fetch。确定性样本只证明客户端请求与解析行为，
不证明供应商线上接口的最新兼容性；真实供应商保持显式人工观察。

`pl-tool` 的 `queries` 扇出、并发中止、位次合并、URL 去重与来源上限不在上述测试范围内；
该 crate 不新增自动化用例，由模拟供应商人工验收与静态审查覆盖，缺口在交付说明中记录。
`pl-studio-runtime` 的配置迁移专项测试覆盖 schema 20→21：旧方言 token 改写且其它字段逐项
保留、无旧 token 的 20 配置等价迁移、未知未来版本仍 fail-closed 且不重写文件、迁移失败不落盘。

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
真实 socket 与暂停时钟协作时，业务时间只由用例显式推进，不得让自动推进在等待实际
I/O 时偷偷耗尽空闲预算。抑制自动推进的生命周期覆盖整个暂停区间，并在成功、失败或
panic 时都释放所属阻塞任务；不通过扩大 deadline、减少时间跨度或跳过用例掩盖问题。

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

压缩阈值的公开 API 证明以合成模型覆盖预设优先、缺省按解析上下文 90% 推导、未知或非正
安全容量不自动压缩、覆盖夹紧、恢复默认、provider/model 隔离和大整数计算。配置验证保留
缺键为默认、空集合省略与已有覆盖往返的边界；在线发现用例验证显式声明和缺省推导不混淆。
不将内置模型名单与默认数值逐项镜像成测试清单。Client/adapter 契约证明可空预设和默认的
完整投影、provider 自动保存只改变目标模型、并发修改/CAS 重试保留兄弟字段、旧 ACK 不清新意图。
双水位回归同时验证同 slug 目录更新重算预算、未知容量关闭压缩、撤下与重现保留覆盖，以及
默认暂时等于覆盖时只读投影不消费存量选择；旧配置响应不得回滚更新的目录预算。
原生供应商设置人工旅程另核对模型列表内输入、拖动和恢复默认的自动生效、无效输入不落盘、
失败反馈、未知容量及超限生效夹紧；截图检查宽窄窗口、键盘焦点、输入法与滚动，确认无需进入
编辑页或刷新。快照、配置、日志与进程回收共同证明真实 bridge 路径，不由 demo 或 fake 代替。
通用 `gui` 隔离场景提供派生默认、显式预设和未知容量三种模型元数据，默认推理路由不变，
以便在同一真实供应商详情列表观察差异与模型间覆盖隔离，不创建用户系统凭据关联。

GUI 验收编排由独立工程工具 `pl-studio-acceptance` 拥有；xtask 只在选择验收命令时
按需编译并启动该工具，原样透传参数，不维护场景或选项的第二份定义。验收工具直接使用
Studio canonical 配置及现有模拟供应商场景，数据库锁与证据检查不进入普通 xtask 依赖图。
两工具共用 `pl-dev-support` 的路径及进程支持；迁移不得改变交互输入、退出错误传播、
取消或进程树回收语义。验收工具使用正常 workspace 构建缓存并尊重显式目标目录配置，
不继承 xtask 入口专用的构建目录参数。场景仍保持全部现有能力，不通过删场景精简依赖。
fixture 的构建与依赖准备必须在服务就绪计时前完成，并从 Cargo 确认的本机可执行工件启动；
就绪期限只度量进程启动和本地服务握手，不包含编译或 Cargo 包缓存锁等待。启动后的进程
继续由统一的进程树 owner 管理，失败与取消的退出、回收边界不变。

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
响应和定速流式响应，并从设置页检查已记录的成功调用与可计算的性能汇总。定速流的
输出词元、首个非空输出事件和终止事件必须落在同一 inference 计时样本内；验收同时核对
实时状态栏与历史汇总采用相同的有效 decode 口径，不把缺失/零计时显示成高速度。在隔离的
`calls.sqlite` 上短暂持有写锁，检查写入未完成时的状态及释放后的自动刷新；同一隔离
home 重启后再次读取。取证保存设置页截图、快照、调用库计数和脱敏日志，结论仍由
人工填写。统计读写缺口不得误判为没有调用；不把模拟服务商验收外推为线上供应商兼容。
实时 projection 的 live output/decode 两个字段按成对样本判定：只注入其中一个字段时，
状态栏不得将它与上一份 authoritative 字段混算；应成对采用 authoritative 或保持未知。
状态栏还必须单独验证显示保持边界：同一 Thread 首次显示有效速度后，收到不含新 throughput
observation 的 runtime 更新仍显示最近快照；新 turn/attempt 的 partial、零值或无效 observation
不得借用旧值，Thread 切换或 readout 重建后也不得泄漏旧会话值。该判据使用稳定
`thread-throughput` key、最小 typed runtime snapshot 与前后截图/界面树记录，不能用外层
`??` 或延迟等待制造通过。

压力场景在 Flutter profile/AOT 模式及优化 Rust bridge 下按固定提示词从本地供应商以约 5,000 token/s 返回约 20,000 个混合消息事件；
采集请求耗时、事件完成数、GUI 帧/操作探针、窗口大小、截图、快照、未处理错误和进程回收；
内存证据按同一场景对 GUI 原生进程采样驻留内存（进程身份由 Driver 暴露），记录峰值与稳态。
该值是 Rust bridge 与 Flutter 引擎的同一进程合计，只用于同一场景的前后对比；不据此推断单层
分配、任意负载的内存上限或跨平台内存峰值，未运行的平台仍标记未验证。
事件数、独立 `item_id` 数与实际 token 数须分别记录：同三个条目上的两万次 delta
只能检验持续流和长正文，不能代替约两万条不同信息的窗口淘汰、深度分页与 GUI 压测。
压力入口还要在同一隔离项目中通过 Flutter Driver 创建 16 个独立会话，给第一个新会话
写入长正文，并重新打开原始长历史会话；逐会话核对目录身份、窗口上限、保存完成及
模型请求的严格匹配。长历史、长正文、长 Turn 和多会话分别留存证据，不以单次流速
代替四者的性能证据。场景脚本完成与人工验收结论分开：未观察到屏幕更新、卡死、未退出
的子进程或未核查截图时，不得判为 GUI 压测通过。

GUI 人工验收与 Client/adapter 门禁均不能证明未覆盖的 Studio、工具、远端进程、真实 HTTP/FRB
及发布流程行为；交付报告须明确具体缺口，不把新增 Client 测试写成完整 native 回归。

滚动验收使用真实拖动与滚轮信号，核对持续输出时自动跟随、小幅上翻即脱离、正文增长后
锚点及偏移不变、手动回到最新和显式跳最新后继续跟随，以及再次拖动不被贴底打断。
长正文场景同时保留会话切换后的阅读位置证据；不足一屏的折叠历史需另核对最早消息可达、
双向分页到端停止及窗口有界。滚动命令执行成功不能代替可见几何、截图与窗口身份的判定。
超长纯文本须同时核对实际可见字符在 canonical 正文中的位置和字符的视口像素位置；
仅有 itemId 与条目顶部偏移相等，不能排除条目内排版变化。流式追加、无关行更新、非追加
替换与全量重开应使用相同分区规则，选择复制仍须保持完整原文。处于真实底部但尚未跟随时，
向下滚轮即使不产生 ScrollUpdate 也应恢复跟随，嵌套滚动的消费优先权仍由框架协调。
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
来源与成员资格同样是公开 API 证明的一部分：在线名单之外的内置模型不出现、无观察时回退
`bundled` 来源、手动模型优先且来源为 `manual`、目录声明的排除 slug 不进入有效目录、
未知在线模型保持未知价格。手动模型删除的契约证明覆盖删除后配置与按 slug 覆盖值收敛、
非手动 slug 被拒绝，以及历史账单不受影响。
Studio 人工场景另观察每实例启动一次并行探测、一家失败另一家成功、默认回退、同身份成功缓存的
离线重启与 cached-only Profile、损坏缓存不全局重置、迟到结果/事件拒绝、普通设置保存不泄漏
发现模型、选中模型撤下的 unavailable 及在途请求冻结。取证保存模拟服务请求、成功缓存文件、
配置前后、typed 快照、原生截图和进程回收；不新增 Studio 常规自动测例或将启动探针等同完整验收。
隔离 harness 可注入显式 loopback 模型目录服务，为 OpenAI 两个独立实例与 DeepSeek 实例
提供可控声明，并以同一服务观察 MiMo 目录的 Chat 传输发现与排除列表；推理仍使用原有严格
fixture。目录故障、空名单和库存变更（重启后生效）不得改变推理 fixture 或用户配置，人工观察
与其他 GUI 场景采用相同的取证及进程回收边界。

## 24.5 单一内容链路验收

单一内容出口（ChatView `next`）与控制回执协议的实施验收按下列矩阵执行；证据分级沿用本篇
既有口径：静态审查只证明连接关系，Dart fake 只证明 reducer/组件行为，原生 FRB fixture 证明
端到端收敛，真实供应商另走人工观察，均不互相冒充。

- 无手动刷新的实时输出：打开会话后不滚动、不翻页，首字、连续 token、工具进度与终态均由
  内容泵交付（对应审查 F01；`manual-gui`/Driver 快照核对 `contentDelivery` 计数）。
- 控制操作不旁路：翻页、跳最新、锚点聚焦、正文展开后，窗口移动只经 `next` 的 Reset/Patch
  到达；Driver 断言无 `readStudioState` 热路径调用（F02/F03）。
- 帧时序：命令回执先到/帧先到两种顺序、`next` 挂起时 load/focus/close、低版本帧与跨版本
  合并 Patch、缺口后的绑定重开（F05 与流协议）。
- 生命周期：快速 A→B→A 切换回收旧 attempt 和晚到句柄；Settings、标签切换及 provider 无
  观察者期间保留当前 ChatView 和草稿。只有显式 Thread 切换、项目关闭和退出 retire 对应资源，
  新 view 由自己的 Reset 建立 baseline（F06）；Driver 核对 view 身份、窗口数量与关闭报告。
- 视图单事实源：Timeline 业务 projection 由 AppGraph 的具体 owner 管理；显示组件只接受
  props，附件读取只走 Client 公共 query（F07），预览与生产由 Connector 提供同一形状输入。
- 旧入口移除：GUI 生产链路不存在 SQL 分页（`listTimelineItems`）与实时条目的双路合并；
  `verify-client` 的 AST 边界检查拒绝 feature 直连 FRB/transport、Client 私有 import、provider
  生命周期关闭资源、命令成功后聚合回读和不稳定 key；静态通过只证明这些边界，不证明 native
  资源已实际回收，仍须结合前述运行证据。
- 时间线确定性回归：经生产投影验证混合活动组不跨正文、图片、压缩和智能体事件；
  固定流式完成、窗口移动、行回收与选择复制的交错，在旧实现失败后以同一输入验证修复。
  核心历史读取使用屏障证明等待期间的新正文、保存和终态不会回退；Client 以受控正文请求
  证明旧 attempt 不能污染新窗口。保留既有有效行为证明，删除只验证旧消息控制器内部操作的断言。
  选择清空后的右键菜单不得继续提供旧文本复制；该回归通过真实菜单验证，不只检查内部选区状态。
  代码块获得焦点后滚出视口时，必须证明官方保留缓存仍挂载且无法取得绘制坐标，随后检查
  发布锚点、滚动位置与逐帧诊断均有效，不能把该行误当成可见内容。
- 呈现稳定性（原生逐帧）：持续纯文本、持续推理摘要与长 Markdown 各做一组，另加历史滚动、
  保存确认、会话切换与故障重连；每组核对连续输出期间没有中间空窗、摘要不反复淡化、保存
  确认不改变正文几何、后台输出不打断历史阅读。**必须在动画尚未 settle 的帧上判定**：
  淡入淡出结束后截图完全可能正常，频闪恰好发生在动画周期内；只在 `pumpAndSettle` 之后
  截图或只看最终画面不算通过。
- 原生门禁记录：同一固定构建记录实际提交、加载的 Rust 动态库、Flutter/Rust 版本、DPI、
  窗口尺寸、GPU、刷新率与诊断开关；用 profile 核对 UI/raster 与异步链路耗时，用 release
  核验最终观感。debug 构建的耗时数据不能代替正式性能结论，帧率正常也不能证明动画正确。

## 存储职责重构验收

长任务必须证明常驻状态不保留历史请求输入，最新上下文增量写入而不反复序列化历史前缀。
同一确定性回归先红后绿；补充记录内存、数据库/WAL、日志大小和写入量。覆盖压缩后的
工具身份去重、最新请求重试、重启不重跑工具、历史与 checkpoint 原子提交、确认丢失、
迁移中断重试及日志时间/容量淘汰。日志故障或清理不能改变累计用量和会话恢复结果。
原生 GUI 人工验收覆盖历史回看、重开、压缩后继续及统计展示，并明确实际宿主平台。

存储压缩人工场景通过真实 bridge 和模拟供应商连续完成两次自动压缩，检查原始用户输入与
两次回复仍可完整回看，当前上下文与压缩回执只保留最新有效集合。该场景与计划跨重启恢复、
统计重启场景组合验证保存边界，不将模拟结果外推为真实供应商兼容性。

## 上下文连续实施验收

确定性公开接口回归覆盖事实追加与失效、完整输入预览、冻结水位、取消和冲突、指令更新能力、来源约束及完整工具批次。原生界面验收实际批准计划、推进非初始阶段、连续压缩两次、继续执行、关闭并重启，核对没有重复批准、阶段回退或工具重复执行。分别在独立根会话和父子兄弟会话设置合成标记，核查最终请求与持久化恢复的归属。

真实缓存对照固定端点、模型、任务、工具、推理强度和预热规则，以累计缓存读取词元除以累计总输入词元计算，辅助推理单列，缺失计量标为未知。证据关联会话、推理、压缩回执与进程身份，避免重启覆盖；实际阅读可读摘要后的最终请求，加密检查点仅核查来源、非空、数量、散列、持久化及发送。保留截图、界面树、状态、请求、执行产物和进程回收记录，人工结论独立记录。

真实上下文旅程通过 `context_state_journey.dart` 操作原生界面。切换会话须等待目标会话身份
加载完成；提交使用唯一观察编号，验收同时核对已受理输入身份、对应轮次完成状态和新答复。
待提交状态或旧历史答复不得证明重启后继续成功。覆盖批准后的非初始阶段、连续压缩、正常
关闭重启、独立根与父子兄弟代理隔离，以及模型基础指令更新和辅助调用身份关联。

## 24.4 实时新鲜度矩阵

实时验收必须维护一张覆盖 Thread runtime、turn/activity/storage、timeline、composer、产品
settings、provider usage/model performance/session costs、MCP/LSP/Skills/Profiles、persistence
queue、updater、recovery 与 lifecycle 的矩阵。每行记录可见字段、生产 owner、事件/快照、Dart
reducer/read model、资源版本域、selector/projection、Client 逻辑需求与页面观察、隐藏后的资源
保留/释放及重进基线、
lagged/失败/重连行为和未支持能力；无缺陷的行也保留静态证据，不能因已有类名推断实时正确。

下表定义验收职责；交付时在各行补充当前提交的实际事件、版本与证据，不得因类名已存在或旧测试
曾通过而认定本次 Client/FRB 迁移已覆盖，也不得省略无缺陷领域：

| 领域 | 可见字段/页面 | 生产触发与事件 | Dart mirror owner | selector/projection | Client 需求/页面观察 | 验收判据 |
| --- | --- | --- | --- | --- | --- | --- |
| Thread runtime | context、total/input/output/cache/reasoning tokens、context window、throughput、model、workflow/run、plan（状态与当前计划文档） | runtime snapshot/update、live usage | Client RuntimeChannel ordered reducer | runtime/status/context/throughput | Client selected Thread owner；Settings 保留 | paced stream 同页多次变化 |
| Turn/activity/storage | 阶段、活动摘要、保存水位、失败/取消 | turn/activity/storage notifications | 同一 Client RuntimeChannel reducer | turn/activity/storage | Session 逻辑寿命，独立于 provider | 阶段变化和保存故障即时可见 |
| Timeline | 流式正文、分页、正文补齐、图片/工具预览 | Client ChatView `initial`/`next` 与 typed receipt | Client ChatReadModel；TimelineSessionOwner lease | Connector/View props | current `threadId/viewId` | 不切页、不重读、不重复；迟到回调按 tuple 丢弃 |
| Composer | 文本、attachments、preview、pending/failure | Client DraftStore + admission result | Client canonical draft reducer | composer Connector/View | logical draft/project | paste/local/remove/submit race；旧 ACK 不清新 revision |
| Product settings | providers/routes/catalog/settings | typed resource baseline/frame + confirmed owner response | Client SettingsReadModel；catalog 独立版本 | settings/provider | app owner，页面只观察 | 保存后同页 canonical 更新，无成功补偿回读 |
| Usage/statistics/costs | quota、model performance、session costs | scoped product topics | Client scoped reducers | usage/statistics/costs | 可见需求由 Client 管理，observer 可取消 | 隐藏取消观察；需求重开取基线 |
| Resources | MCP/LSP/Skills/Profiles/updater/persistence/recovery | scoped topic/command | Client resource read models | resource projections | Client topic owner/current interest | 连接、失败、恢复即时可见 |
| Lifecycle | startup/shutdown/subscription diagnostics | startup/exit progress | AppGraph/Client lifecycle | startup/exit views | process owner | cancel/release/no stale frame；unresolved 保留 |

每个领域至少要有一个真实事件到同页 UI 变化的判据，记录变化前后最小脱敏 snapshot、event
revision、资源 stamp/attempt 与失败恢复结果。不得用延迟等待、切页、重进或前端轮询替代事件证据。
Thread runtime 的 paced stream 应在同一 attempt 完成前观察多次 live 变化并在完成后收敛
authoritative；无法提供 partial token 的能力显示 unknown，不得显示以字符数或字节数计算的假值。
状态栏的显示保持测试与 canonical throughput 计算测试分开：前者证明组件的短暂显示生命周期，
后者继续证明成对样本和 unknown 语义。

Driver snapshot 只暴露必要 typed 诊断，不暴露图片 bytes、凭据、raw payload 或内部数据库。
粘贴回归使用 Driver-only `ClipboardImageReader` override：先断言当前 composer 出现准入 draft
metadata，再断言同一 draft 的 preview-ready 更新，覆盖删除、失败、提交和并发切换。供应商列表
断言不含 fixture model slug、模型胶囊或 `+N`，同时详情/编辑/选择器仍保留模型数量和能力。
topic 弹层保持打开时须更新，隐藏后自己的观察释放；最后一个 Client 逻辑需求释放时才关闭该
topic。Settings 隐藏不能关闭当前会话、清草稿或扫描附件。重进原子取得当前值，重开资源再取
自己的基线并拒绝旧 frame；stream lagged、
旧 generation、topic closed、应用退出和 provider/preview 失败都必须验证 last-valid、typed
failure 或 unknown 及资源释放。

应用 `test/client` 是唯一进入 `verify-client` 的 Flutter 测试目录；其他 widget/integration、
`test_driver`、真实宿主的启动路由与实时 UI 仍通过 `manual-gui`/Driver 和人工取证覆盖，不加入自动门禁，
也不建立第二套实时状态事实源。真实系统剪贴板 PNG 另在当前宿主人工验收并记录截图、snapshot、
日志和进程回收；模拟或纯 Dart 通过不外推真实供应商兼容、native FRB 或跨平台通过。

## 24.6 供应商、额度与 MCP 隔离验收

供应商自动额度需求按 [25.7.1](./25-frontend-state.md#2571-供应商用量需求) 验证。Client/adapter
公开行为证明覆盖首次可见自动加载、按实例并行和去重、隐藏/重现、查询身份失效、删除与旧响应
拒绝、失败不串卡及应用实例隔离；表单保存不得依赖额度完成。优先增强既有证明并记录确定性
红绿结果，不在 Studio 新增常规 Rust 测试或以 fake 代替真实后端。

隔离原生观察使用可控 loopback 模型、额度和通用 MCP 服务：进入供应商标签后不点击刷新，
核对各实例请求已进入，慢/401 实例未结束时快实例已显示成功；同时完成供应商保存、再次
新增/编辑/取消、其它标签操作及新 Thread 创建。释放慢请求后各领域独立收敛，设置默认、
名称、筛选与模型目录观察不得使所有实例重查。补查隐藏期间新增/改 key 的实例，并验证
迟到响应不复活删除项或覆盖新查询身份。

智谱系统 MCP 的凭据门控在 canonical 配置合成边界核对空 key、普通智谱/域名不匹配、
多个 Coding Plan 稳定选择、轮换/清空/删除及显式禁用；连接、发现、调用与在途租约复用
通用 MCP 观察。不得为模拟固定端点放松 TLS、修改 hosts 或新增生产测试后门。
证据包括逐阶段 typed 快照、截图/界面树、模拟服务请求和最终进程回收；脚本成功与人工结论
分开，离线模拟不证明官方服务兼容性，单平台结果不外推其它平台。
