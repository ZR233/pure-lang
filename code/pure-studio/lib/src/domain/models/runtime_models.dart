class SessionRuntimeView {
  const SessionRuntimeView({
    required this.model,
    required this.contextTokens,
    required this.contextWindow,
    required this.totalTokens,
    required this.costLabel,
    required this.activeSkills,
    required this.activeMcpServers,
    required this.activeLspServers,
    required this.agentCount,
    this.task,
  });

  final String model;
  final int contextTokens;
  final int contextWindow;
  final int totalTokens;
  final String costLabel;
  final List<String> activeSkills;
  final List<String> activeMcpServers;
  final List<String> activeLspServers;
  final int agentCount;
  final TaskRuntimeView? task;

  bool get hasActiveTask => task?.isActive ?? false;

  SessionRuntimeView copyWith({
    String? model,
    int? contextTokens,
    int? contextWindow,
    int? totalTokens,
    String? costLabel,
    List<String>? activeSkills,
    List<String>? activeMcpServers,
    List<String>? activeLspServers,
    int? agentCount,
    TaskRuntimeView? task,
  }) {
    return SessionRuntimeView(
      model: model ?? this.model,
      contextTokens: contextTokens ?? this.contextTokens,
      contextWindow: contextWindow ?? this.contextWindow,
      totalTokens: totalTokens ?? this.totalTokens,
      costLabel: costLabel ?? this.costLabel,
      activeSkills: activeSkills ?? this.activeSkills,
      activeMcpServers: activeMcpServers ?? this.activeMcpServers,
      activeLspServers: activeLspServers ?? this.activeLspServers,
      agentCount: agentCount ?? this.agentCount,
      task: task ?? this.task,
    );
  }
}

class TaskRuntimeView {
  const TaskRuntimeView({
    required this.runId,
    required this.phase,
    required this.branch,
    required this.expectedHead,
    required this.statusMessage,
    required this.stopRequestedOrigin,
    required this.stopRequestedReason,
    required this.taskGeneration,
    required this.workUnits,
    required this.agents,
    required this.merges,
    required this.reviews,
  });

  final String runId;
  final String phase;
  final String branch;
  final String expectedHead;
  final String? statusMessage;
  final String? stopRequestedOrigin;
  final String? stopRequestedReason;
  final int taskGeneration;
  final List<TaskWorkUnitView> workUnits;
  final List<TaskAgentOutcomeView> agents;
  final List<TaskMergeView> merges;
  final List<TaskReviewView> reviews;

  bool get isActive =>
      !const {'completed', 'blocked', 'failed', 'cancelled'}.contains(phase);
}

class TaskWorkUnitView {
  const TaskWorkUnitView({
    required this.id,
    required this.title,
    required this.status,
    required this.worktreePath,
    required this.branch,
    required this.agentId,
  });

  final String id;
  final String title;
  final String status;
  final String worktreePath;
  final String branch;
  final String? agentId;
}

class TaskAgentOutcomeView {
  const TaskAgentOutcomeView({
    required this.agentId,
    required this.role,
    required this.status,
    required this.initiatedBy,
    required this.requestedByCallId,
    required this.summary,
    required this.error,
    required this.headCommit,
  });

  final String agentId;
  final String role;
  final String status;
  final String initiatedBy;
  final String requestedByCallId;
  final String? summary;
  final String? error;
  final String? headCommit;
}

class TaskMergeView {
  const TaskMergeView({
    required this.id,
    required this.agentId,
    required this.status,
    required this.mergeCommit,
    required this.conflictFiles,
    required this.resolutionSummary,
  });

  final String id;
  final String agentId;
  final String status;
  final String? mergeCommit;
  final List<String> conflictFiles;
  final String? resolutionSummary;
}

class TaskReviewView {
  const TaskReviewView({
    required this.round,
    required this.headCommit,
    required this.verdict,
    required this.reviewerAgentId,
    required this.summary,
    required this.designReferences,
  });

  final int round;
  final String headCommit;
  final String verdict;
  final String? reviewerAgentId;
  final String? summary;
  final List<String> designReferences;
}
