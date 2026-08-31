class ConfigRecoveryNotice {
  const ConfigRecoveryNotice({required this.backupPath});

  final String backupPath;
}

enum RecoveryIssueScope { application, project, thread }

enum RecoveryIssueCategory { processLease, agentState, repository }

enum RecoveryIssueAction {
  retry,
  cleanupThread,
  removeProject,
  cleanupWorktree,
}

class WorktreeRecoveryPreview {
  const WorktreeRecoveryPreview({
    required this.childId,
    required this.leaseRevision,
    required this.state,
    required this.repositoryRoot,
    required this.path,
    required this.branch,
    required this.baseCommit,
    required this.headCommit,
    required this.dirty,
    required this.changedFiles,
  });

  final String childId;
  final int leaseRevision;
  final String state;
  final String repositoryRoot;
  final String path;
  final String branch;
  final String baseCommit;
  final String? headCommit;
  final bool dirty;
  final List<String> changedFiles;
}

class StudioRecoveryIssue {
  const StudioRecoveryIssue({
    required this.id,
    required this.scope,
    required this.category,
    required this.availableActions,
    required this.detail,
    this.projectId,
    this.threadId,
    this.worktree,
  });

  final String id;
  final RecoveryIssueScope scope;
  final RecoveryIssueCategory category;
  final List<RecoveryIssueAction> availableActions;
  final String? projectId;
  final String? threadId;
  final String detail;
  final WorktreeRecoveryPreview? worktree;

  bool get canCleanup =>
      availableActions.contains(RecoveryIssueAction.cleanupThread) ||
      availableActions.contains(RecoveryIssueAction.removeProject) ||
      availableActions.contains(RecoveryIssueAction.cleanupWorktree);

  bool get canRetry => availableActions.contains(RecoveryIssueAction.retry);
}
