class StudioAgentView {
  const StudioAgentView({
    required this.id,
    required this.sessionId,
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
  });

  final String id;
  final String sessionId;
  final String path;
  final String? parentPath;
  final String role;
  final String task;
  final String status;
  final String? summary;
  final int depth;
  final String? error;
  final String? reason;
  final DateTime updatedAt;
}
