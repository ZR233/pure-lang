enum RecoveryIssueScope { application, project, thread }

enum RecoveryIssueCategory {
  processLease,
  agentState,
  worktree,
  repository,
  merge,
  conflict,
}

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
    this.taskRunId,
  });

  final String id;
  final RecoveryIssueScope scope;
  final RecoveryIssueCategory category;
  final List<RecoveryIssueAction> availableActions;
  final String? projectId;
  final String? threadId;
  final String? taskRunId;
  final String detail;

  bool get canCleanup =>
      availableActions.contains(RecoveryIssueAction.cleanupThread) ||
      availableActions.contains(RecoveryIssueAction.removeProject);
}

enum RecoveryResourcePresence { absent, complete, partial }

class RecoveryCleanupResource {
  const RecoveryCleanupResource({
    required this.workUnitId,
    required this.path,
    required this.branch,
    required this.presence,
    required this.registrationExists,
    required this.pathExists,
    required this.branchExists,
    required this.dirty,
    required this.aheadBy,
    required this.changedFileCount,
    this.branchHead,
  });

  final String workUnitId;
  final String path;
  final String branch;
  final RecoveryResourcePresence presence;
  final bool registrationExists;
  final bool pathExists;
  final bool branchExists;
  final String? branchHead;
  final bool dirty;
  final int aheadBy;
  final int changedFileCount;

  bool get hasUnmergedWork => dirty || aheadBy > 0 || changedFileCount > 0;
}

class RecoveryCleanupPreview {
  const RecoveryCleanupPreview({
    required this.issueId,
    required this.expectedRevision,
    required this.scope,
    required this.detail,
    required this.resources,
    this.projectId,
    this.threadId,
  });

  final String issueId;
  final String expectedRevision;
  final RecoveryIssueScope scope;
  final String? projectId;
  final String? threadId;
  final String detail;
  final List<RecoveryCleanupResource> resources;

  int get unmergedCommitCount =>
      resources.fold(0, (total, resource) => total + resource.aheadBy);

  int get changedFileCount =>
      resources.fold(0, (total, resource) => total + resource.changedFileCount);
}
