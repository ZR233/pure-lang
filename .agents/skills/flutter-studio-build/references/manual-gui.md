# 原生 GUI 人工验收

## 入口与范围

- 从仓库根目录运行 `cargo xtask manual-gui`，由 harness 创建隔离的 `ANYWORK_HOME`、
  本地模拟供应商和真实 bridge GUI，不修改用户配置或凭据。不要用 demo 证明持久化行为。
  xtask 仅按需编译并调用独立 `pl-studio-acceptance`，不链接该工具的重依赖。
- 先明确要验证的交互、状态变化、异常路径及证据；通过 `code/pl-studio-acceptance/src/`
  核实当前 CLI 与场景参数，不为绕过终端检查选择无关场景。
- `cargo xtask verify-gui` 负责生成一致性、格式及静态分析，不包含 GUI 人工结论。
- 真实供应商验收使用 `cargo xtask run-gui --driver`，不把模拟通过外推为真实模型兼容。
  使用外部 helper 模式的 Linux Studio/Server 入口按根 `AGENTS.md` 预先安装 remote helper；
  桌面 xtask 入口使用 bundle 内的 `data/remote-helper/`，不能以 PATH 中的程序代替资源校验。

## Linux SSH：补齐终端和显示环境

先检查事实，不根据控制端环境猜测：

```bash
test -t 0 && echo 'stdin: tty' || echo 'stdin: not a tty'
printf 'DISPLAY=%s WAYLAND_DISPLAY=%s\n' "$DISPLAY" "$WAYLAND_DISPLAY"
command -v script xvfb-run Xvfb xauth
```

默认 `gui` 场景要求交互 stdin；非 TTY 会返回 `manual-gui requires an interactive terminal`。
显示环境变量为空说明未指定桌面会话，并不证明本机无法运行 GUI。工具齐全时，可用伪终端和
隔离虚拟显示器，不占用用户桌面，也不绕开 xtask：

```bash
xvfb-run -a -s '-screen 0 1440x1000x24' \
  script -q -e -c 'cargo xtask manual-gui' /dev/null
```

需要固定证据目录时，给内部命令增加 `--output target/manual-gui/<本次唯一目录名>`，
目录必须尚不存在；不要删除旧证据来复用路径。已有可用显示会话时按需要只用 `script`。
`script -e` 透传子命令退出状态，`xvfb-run -a` 自动分配显示号并在命令退出后回收 Xvfb。
不要把示例虚拟屏幕大小当作实际窗口大小；窗口尺寸须从截图、Driver 或窗口信息核实。

保持命令的 stdin 可写，保存后台 taskId；给首次 Rust/Flutter 构建留足时间，用 `wait`
等待，不重复启动。GUI ready 后才能交互。若启动迟迟未完成，检查本次进程树及其日志，
区分构建中、VM Service 未就绪、显示失败与应用异常，不盲目重试。

## Flutter Driver 操作与取证

1. 使用 Flutter Driver，不用 Computer Use 覆盖 Driver 已支持的交互。优先复用
   `code/anywork/test_driver/flutter_driver_session.dart`；它提供有截止时间的操作和
   `readSnapshot()`、`renderTree()`、`screenshot()`。客户端在 `finally` 中关闭连接。
2. 从**本次**启动日志取得 VM Service URL。harness 最终日志会脱敏该地址，故需要在运行中
   根据本次子进程/日志定位；不要硬编码端口、PID、临时目录或以广泛扫描他人临时目录代替定位。
   不占用项目保留端口 `1420`，不对外暴露调试服务。
3. 先读当前源码中的 `StudioDriverKeys`、实际控件 key 与快照，再构建 Finder。
   动态 key 可包含 provider/profile ID，不能猜测为空或照抄上次 ID。
   Linux 原生环境中普通 tap 的 hit-test 可能超时，项目已有 `rawTap` 扩展会验证目标及
   模态遮挡，应复用该入口而不是屏幕坐标点击；查找超时先核对 key、滚动位置及模态状态。
4. 临时定向客户端可放在忽略的 `target/gui-acceptance/probe.dart`，无需增加永久测试。
   该位置可通过 `../../code/anywork/test_driver/flutter_driver_session.dart` 导入会话封装。
   从根目录运行（将 `<本次VM_URL>` 替换为实测地址）：

   ```bash
   cargo dart --packages=.dart_tool/package_config.json \
     ../../target/gui-acceptance/probe.dart '<本次VM_URL>'
   ```

   包装命令实际 cwd 为 `code/anywork`；脚本的文件输出路径也须据此计算，或显式传入
   已确认的路径。不在 SDK 目录运行项目命令，不直接调用单个 GUI 生成器。
5. 对关键状态分别保存截图、界面树、必要的最小快照，并实际打开截图核对。检查日志异常、
   布局溢出、菜单边界、禁用态与键盘可访问性；按需求覆盖长名称、窄窗口、空数据及无效选择。
   保存行为必须观察 bridge canonical 状态，必要时重新打开验证；仅点击当前值不能证明
   更换值后的持久化。只有一个 fixture 模型时，明确记录覆盖不足。
6. 不把临时原始快照/界面树当作已脱敏产物；可能包含正文、配置与路径。只收集需要的字段，
   分享前检查并脱敏；不提交截图、运行日志、VM URL 或一次性脚本到源码库。

## 跨重启交互与 Driver 判定

- `cargo xtask manual-gui --scenario plan-recovery` 使用同一隔离 home 的多个原生 GUI 生命周期，
  覆盖待确认计划重开后的批准、修改和已回答计划不再弹出；Linux 无显示环境时配合 Xvfb。
- 起始页的 Thread 在提交 prompt 后才创建，不能先等待 Thread ID 再提交。Turn 已 completed
  也可能仍在 flushing；成功等待须包含持久化就绪，不能把 completed 当作等待失败条件。
- 点击命令超时不证明动作未发生。对已有明确前置状态和可验证后置状态的动作，可在只读重连
  后有界确认结果并记录证据；不得重放批准、提交等副作用，也不得吞掉未达成后置状态的错误。
- 跨进程阶段使用独立不可覆盖的阶段事实或显式握手，不依赖 latest-stage 的短暂值。用于
  证明无额外请求的观测文件必须传播写失败，不能用陈旧计数制造静默窗口通过。

## 富文本选择与复制回归

- 排版截图正确不代表复制正确。覆盖中文与行内代码、连续代码片段、长路径、标题后有/无空行，
  在正常/窄窗口及不同文字缩放下执行真实选择与复制，逐字核对剪贴板中的字符、空白和顺序。
  代码块若使用独立 `SelectableText`，需单独验证，不能把外层全选当作代码块复制证据。
- 优先用 Driver 完成交互；Driver 无法读取剪贴板或诊断选择几何时，可在本次隔离 debug GUI
  的 VM Service 中使用一次性探针调用现有选择/复制操作，再读 `Clipboard.getData`。不得直接
  写入期望文本伪造成功，不修改生产 Driver 或 SDK。hot reload 后先等待一帧稳定再访问布局。
- `WidgetSpan` 可切分选择片段；占位字符未进入剪贴板，并不证明片段顺序正确。Flutter 的
  几何排序读取文本选择盒，不等于 `RenderParagraph.size`；单纯拉宽容器不能证明修复有效。
  如需诊断，读取当前 SDK 实现，再在真实 RenderParagraph 上按片段范围调用
  `getBoxesForSelection`，使用与 SDK 一致的 `BoxHeightStyle` 并转换到同一全局坐标。
  前导换行的零宽选择盒也可能扩大片段包围框，应一并记录，避免仅凭文字墨迹或截图推断。
- VM 探针、原始剪贴板和几何数据仅保存在忽略的证据目录；不固化 VM URL、PID、临时路径或
  调试私有 API 为产品接口。复制回归以实际结果为准，静态推演与格式/分析通过不能替代。

## 结束、回收与判定

- 在 harness ready 后，通过所属 task 的 `write_stdin` 发送实际发生的动作代码（如
  `open-settings`、`select-model`），这些代码只是记录，不会替你执行 UI 操作。
- 完成后发送 `done\n`。harness 调用 `test_driver/manual_capture.dart` 保存最终截图和摘要，
  请求应用 shutdown；必须等待原任务终态，核对 `capture-stage.txt` 的 `shutdown_completed`。
- 失败或取消也要回收：优先向本次伪终端发送 Ctrl-C，让 harness 清理；必要时使用任务取消
  并等待终态。核对本次所属 Flutter、DTD、GUI、fixture、Driver 与 Xvfb 进程是否退出。
  不使用无差别 `pkill`，不杀用户已有桌面或无关任务；仍有残留则明确报告并按所有权处理。
- 结合截图、快照、日志和进程状态给结论。`verdict.json` 默认为 `pending`；命令 exit 0、
  日志无错误或截图成功均不自动构成完整验收。未执行 fixture 主流程时可能提示
  `Fixture main step was not exercised`，可报告定向 UI 结果，但不能宣称端到端流程通过。
- 汇报实际平台/显示方式、覆盖项、未覆盖项、证据目录、命令退出状态和回收结果。
  遇阻记录真实命令及原始错误，区分缺 TTY、显示服务、原生构建依赖、Driver 连接/定位问题。
  缺工具时报告具体缺项，不擅自安装系统包，也不写死机器专用 include/library 路径。
