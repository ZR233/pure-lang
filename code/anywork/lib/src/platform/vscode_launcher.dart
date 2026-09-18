import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'studio_platform.dart';
import 'vscode_detection.dart';

typedef VsCodeLauncher = Future<void> Function(String url);

const _maxVsCodeUrlBytes = 8 * 1024;
final _urlControlCharacters = RegExp(r'[\u0000-\u001F\u007F-\u009F]');

/// 进程内缓存一次 VS Code 安装探测；widget 测试用覆写注入确定结果。
final vsCodeAvailabilityProvider = FutureProvider<bool>((ref) async {
  return probeVsCodeInstalled();
});

/// 打开 vscode 协议 URL 的注入点；生产实现复用外部 URL 启动器。
final vsCodeLauncherProvider = Provider<VsCodeLauncher>(
  (ref) => openExternalUrl,
);

/// 只放行两种 VS Code 打开形态，规则对齐 `safeExternalWebUrl` 的清洗边界。
String? safeVsCodeUrl(String value) {
  if (utf8.encode(value).length > _maxVsCodeUrlBytes) {
    return null;
  }
  final sanitized = value.replaceAll(_urlControlCharacters, '');
  final isLocal = sanitized.startsWith('vscode://file/');
  final isRemote = sanitized.startsWith('vscode://vscode-remote/ssh-remote+');
  if (!isLocal && !isRemote) {
    return null;
  }
  return Uri.tryParse(sanitized) == null ? null : sanitized;
}

/// 本地文件夹 URI：`vscode://file/<绝对路径>/`，尾斜杠表示打开为工作区。
///
/// Windows 盘符路径形如 `C:\x\y` → `vscode://file/c:/x/y/`；POSIX 路径
/// `/home/x/y` → `vscode://file/home/x/y/`。空格等字符按 URI 规则转义，
/// 盘符冒号保留原样。
String buildLocalVsCodeFolderUri(String absolutePath) {
  var normalized = absolutePath.replaceAll('\\', '/');
  if (normalized.startsWith('/')) {
    normalized = normalized.substring(1);
  } else if (normalized.length >= 2 && normalized[1] == ':') {
    normalized =
        normalized.substring(0, 1).toLowerCase() + normalized.substring(1);
  }
  if (normalized.endsWith('/')) {
    normalized = normalized.substring(0, normalized.length - 1);
  }
  final segments = normalized.split('/');
  final encodedPath = [
    for (final (index, segment) in segments.indexed)
      index == 0 && RegExp(r'^[a-zA-Z]:$').hasMatch(segment)
          ? segment
          : Uri.encodeComponent(segment),
  ].join('/');
  return 'vscode://file/$encodedPath/';
}

/// 远端文件夹 URI：`vscode://vscode-remote/ssh-remote+<别名>/<远端路径>`。
///
/// Remote-SSH 的 URI 不携带端口与密钥，连接参数由 `~/.ssh/config` 的别名解析。
String buildRemoteVsCodeFolderUri({
  required String alias,
  required String remotePath,
}) {
  var normalized = remotePath.replaceAll('\\', '/');
  while (normalized.startsWith('/')) {
    normalized = normalized.substring(1);
  }
  final encodedAlias = Uri.encodeComponent(alias);
  final encodedPath = normalized.split('/').map(Uri.encodeComponent).join('/');
  return 'vscode://vscode-remote/ssh-remote+$encodedAlias/$encodedPath';
}
