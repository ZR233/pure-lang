import 'dart:io';

import 'vscode_icon.dart';

/// 探测宿主是否提供 VS Code 打开入口。
///
/// Windows 查询与图标相同的 `vscode` 协议关联；Linux/其他桌面检查 PATH 与
/// `x-scheme-handler/vscode` 的默认处理器（覆盖无 PATH 链接的解压安装）。
Future<bool> probeVsCodeInstalled() async {
  if (Platform.isWindows) {
    return _probeWindows();
  }
  return _probeUnixLike();
}

/// PATH 目录列表中是否存在任一候选可执行文件；纯函数便于单测。
bool pathEntriesContainExecutable(
  List<String> pathEntries,
  List<String> candidates,
) {
  for (final directory in pathEntries) {
    if (directory.isEmpty) continue;
    for (final candidate in candidates) {
      if (File('$directory${Platform.pathSeparator}$candidate').existsSync()) {
        return true;
      }
    }
  }
  return false;
}

Future<bool> _probeWindows() async {
  try {
    return await vsCodeIconChannel.invokeMethod<bool>('vsCodeAvailable') ??
        false;
  } on Object {
    return false;
  }
}

Future<bool> _probeUnixLike() async {
  if (pathEntriesContainExecutable(_pathEntries(), ['code', 'code-insiders'])) {
    return true;
  }
  try {
    final result = await Process.run('xdg-mime', [
      'query',
      'default',
      'x-scheme-handler/vscode',
    ]);
    final handler = (result.stdout as String).trim();
    return result.exitCode == 0 && handler.isNotEmpty;
  } on Object {
    return false;
  }
}

List<String> _pathEntries() =>
    (Platform.environment['PATH'] ?? '').split(Platform.pathSeparator);
