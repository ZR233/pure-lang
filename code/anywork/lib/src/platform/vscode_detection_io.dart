import 'dart:io';

/// 探测本机是否安装了 VS Code（稳定版或 Insiders）。
///
/// Windows 检查 PATH 与默认安装位置；Linux/其他桌面检查 PATH 与
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
  final pathEntries = _pathEntries();
  if (pathEntriesContainExecutable(pathEntries, [
    'code.cmd',
    'code.exe',
    'code-insiders.cmd',
  ])) {
    return true;
  }
  final localAppData = Platform.environment['LOCALAPPDATA'];
  final programFiles = Platform.environment['ProgramFiles'];
  final candidates = [
    if (localAppData != null)
      '$localAppData\\Programs\\Microsoft VS Code\\bin\\code.cmd',
    if (localAppData != null)
      '$localAppData\\Programs\\Microsoft VS Code Insiders\\bin\\code-insiders.cmd',
    if (programFiles != null) '$programFiles\\Microsoft VS Code\\bin\\code.cmd',
    if (programFiles != null)
      '$programFiles\\Microsoft VS Code Insiders\\bin\\code-insiders.cmd',
  ];
  return candidates.any((candidate) => File(candidate).existsSync());
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
