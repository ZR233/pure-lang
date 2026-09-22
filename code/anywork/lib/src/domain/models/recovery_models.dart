enum RecoveryIssueScope { application, project, thread }

enum RecoveryIssueCategory { processLease, agentState, repository, storage }

enum RecoveryIssueAction {
  retry,
  cleanupThread,
  removeProject,
  cleanupWorktree,
}

/// Worktree lease 归属来源；显式清理必须与 durable lease 的归属一致。
enum WorktreeOwnerKind {
  session,
  child;

  /// Canonical wire 值：`session` | `child`。
  String get id => name;

  bool get isSession => this == WorktreeOwnerKind.session;
}

class WorktreeRecoveryPreview {
  const WorktreeRecoveryPreview({
    required this.ownerKind,
    required this.ownerThreadId,
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

  final WorktreeOwnerKind ownerKind;
  final String ownerThreadId;
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
