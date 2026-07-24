import 'studio_enums.dart';

enum StudioSessionKind { root, agent }

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

class StudioSession {
  const StudioSession({
    required this.id,
    required this.projectId,
    required this.title,
    required this.mode,
    required this.updatedAt,
    this.createdAt,
    this.visibility = 'active',
    this.parentSessionId,
    this.rootSessionId = '',
    this.sessionKind = StudioSessionKind.root,
    this.ownerAgentId = '',
    this.ownerRole = 'planner',
    this.agentStatus = 'idle',
    this.agentSummary,
    this.agentError,
    this.agentUpdatedAt,
  });

  final String id;
  final String projectId;
  final String title;
  final StudioMode mode;
  final DateTime? createdAt;
  final DateTime updatedAt;
  final String visibility;
  final String? parentSessionId;
  final String rootSessionId;
  final StudioSessionKind sessionKind;
  final String ownerAgentId;
  final String ownerRole;
  final String agentStatus;
  final String? agentSummary;
  final String? agentError;
  final DateTime? agentUpdatedAt;

  bool get isRoot => sessionKind == StudioSessionKind.root;

  bool get isAgent => sessionKind == StudioSessionKind.agent;

  String get effectiveRootSessionId => rootSessionId.isEmpty ? id : rootSessionId;

  DateTime get effectiveCreatedAt => createdAt ?? updatedAt;

  StudioSession copyWith({
    String? title,
    StudioMode? mode,
    DateTime? createdAt,
    DateTime? updatedAt,
    String? visibility,
    String? parentSessionId,
    String? rootSessionId,
    StudioSessionKind? sessionKind,
    String? ownerAgentId,
    String? ownerRole,
    String? agentStatus,
    String? agentSummary,
    String? agentError,
    DateTime? agentUpdatedAt,
  }) {
    return StudioSession(
      id: id,
      projectId: projectId,
      title: title ?? this.title,
      mode: mode ?? this.mode,
      createdAt: createdAt ?? this.createdAt,
      updatedAt: updatedAt ?? this.updatedAt,
      visibility: visibility ?? this.visibility,
      parentSessionId: parentSessionId ?? this.parentSessionId,
      rootSessionId: rootSessionId ?? this.rootSessionId,
      sessionKind: sessionKind ?? this.sessionKind,
      ownerAgentId: ownerAgentId ?? this.ownerAgentId,
      ownerRole: ownerRole ?? this.ownerRole,
      agentStatus: agentStatus ?? this.agentStatus,
      agentSummary: agentSummary ?? this.agentSummary,
      agentError: agentError ?? this.agentError,
      agentUpdatedAt: agentUpdatedAt ?? this.agentUpdatedAt,
    );
  }
}
