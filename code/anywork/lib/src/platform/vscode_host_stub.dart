Future<bool> probeVsCodeInstalled() async => false;

Future<void> launchVsCodeFolder(String folderUri) =>
    Future.error(UnsupportedError('VS Code requires a desktop host'));
