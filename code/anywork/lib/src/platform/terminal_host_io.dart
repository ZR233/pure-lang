import 'dart:io';

import 'terminal_launcher.dart';

/// 探测宿主可用的终端入口；只查找声明的入口，探测不到即不可用，
/// 不猜测其他已安装终端，也不以文件管理器代替。
Future<bool> probeHostTerminalAvailable() async {
  return await _resolveHostTerminal() != null;
}

/// 解析入口并启动外部终端；计划参数以数组传递，不经本地 shell 拼接。
Future<void> launchHostTerminal(HostTerminalTarget target) async {
  final resolved = await _resolveHostTerminal();
  if (resolved == null) {
    throw StateError('No host terminal is available');
  }
  final plan = buildHostTerminalLaunchPlan(
    executable: resolved.executable,
    program: resolved.program,
    target: target,
  );
  // 终端由用户主动打开，拥有独立生命周期：detached 启动后 Studio 退出
  // 不回收该进程。
  await Process.start(
    plan.executable,
    plan.arguments,
    workingDirectory: plan.workingDirectory,
    mode: ProcessStartMode.detached,
  );
}

/// 按宿主解析终端入口。Windows 只使用 Windows Terminal；Linux 优先
/// `xdg-terminal-exec`，其次系统配置的 `x-terminal-emulator`。
Future<({HostTerminalProgram program, String executable})?>
_resolveHostTerminal() async {
  const candidates = [
    (program: HostTerminalProgram.windowsTerminal, platforms: ['windows']),
    (program: HostTerminalProgram.xdgTerminalExec, platforms: ['linux']),
    (program: HostTerminalProgram.xTerminalEmulator, platforms: ['linux']),
  ];
  final os = Platform.operatingSystem;
  for (final candidate in candidates) {
    if (!candidate.platforms.contains(os)) continue;
    final executable = await _findExecutableInPath(
      candidate.program.executableName,
    );
    if (executable != null) {
      return (program: candidate.program, executable: executable);
    }
  }
  return null;
}

/// 在 `PATH` 中查找可执行文件；Windows 的 wt.exe 位于 WindowsApps 执行
/// 别名目录，符号链接与常规文件都接受，Linux 还要求任一执行位。
Future<String?> _findExecutableInPath(String name) async {
  final separator = Platform.isWindows ? ';' : ':';
  final directories = (Platform.environment['PATH'] ?? '').split(separator);
  for (final directory in directories) {
    if (directory.isEmpty) continue;
    final candidate = '$directory${Platform.pathSeparator}$name';
    if (Platform.isWindows) {
      // WindowsApps execution aliases are reparse points, not ordinary symlinks.
      // Following them with FileStat.stat reports notFound even when Windows
      // can launch the alias. Inspect the entry itself and let startup resolve it.
      final type = await FileSystemEntity.type(candidate, followLinks: false);
      if (type == FileSystemEntityType.file ||
          type == FileSystemEntityType.link) {
        return candidate;
      }
    } else {
      final stat = await FileStat.stat(candidate);
      // 0x49 = 0o111：任一执行位即可执行。
      final executable =
          stat.type == FileSystemEntityType.file && (stat.mode & 0x49) != 0;
      if (executable) return candidate;
    }
  }
  return null;
}
