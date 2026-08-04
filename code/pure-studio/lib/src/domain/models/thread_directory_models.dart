import 'studio_enums.dart';

class StudioProject {
  const StudioProject({
    required this.id,
    required this.name,
    required this.path,
  });

  final String id;
  final String name;
  final String path;
}

class StudioThread {
  const StudioThread({
    required this.id,
    required this.projectId,
    required this.title,
    required this.mode,
    required this.updatedAt,
    this.createdAt,
    this.parentThreadId,
    this.rootThreadId = '',
    this.agentPath = '',
    this.role = 'planner',
    this.status = 'idle',
    this.archived = false,
  });

  final String id;
  final String projectId;
  final String title;
  final StudioMode mode;
  final DateTime? createdAt;
  final DateTime updatedAt;
  final String? parentThreadId;
  final String rootThreadId;
  final String agentPath;
  final String role;
  final String status;
  final bool archived;

  bool get isRoot => parentThreadId == null;

  bool get isAgent => parentThreadId != null;

  String get effectiveRootThreadId => rootThreadId.isEmpty ? id : rootThreadId;

  DateTime get effectiveCreatedAt => createdAt ?? updatedAt;

  StudioThread copyWith({
    String? title,
    StudioMode? mode,
    DateTime? createdAt,
    DateTime? updatedAt,
    String? parentThreadId,
    String? rootThreadId,
    String? agentPath,
    String? role,
    String? status,
    bool? archived,
  }) {
    return StudioThread(
      id: id,
      projectId: projectId,
      title: title ?? this.title,
      mode: mode ?? this.mode,
      createdAt: createdAt ?? this.createdAt,
      updatedAt: updatedAt ?? this.updatedAt,
      parentThreadId: parentThreadId ?? this.parentThreadId,
      rootThreadId: rootThreadId ?? this.rootThreadId,
      agentPath: agentPath ?? this.agentPath,
      role: role ?? this.role,
      status: status ?? this.status,
      archived: archived ?? this.archived,
    );
  }
}
