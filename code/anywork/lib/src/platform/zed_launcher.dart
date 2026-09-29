import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'zed_host.dart';

sealed class ZedWorkspaceTarget {
  const ZedWorkspaceTarget();
}

class LocalZedWorkspaceTarget extends ZedWorkspaceTarget {
  const LocalZedWorkspaceTarget({required this.directory});

  final String directory;
}

class RemoteSshZedWorkspaceTarget extends ZedWorkspaceTarget {
  const RemoteSshZedWorkspaceTarget({
    required this.alias,
    required this.remotePath,
  });

  final String alias;
  final String remotePath;
}

typedef ZedLauncher = Future<void> Function(ZedWorkspaceTarget target);

const _maxZedArgumentBytes = 8 * 1024;
final _controlCharacters = RegExp(r'[\u0000-\u001F\u007F-\u009F]');
final _invalidSshAliasCharacters = RegExp(r'''[\s"';/@:#?\\]''');

/// 进程内缓存一次 Zed 安装探测；demo 与界面验收可覆写注入确定结果。
final zedAvailabilityProvider = FutureProvider<bool>((ref) async {
  return probeZedInstalled();
});

/// 启动宿主 Zed；工作区目标先转换为单个、经过白名单校验的参数。
final zedLauncherProvider = Provider<ZedLauncher>(
  (ref) => (target) async {
    final argument = buildZedWorkspaceArgument(target);
    if (utf8.encode(argument).length > _maxZedArgumentBytes) {
      throw ArgumentError.value(argument, 'target', 'Zed target is too long');
    }
    await launchZedWorkspace(argument);
  },
);

/// Zed 本地目录直接使用 canonical 路径；远端目录使用其官方 SSH URI。
String buildZedWorkspaceArgument(ZedWorkspaceTarget target) {
  return switch (target) {
    LocalZedWorkspaceTarget(directory: final directory) =>
      _validatedLocalDirectory(directory),
    RemoteSshZedWorkspaceTarget(
      alias: final alias,
      remotePath: final remotePath,
    ) =>
      _remoteSshUri(alias: alias, remotePath: remotePath),
  };
}

String _validatedLocalDirectory(String directory) {
  final uri = Uri.directory(directory);
  if (directory.trim().isEmpty ||
      _controlCharacters.hasMatch(directory) ||
      uri.scheme != 'file' ||
      uri.hasQuery ||
      uri.hasFragment) {
    throw ArgumentError.value(directory, 'directory', 'Invalid workspace');
  }
  return directory;
}

String _remoteSshUri({required String alias, required String remotePath}) {
  if (alias.isEmpty ||
      alias.startsWith('-') ||
      _invalidSshAliasCharacters.hasMatch(alias) ||
      _controlCharacters.hasMatch(alias)) {
    throw ArgumentError.value(alias, 'alias', 'Invalid SSH host alias');
  }
  if (!remotePath.startsWith('/') ||
      remotePath.contains(r'\') ||
      _controlCharacters.hasMatch(remotePath)) {
    throw ArgumentError.value(
      remotePath,
      'remotePath',
      'Invalid remote workspace',
    );
  }
  final encodedAlias = Uri.encodeComponent(alias);
  final encodedPath = remotePath.split('/').map(Uri.encodeComponent).join('/');
  return 'ssh://$encodedAlias$encodedPath';
}
