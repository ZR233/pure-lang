import 'dart:io';

import 'package:flutter/services.dart';

/// 宿主应用通道名；仅承载 VS Code 可执行文件解析，与各平台 runner 共享。
const MethodChannel _hostAppsChannel = MethodChannel(
  'io.github.zr233.anywork/host_apps',
);

/// 可用性与启动共用同一解析入口，保证探测和实际启动一致。
Future<String?> _executable() async {
  final path = await _hostAppsChannel.invokeMethod<String>('vsCodeExecutable');
  return path == null || path.isEmpty ? null : path;
}

Future<bool> probeVsCodeInstalled() async {
  try {
    return await _executable() != null;
  } on Object {
    return false;
  }
}

Future<void> launchVsCodeFolder(String folderUri) async {
  final executable = await _executable();
  if (executable == null) throw StateError('VS Code is unavailable');
  // A GUI app owns its own lifetime. No shell, .cmd wrapper or --open-url path;
  // each argument is passed literally, including spaces and URI escaping.
  await Process.start(executable, [
    '--folder-uri',
    folderUri,
  ], mode: ProcessStartMode.detached);
}
