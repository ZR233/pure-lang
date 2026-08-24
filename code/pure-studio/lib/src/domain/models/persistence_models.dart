import 'studio_state_snapshots.dart';

/// 持久化 owner 发布的单调快照。
class PersistenceStateSnapshot {
  const PersistenceStateSnapshot({required this.revision, required this.state});

  const PersistenceStateSnapshot.ready()
    : revision = 0,
      state = const ReadyPersistenceState(pendingCommits: 0);

  final int revision;
  final PersistenceState state;

  bool get acceptsNewWork => state.acceptsNewWork;
}

sealed class PersistenceState {
  const PersistenceState();

  int get pendingCommits;
  int? get oldestPendingRevision;
  int? get firstFailedAt => null;
  ObservedResourceError? get error => null;
  bool get acceptsNewWork =>
      this is ReadyPersistenceState || this is FlushingPersistenceState;
}

final class ReadyPersistenceState extends PersistenceState {
  const ReadyPersistenceState({required this.pendingCommits});

  @override
  final int pendingCommits;
  @override
  int? get oldestPendingRevision => null;
}

final class FlushingPersistenceState extends PersistenceState {
  const FlushingPersistenceState({
    required this.pendingCommits,
    required this.oldestPendingRevision,
  });

  @override
  final int pendingCommits;
  @override
  final int? oldestPendingRevision;
}

final class DegradedPersistenceState extends PersistenceState {
  const DegradedPersistenceState({
    required this.pendingCommits,
    required this.oldestPendingRevision,
    required this.firstFailedAt,
    required this.error,
  });

  @override
  final int pendingCommits;
  @override
  final int? oldestPendingRevision;
  @override
  final int firstFailedAt;
  @override
  final ObservedResourceError error;
}

final class RecoveringPersistenceState extends PersistenceState {
  const RecoveringPersistenceState({
    required this.pendingCommits,
    required this.oldestPendingRevision,
    required this.firstFailedAt,
  });

  @override
  final int pendingCommits;
  @override
  final int? oldestPendingRevision;
  @override
  final int firstFailedAt;
}

final class BlockedPersistenceState extends PersistenceState {
  const BlockedPersistenceState({
    required this.pendingCommits,
    required this.oldestPendingRevision,
    required this.firstFailedAt,
    required this.error,
  });

  @override
  final int pendingCommits;
  @override
  final int? oldestPendingRevision;
  @override
  final int firstFailedAt;
  @override
  final ObservedResourceError error;
}
