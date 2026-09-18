/// `~/.ssh/config` 中的一个 Host 条目；别名即服务器身份。
class SshServer {
  const SshServer({
    required this.alias,
    required this.hostName,
    required this.port,
    required this.username,
    required this.managed,
    this.identityFile,
  });

  final String alias;
  final String hostName;
  final int port;
  final String username;
  final String? identityFile;

  /// 是否为 anywork 管理块；用户手写条目只读，不可编辑或删除。
  final bool managed;
}

class SaveSshServerCommand {
  const SaveSshServerCommand({
    required this.alias,
    required this.hostName,
    required this.port,
    required this.username,
    this.identityFile,
  });

  final String alias;
  final String hostName;
  final int port;
  final String username;
  final String? identityFile;
}

class SshConnectionView {
  const SshConnectionView({
    required this.alias,
    required this.state,
    this.helperVersion,
    this.architecture,
    this.attempt,
    this.delaySeconds,
    this.errorCode,
    this.errorMessage,
  });

  final String alias;
  final String state;
  final String? helperVersion;
  final String? architecture;
  final int? attempt;
  final int? delaySeconds;
  final String? errorCode;
  final String? errorMessage;
}

class RemoteDirectoryListing {
  const RemoteDirectoryListing({
    required this.path,
    required this.entries,
    this.parent,
  });

  final String path;
  final String? parent;
  final List<RemoteDirectoryEntry> entries;
}

class RemoteDirectoryEntry {
  const RemoteDirectoryEntry({required this.name, required this.path});
  final String name;
  final String path;
}
