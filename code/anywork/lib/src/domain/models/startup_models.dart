enum StudioStartupPhase {
  preparing,
  waitingForSteps,
  closingResources,
  backingUp,
  resetting,
  startingServices,
  loadingBridge,
  openingStorage,
  loadingConfiguration,
  readingProjects,
  preparingResources,
  readingState,
  ready,
  failed,
}

class StartupRecoveryReport {
  const StartupRecoveryReport({
    required this.backupPath,
    required this.reason,
    required this.createdAt,
  });
  final String backupPath;
  final String reason;
  final int createdAt;
}
