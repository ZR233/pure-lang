class ConfigRecoveryNotice {
  const ConfigRecoveryNotice({required this.backupPath});

  final String backupPath;
}

enum RecoveryIssueScope { application, project, thread }

enum RecoveryIssueCategory { processLease, agentState, repository }

enum RecoveryIssueAction { retry, cleanupThread, removeProject }

class StudioRecoveryIssue {
  const StudioRecoveryIssue({
    required this.id,
    required this.scope,
    required this.category,
    required this.availableActions,
    required this.detail,
    this.projectId,
    this.threadId,
  });

  final String id;
  final RecoveryIssueScope scope;
  final RecoveryIssueCategory category;
  final List<RecoveryIssueAction> availableActions;
  final String? projectId;
  final String? threadId;
  final String detail;

  bool get canCleanup =>
      availableActions.contains(RecoveryIssueAction.cleanupThread) ||
      availableActions.contains(RecoveryIssueAction.removeProject);

  bool get canRetry => availableActions.contains(RecoveryIssueAction.retry);
}
