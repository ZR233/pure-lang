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
}
