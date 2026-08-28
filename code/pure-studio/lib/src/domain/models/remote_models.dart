enum SshAuthKind { agentOrKey, password }

class SshServer {
  const SshServer({
    required this.id,
    required this.name,
    required this.host,
    required this.port,
    required this.username,
    required this.authKind,
    this.identityFile,
  });

  final String id;
  final String name;
  final String host;
  final int port;
  final String username;
  final SshAuthKind authKind;
  final String? identityFile;
}

class SaveSshServerCommand {
  const SaveSshServerCommand({
    this.id,
    required this.name,
    required this.host,
    required this.port,
    required this.username,
    required this.authKind,
    this.identityFile,
    this.password,
  });

  final String? id;
  final String name;
  final String host;
  final int port;
  final String username;
  final SshAuthKind authKind;
  final String? identityFile;
  final String? password;
}

class SshConnectionView {
  const SshConnectionView({
    required this.serverId,
    required this.state,
    this.helperVersion,
    this.architecture,
    this.attempt,
    this.delaySeconds,
    this.errorCode,
    this.errorMessage,
  });

  final String serverId;
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
