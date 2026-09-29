import 'dart:io';

import 'host_app_icons.dart';

/// 可用性与启动共用同一解析入口，保证探测和实际启动一致。
Future<String?> _executable() async {
  final path = await hostAppsChannel.invokeMethod<String>('zedExecutable');
  return path == null || path.isEmpty ? null : path;
}

Future<bool> probeZedInstalled() async {
  try {
    return await _executable() != null;
  } on Object {
    return false;
  }
}

Future<void> launchZedWorkspace(String workspaceArgument) async {
  final executable = await _executable();
  if (executable == null) throw StateError('Zed is unavailable');
  // 用户主动打开的 GUI 应用拥有独立生命周期；参数按 argv 原样传递，
  // 不经过 shell 或外部 URL 打开入口。
  await Process.start(executable, [
    workspaceArgument,
  ], mode: ProcessStartMode.detached);
}
