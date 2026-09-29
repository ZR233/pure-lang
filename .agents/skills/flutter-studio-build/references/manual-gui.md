# 原生 GUI 人工验收

## 入口与范围

- 从仓库根目录运行 `cargo xtask manual-gui`，由 harness 创建隔离的 `ANYWORK_HOME`、
  本地模拟供应商和真实 bridge GUI，不修改用户配置或凭据。不要用 demo 证明持久化行为。
- 先明确要验证的交互、状态变化、异常路径及证据；通过 `xtask/src/cli/mod.rs` 和
  `xtask/src/manual_gui.rs` 核实当前场景参数，不为绕过终端检查选择无关场景。
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
