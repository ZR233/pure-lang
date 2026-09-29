Future<bool> probeZedInstalled() async => false;

Future<void> launchZedWorkspace(String workspaceArgument) =>
    Future.error(UnsupportedError('Zed requires a desktop host'));
