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

  bool get canRetry => availableActions.contains(RecoveryIssueAction.retry);
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

enum ConversationRecoveryMode { rewindTail, rebuildThread }

enum TaskRecoveryTargetKind { planner, executor }

class TaskGitFingerprint {
  const TaskGitFingerprint({
    required this.workspaceRoot,
    required this.gitCommonDir,
    required this.branch,
    required this.head,
    required this.baseCommit,
    required this.expectedHead,
    required this.operation,
    required this.indexDiffHash,
    required this.workingTreeDiffHash,
    required this.untrackedContentHash,
  });

  final String workspaceRoot;
  final String gitCommonDir;
  final String branch;
  final String head;
  final String baseCommit;
  final String expectedHead;
  final String operation;
  final String indexDiffHash;
  final String workingTreeDiffHash;
  final String untrackedContentHash;
}

class TaskRecoveryTurn {
  const TaskRecoveryTurn({
    required this.turnId,
    required this.status,
    required this.updatedAt,
    required this.itemCount,
    required this.inputCount,
    required this.toolCount,
    required this.toolSummaries,
  });

  final String turnId;
  final String status;
  final DateTime updatedAt;
  final int itemCount;
  final int inputCount;
  final int toolCount;
  final List<String> toolSummaries;
}

class TaskRecoveryTarget {
  const TaskRecoveryTarget({
    required this.threadId,
    required this.kind,
    required this.expectedRuntimeRevision,
    required this.expectedThreadRevision,
    required this.branch,
    required this.worktreePath,
    required this.turns,
    required this.defaultTurnIds,
    required this.availableModes,
    required this.gitFingerprint,
    this.workUnitId,
    this.attempt,
    this.continuationRevision,
  });

  final String threadId;
  final TaskRecoveryTargetKind kind;
  final String? workUnitId;
  final int? attempt;
  final int? continuationRevision;
  final int expectedRuntimeRevision;
  final int expectedThreadRevision;
  final String branch;
  final String worktreePath;
  final List<TaskRecoveryTurn> turns;
  final List<String> defaultTurnIds;
  final List<ConversationRecoveryMode> availableModes;
  final TaskGitFingerprint gitFingerprint;
}

class TaskRecoveryPreview {
  const TaskRecoveryPreview({
    required this.previewToken,
    required this.rootThreadId,
    required this.runId,
    required this.taskGeneration,
    required this.phase,
    required this.expectedHead,
    required this.stopRequested,
    required this.branchLeaseId,
    required this.branchLeaseBranch,
    required this.branchLeaseGitCommonDir,
    required this.branchLeaseExpectedHead,
    required this.recommendedThreadId,
    required this.targets,
    required this.mainGitFingerprint,
    required this.completionRevisionFingerprint,
    required this.reviewRevisionFingerprint,
    required this.mergeRevisionFingerprint,
  });

  final String previewToken;
  final String rootThreadId;
  final String runId;
  final int taskGeneration;
  final String phase;
  final String expectedHead;
  final bool stopRequested;
  final String branchLeaseId;
  final String branchLeaseBranch;
  final String branchLeaseGitCommonDir;
  final String branchLeaseExpectedHead;
  final String recommendedThreadId;
  final List<TaskRecoveryTarget> targets;
  final TaskGitFingerprint mainGitFingerprint;
  final String completionRevisionFingerprint;
  final String reviewRevisionFingerprint;
  final String mergeRevisionFingerprint;

  TaskRecoveryTarget target(String threadId) =>
      targets.firstWhere((candidate) => candidate.threadId == threadId);

  TaskRecoveryTarget get recommendedTarget => target(recommendedThreadId);
}

class TaskRecoveryRequest {
  const TaskRecoveryRequest({
    required this.recoveryId,
    required this.rootThreadId,
    required this.targetThreadId,
    required this.mode,
    required this.turnIds,
    required this.preview,
  });

  final String recoveryId;
  final String rootThreadId;
  final String targetThreadId;
  final ConversationRecoveryMode mode;
  final List<String> turnIds;
  final TaskRecoveryPreview preview;
}

class TaskRecoveryResult {
  const TaskRecoveryResult({
    required this.recoveryId,
    required this.runId,
    required this.rootThreadId,
    required this.targetThreadId,
    required this.mode,
    required this.recoveryRevision,
    required this.runtimeRevision,
    required this.threadRevision,
    required this.beforeTranscriptHash,
    required this.afterTranscriptHash,
    required this.removedItemCount,
    required this.removedInputCount,
    required this.stopCleared,
    required this.resumeTurnId,
    required this.gitFingerprint,
    this.workUnitId,
  });

  final String recoveryId;
  final String runId;
  final String? workUnitId;
  final String rootThreadId;
  final String targetThreadId;
  final ConversationRecoveryMode mode;
  final int recoveryRevision;
  final int runtimeRevision;
  final int threadRevision;
  final String beforeTranscriptHash;
  final String afterTranscriptHash;
  final int removedItemCount;
  final int removedInputCount;
  final bool stopCleared;
  final String resumeTurnId;
  final TaskGitFingerprint gitFingerprint;
}
