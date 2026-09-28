import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'vscode_host.dart';

typedef VsCodeLauncher = Future<void> Function(String url);

const _maxVsCodeUrlBytes = 8 * 1024;
final _urlControlCharacters = RegExp(r'[\u0000-\u001F\u007F-\u009F]');

/// 进程内缓存一次 VS Code 安装探测；widget 测试用覆写注入确定结果。
final vsCodeAvailabilityProvider = FutureProvider<bool>((ref) async {
  return probeVsCodeInstalled();
});

/// 直接启动 VS Code；不经外部 URL 入口，不更改 VS Code 的安全设置。
final vsCodeLauncherProvider = Provider<VsCodeLauncher>(
  (ref) => (folderUri) async {
    final uri = Uri.tryParse(folderUri);
    if (utf8.encode(folderUri).length > _maxVsCodeUrlBytes ||
        _urlControlCharacters.hasMatch(folderUri) ||
        uri == null ||
        uri.hasQuery ||
        uri.hasFragment ||
        !uri.path.startsWith('/') ||
        !(uri.scheme == 'file' ||
            (uri.scheme == 'vscode-remote' &&
                uri.authority.startsWith('ssh-remote+') &&
                uri.authority.length > 'ssh-remote+'.length))) {
      throw ArgumentError.value(
        folderUri,
        'folderUri',
        'Invalid VS Code folder',
      );
    }
    await launchVsCodeFolder(folderUri);
  },
);

/// CLI folder URI; Dart handles drive letters, UNC paths and percent encoding.
String buildLocalVsCodeFolderUri(String absolutePath) {
  return Uri.directory(absolutePath).toString();
}

/// 远端文件夹 URI：`vscode-remote://ssh-remote+<别名>/<远端路径>`。
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
  return 'vscode-remote://ssh-remote+$encodedAlias/$encodedPath';
}
