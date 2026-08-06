import 'package:flutter/foundation.dart' show listEquals;

class ThreadRuntimeView {
  const ThreadRuntimeView({
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

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is ThreadRuntimeView &&
            model == other.model &&
            contextTokens == other.contextTokens &&
            contextWindow == other.contextWindow &&
            totalTokens == other.totalTokens &&
            costLabel == other.costLabel &&
            listEquals(activeSkills, other.activeSkills) &&
            listEquals(activeMcpServers, other.activeMcpServers) &&
            listEquals(activeLspServers, other.activeLspServers) &&
            agentCount == other.agentCount &&
            task == other.task;
  }

  @override
  int get hashCode => Object.hash(
    model,
    contextTokens,
    contextWindow,
    totalTokens,
    costLabel,
    Object.hashAll(activeSkills),
    Object.hashAll(activeMcpServers),
    Object.hashAll(activeLspServers),
    agentCount,
    task,
  );

  ThreadRuntimeView copyWith({
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
    return ThreadRuntimeView(
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
    required this.completions,
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
  final List<TaskCompletionView> completions;
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

class TaskCompletionView {
  const TaskCompletionView({
    required this.id,
    required this.workUnitId,
    required this.executorAgentId,
    required this.revision,
    required this.kind,
    required this.status,
    required this.baseCommit,
    required this.headCommit,
    required this.changedFiles,
    required this.verificationSummary,
    required this.worktreePath,
    required this.branch,
    required this.createdAt,
    required this.updatedAt,
  });

  final String id;
  final String workUnitId;
  final String executorAgentId;
  final int revision;
  final String kind;
  final String status;
  final String baseCommit;
  final String? headCommit;
  final List<String> changedFiles;
  final String verificationSummary;
  final String worktreePath;
  final String branch;
  final DateTime createdAt;
  final DateTime updatedAt;
}

class TaskMergeView {
  const TaskMergeView({
    required this.id,
    required this.workUnitId,
    required this.completionId,
    required this.completionRevision,
    required this.executorAgentId,
    required this.expectedPreviousHead,
    required this.resultingHead,
    required this.deliveryHead,
    required this.method,
    required this.summary,
    required this.cleanupStatus,
    required this.cleanupDetail,
    required this.createdAt,
    required this.updatedAt,
  });

  final String id;
  final String workUnitId;
  final String completionId;
  final int completionRevision;
  final String executorAgentId;
  final String expectedPreviousHead;
  final String resultingHead;
  final String deliveryHead;
  final String method;
  final String summary;
  final String cleanupStatus;
  final String? cleanupDetail;
  final DateTime createdAt;
  final DateTime updatedAt;
}

class TaskReviewView {
  const TaskReviewView({
    required this.id,
    required this.round,
    required this.scope,
    required this.workUnitId,
    required this.completionId,
    required this.completionRevision,
    required this.reviewedHead,
    required this.verdict,
    required this.requestedByCallId,
    required this.reviewerAgentId,
    required this.summary,
    required this.designReferences,
    required this.findings,
    required this.createdAt,
    required this.updatedAt,
  });

  final String id;
  final int round;
  final String scope;
  final String? workUnitId;
  final String? completionId;
  final int? completionRevision;
  final String reviewedHead;
  final String verdict;
  final String requestedByCallId;
  final String? reviewerAgentId;
  final String? summary;
  final List<TaskDesignReferenceView> designReferences;
  final List<TaskReviewFindingView> findings;
  final DateTime createdAt;
  final DateTime updatedAt;
}

class TaskDesignReferenceView {
  const TaskDesignReferenceView({required this.path, required this.section});

  final String path;
  final String section;
}

class TaskReviewFindingView {
  const TaskReviewFindingView({
    required this.severity,
    required this.title,
    required this.body,
    required this.path,
    required this.line,
    required this.designReferences,
  });

  final String severity;
  final String title;
  final String body;
  final String? path;
  final int? line;
  final List<TaskDesignReferenceView> designReferences;
}
