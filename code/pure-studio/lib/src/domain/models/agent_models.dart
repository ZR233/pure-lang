class StudioAgentView {
  const StudioAgentView({
    required this.id,
    required this.threadId,
    required this.path,
    required this.role,
    required this.task,
    required this.status,
    required this.updatedAt,
    this.parentPath,
    this.summary,
    this.depth = 0,
    this.error,
    this.reason,
    this.rootThreadId,
    this.lifecycle,
    this.activity,
    this.progress,
    this.summaryAgeSeconds,
  });

  final String id;
  final String threadId;
  final String path;
  final String? parentPath;
  final String role;
  final String task;
  final String status;
  final String? summary;
  final int depth;
  final String? error;
  final String? reason;
  final String? rootThreadId;
  final String? lifecycle;
  final String? activity;
  final AgentProgressView? progress;
  final int? summaryAgeSeconds;
  final DateTime updatedAt;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is StudioAgentView &&
          id == other.id &&
          threadId == other.threadId &&
          path == other.path &&
          parentPath == other.parentPath &&
          role == other.role &&
          task == other.task &&
          status == other.status &&
          summary == other.summary &&
          depth == other.depth &&
          error == other.error &&
          reason == other.reason &&
          rootThreadId == other.rootThreadId &&
          lifecycle == other.lifecycle &&
          activity == other.activity &&
          progress == other.progress &&
          summaryAgeSeconds == other.summaryAgeSeconds &&
          updatedAt == other.updatedAt;

  @override
  int get hashCode => Object.hash(
    id,
    threadId,
    path,
    parentPath,
    role,
    task,
    status,
    summary,
    depth,
    error,
    reason,
    rootThreadId,
    lifecycle,
    activity,
    progress,
    summaryAgeSeconds,
    updatedAt,
  );
}

class AgentProgressView {
  const AgentProgressView({
    required this.stage,
    required this.summary,
    required this.nextStep,
    required this.revision,
    required this.updatedAt,
  });

  final String stage;
  final String summary;
  final String nextStep;
  final int revision;
  final DateTime updatedAt;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is AgentProgressView &&
          stage == other.stage &&
          summary == other.summary &&
          nextStep == other.nextStep &&
          revision == other.revision &&
          updatedAt == other.updatedAt;

  @override
  int get hashCode =>
      Object.hash(stage, summary, nextStep, revision, updatedAt);
}
