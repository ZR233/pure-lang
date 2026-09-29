import 'dart:io';

import 'host_app_icons.dart';

/// 可用性与启动共用同一解析入口，保证探测和实际启动一致。
Future<String?> _executable() async {
  final path = await hostAppsChannel.invokeMethod<String>('vsCodeExecutable');
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
