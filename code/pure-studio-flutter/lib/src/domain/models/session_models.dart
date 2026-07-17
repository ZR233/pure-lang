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

class StudioSession {
  const StudioSession({
    required this.id,
    required this.projectId,
    required this.title,
    required this.mode,
    required this.updatedAt,
  });

  final String id;
  final String projectId;
  final String title;
  final StudioMode mode;
  final DateTime updatedAt;

  StudioSession copyWith({
    String? title,
    StudioMode? mode,
    DateTime? updatedAt,
  }) {
    return StudioSession(
      id: id,
      projectId: projectId,
      title: title ?? this.title,
      mode: mode ?? this.mode,
      updatedAt: updatedAt ?? this.updatedAt,
    );
  }
}
