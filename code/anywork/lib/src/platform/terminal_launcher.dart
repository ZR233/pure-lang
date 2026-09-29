import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'terminal_host.dart';

/// 会话顶栏外部「终端」入口的打开目标；GUI 只负责选择目标与反馈，
/// 平台层负责入口探测与启动。
sealed class HostTerminalTarget {
  const HostTerminalTarget();
}

/// 本地项目：在会话工作区目录中启动宿主默认交互 shell。
class LocalHostTerminalTarget extends HostTerminalTarget {
  const LocalHostTerminalTarget({required this.directory});

  /// 本地绝对路径；按入口参数契约转换为工作目录或进程工作目录。
  final String directory;
}

/// 远端项目：宿主终端内经系统 OpenSSH 连接 Host 别名后的交互会话。
///
/// 连接参数全部由 `~/.ssh/config` 的 Host 别名解析，不在参数中复制端口、
/// 用户或密钥。
class RemoteSshTerminalTarget extends HostTerminalTarget {
  const RemoteSshTerminalTarget({
    required this.alias,
    required this.remotePath,
  });

  /// `~/.ssh/config` 中的 Host 别名，单 token 且不得成为 SSH 选项。
  final String alias;

  /// 会话 canonical 远端 POSIX 路径。
  final String remotePath;
}

/// 宿主真实存在的终端入口；由平台层按宿主事实解析，GUI 不猜测已安装终端。
enum HostTerminalProgram {
  /// Windows Terminal；`;` 在其 argv 层命令拆分中是分隔符，值参数需经
  /// [escapeWindowsTerminalValue] 转义。
  windowsTerminal('wt.exe'),

  /// XDG Default Terminal Execution 规范入口，`--dir=` 指定工作目录，
  /// 命令参数按 argv 原样转交终端，不经 shell 拼接。
  xdgTerminalExec('xdg-terminal-exec'),

  /// 系统配置的 x-terminal-emulator；无标准工作目录参数，本地依赖进程
  /// 继承的工作目录，执行命令使用 `-e`。
  xTerminalEmulator('x-terminal-emulator');

  const HostTerminalProgram(this.executableName);

  final String executableName;
}

/// 终端启动计划：参数始终以数组传递，不经本地 shell 拼接。
class HostTerminalLaunchPlan {
  const HostTerminalLaunchPlan({
    required this.executable,
    required this.arguments,
    this.workingDirectory,
  });

  /// 平台层解析出的可执行文件绝对路径。
  final String executable;

  final List<String> arguments;

  /// 仅本地 `x-terminal-emulator` 使用：该入口没有工作目录参数，
  /// 依赖子进程继承的工作目录。
  final String? workingDirectory;
}

final RegExp _controlCharacters = RegExp(r'[\u0000-\u001F\u007F]');

/// 进程内缓存一次宿主终端入口探测；demo 与 widget 测试可覆写注入确定结果。
final terminalAvailabilityProvider = FutureProvider<bool>((ref) async {
  return probeHostTerminalAvailable();
});

typedef HostTerminalLauncher = Future<void> Function(HostTerminalTarget target);

/// 启动用户主动打开的外部终端；独立生命周期，不随 Studio 退出终止。
final terminalLauncherProvider = Provider<HostTerminalLauncher>(
  (ref) => (target) async {
    // 先做纯校验（别名、远端路径与本地目录），失败以 ArgumentError 上抛，
    // 由 GUI 统一提示，不触碰宿主终端入口。
    validateHostTerminalTarget(target);
    await launchHostTerminal(target);
  },
);

/// 构建启动计划；参数契约按入口各自的真实语义设置工作目录与命令。
HostTerminalLaunchPlan buildHostTerminalLaunchPlan({
  required String executable,
  required HostTerminalProgram program,
  required HostTerminalTarget target,
}) {
  validateHostTerminalTarget(target);
  return switch ((program, target)) {
    (
      HostTerminalProgram.windowsTerminal,
      LocalHostTerminalTarget(directory: final directory),
    ) =>
      HostTerminalLaunchPlan(
        executable: executable,
        arguments: ['-d', escapeWindowsTerminalValue(directory)],
      ),
    (
      HostTerminalProgram.windowsTerminal,
      RemoteSshTerminalTarget(alias: final alias, remotePath: final remotePath),
    ) =>
      HostTerminalLaunchPlan(
        executable: executable,
        // ssh 由 wt 新标签页按 PATH 解析并交互执行；远端命令见
        // [buildRemoteSshRemoteCommand]。别名按 argv 层分号规则转义；
        // 远端命令是含空格的命令元素，还要先按 wt 二次命令行拼接语义
        // 转义内嵌引号，再应用分号转义，保证 ssh 收到原始命令串。
        arguments: [
          'ssh',
          '-t',
          escapeWindowsTerminalValue(alias),
          escapeWindowsTerminalValue(
            escapeWindowsTerminalInnerQuotes(
              buildRemoteSshRemoteCommand(remotePath),
            ),
          ),
        ],
      ),
    (
      HostTerminalProgram.xdgTerminalExec,
      LocalHostTerminalTarget(directory: final directory),
    ) =>
      HostTerminalLaunchPlan(
        executable: executable,
        // 规范要求 `--dir` 以单参数形式传递，不携带命令时启动默认 shell。
        arguments: ['--dir=$directory'],
      ),
    (
      HostTerminalProgram.xdgTerminalExec,
      RemoteSshTerminalTarget(alias: final alias, remotePath: final remotePath),
    ) =>
      HostTerminalLaunchPlan(
        executable: executable,
        arguments: [
          'ssh',
          '-t',
          alias,
          buildRemoteSshRemoteCommand(remotePath),
        ],
      ),
    (
      HostTerminalProgram.xTerminalEmulator,
      LocalHostTerminalTarget(directory: final directory),
    ) =>
      HostTerminalLaunchPlan(
        executable: executable,
        arguments: const [],
        workingDirectory: directory,
      ),
    (
      HostTerminalProgram.xTerminalEmulator,
      RemoteSshTerminalTarget(alias: final alias, remotePath: final remotePath),
    ) =>
      HostTerminalLaunchPlan(
        executable: executable,
        arguments: [
          '-e',
          'ssh',
          '-t',
          alias,
          buildRemoteSshRemoteCommand(remotePath),
        ],
      ),
  };
}

/// 校验打开目标；远端别名与路径不满足契约时抛出 [ArgumentError]。
void validateHostTerminalTarget(HostTerminalTarget target) {
  switch (target) {
    case LocalHostTerminalTarget(directory: final directory):
      if (directory.trim().isEmpty || _controlCharacters.hasMatch(directory)) {
        throw ArgumentError.value(directory, 'directory', 'Invalid workspace');
      }
    case RemoteSshTerminalTarget(
      alias: final alias,
      remotePath: final remotePath,
    ):
      _validateSshHostAlias(alias);
      if (!_isValidRemotePath(remotePath)) {
        throw ArgumentError.value(
          remotePath,
          'remotePath',
          'Invalid remote workspace',
        );
      }
  }
}

void _validateSshHostAlias(String alias) {
  if (alias.isEmpty ||
      alias.startsWith('-') ||
      alias.contains(RegExp('[\\s"\';]')) ||
      _controlCharacters.hasMatch(alias)) {
    // 别名只能是单 token：不得成为 SSH 选项，也不得在 Windows Terminal 的
    // `;` 分隔或远端命令解析中被拆分成其他命令。
    throw ArgumentError.value(alias, 'alias', 'Invalid SSH host alias');
  }
}

bool _isValidRemotePath(String remotePath) {
  return remotePath.startsWith('/') &&
      !remotePath.contains('\\') &&
      !_controlCharacters.hasMatch(remotePath);
}

/// 远端命令：安全切换到 canonical 远端目录后进入交互 shell。
///
/// 目录按 POSIX 引用规则转义（单引号包裹、内嵌单引号写成 `'\''`），空格、
/// 引号与 shell 元字符不会逃逸；`cd` 失败时远端 shell 打印错误并退出，
/// 不会落进其他目录。`$SHELL` 由 sshd 按远端账户设置，交互会话不改动
/// 用户安全配置。
String buildRemoteSshRemoteCommand(String remotePath) {
  if (!_isValidRemotePath(remotePath)) {
    throw ArgumentError.value(remotePath, 'remotePath', 'Invalid remote path');
  }
  final quoted = "'${remotePath.replaceAll("'", r"'\''")}'";
  return 'cd $quoted && exec "\$SHELL"';
}

/// 为 Windows Terminal 准备值参数：wt 在 argv 层拆分命令，不能只假设
/// argv 安全即 wt 安全。
///
/// 上游 `AppCommandlineArgs::_addCommandsForArg` 用 `^;|[^\\];` 识别命令
/// 分隔符——MSVCRT 引号在进入 argv 时已被消费，不能保护含空格参数里的
/// 分号；`Commandline` 的 `AddArg` 只把 `\;` 还原为 `;`。因此对每个 wt
/// 值参数无条件把 `;` 写成 `\;`：拆分正则不再命中，wt 重建子进程命令行
/// 时还原出原始字面量分号，与输入往返一致。含引号的命令元素需先经
/// [escapeWindowsTerminalInnerQuotes] 编码后再应用本函数。
String escapeWindowsTerminalValue(String value) {
  return value.replaceAll(';', r'\;');
}

/// wt 重建子进程命令行前的二次 MSVCRT 编码。
///
/// 上游 `_getNewTerminalArgs` 拼接命令元素时，对含空格的元素只添加外围
/// 双引号，不转义内嵌双引号或反斜杠；ssh 一侧按 MSVCRT 规则重解析时会把
/// 未转义的内嵌引号当作引用边界吞掉、折半紧邻引号的反斜杠，破坏远端命令
/// 的 POSIX 单引号防注入与 `$SHELL` 引用。这里按 Windows 命令行规则转义
/// 内部双引号（其前反斜杠成对加倍）与末尾反斜杠；不自行添加外围引号——
/// 远端命令固定含空格，wt 会自己包裹。调用顺序：先本函数，再
/// [escapeWindowsTerminalValue]。
String escapeWindowsTerminalInnerQuotes(String value) {
  final out = StringBuffer();
  var backslashes = 0;
  for (var i = 0; i < value.length; i++) {
    final char = value[i];
    if (char == r'\') {
      backslashes++;
      continue;
    }
    if (char == '"') {
      out.write('\\' * (backslashes * 2 + 1));
      out.write('"');
    } else {
      out.write('\\' * backslashes);
      out.write(char);
    }
    backslashes = 0;
  }
  // 末尾反斜杠紧邻 wt 添加的外围引号，需成对加倍。
  out.write('\\' * (backslashes * 2));
  return out.toString();
}
