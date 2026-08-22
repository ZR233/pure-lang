import 'studio_enums.dart';

enum ThreadStatusView {
  idle,
  queued,
  running,
  waitingTool,
  waitingInteraction,
  cancelling,
  closing,
  closed,
  faulted;

  bool get isActive => switch (this) {
    queued ||
    running ||
    waitingTool ||
    waitingInteraction ||
    cancelling => true,
    idle || closing || closed || faulted => false,
  };
}

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
    this.status = ThreadStatusView.idle,
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
  final ThreadStatusView status;
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
    ThreadStatusView? status,
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

/// 一次目录分页查询的结果页。
class ThreadDirectoryPage {
  const ThreadDirectoryPage({required this.threads, this.nextCursor});

  final List<StudioThread> threads;
  final String? nextCursor;

  bool get hasMore => nextCursor != null;
}

/// 侧栏会话目录的有界分页窗口。
///
/// 只保留已加载页的条目；触底通过 `nextCursor` 继续加载，目录增量按身份
/// 原位合并（新会话前置、归档移除），未加载条目的增量直接忽略。
class ThreadDirectoryWindow {
  const ThreadDirectoryWindow({
    this.threads = const [],
    this.nextCursor,
    this.hasMore = false,
    this.isLoading = false,
  });

  final List<StudioThread> threads;
  final String? nextCursor;
  final bool hasMore;
  final bool isLoading;

  ThreadDirectoryWindow copyWith({
    List<StudioThread>? threads,
    Object? nextCursor = _sentinel,
    bool? hasMore,
    bool? isLoading,
  }) {
    return ThreadDirectoryWindow(
      threads: threads ?? this.threads,
      nextCursor: identical(nextCursor, _sentinel)
          ? this.nextCursor
          : nextCursor as String?,
      hasMore: hasMore ?? this.hasMore,
      isLoading: isLoading ?? this.isLoading,
    );
  }

  /// 增量合并：已加载条目原位替换；比当前窗口最新条目更新的前置；
  /// 其余（窗口未覆盖的更旧条目）忽略。
  ThreadDirectoryWindow applyDelta({
    required List<StudioThread> upserted,
    required List<String> removed,
  }) {
    if (upserted.isEmpty && removed.isEmpty) {
      return this;
    }
    final removedSet = removed.toSet();
    final upsertedById = {for (final thread in upserted) thread.id: thread};
    final retained = [
      for (final thread in threads)
        if (!removedSet.contains(thread.id))
          upsertedById.remove(thread.id) ?? thread,
    ];
    final newThreads =
        upsertedById.values.where((thread) => !thread.archived).toList()
          ..sort((a, b) => b.updatedAt.compareTo(a.updatedAt));
    final prependable = newThreads.where((thread) {
      if (retained.isEmpty) return true;
      return thread.updatedAt.isAfter(retained.first.updatedAt) ||
          (thread.updatedAt.isAtSameMomentAs(retained.first.updatedAt) &&
              thread.id.compareTo(retained.first.id) > 0);
    }).toList();
    return copyWith(threads: [...prependable, ...retained]);
  }

  ThreadDirectoryWindow appendPage(ThreadDirectoryPage page) {
    final loaded = {...threads.map((thread) => thread.id)};
    final appended = page.threads
        .where((thread) => !loaded.contains(thread.id))
        .toList();
    return copyWith(
      threads: [...threads, ...appended],
      nextCursor: page.nextCursor,
      hasMore: page.hasMore,
      isLoading: false,
    );
  }
}

const Object _sentinel = Object();

/// 关机阶段的只读分类；canonical 状态由 [StudioShutdownProgress] 的 sealed variant 表达。
enum StudioShutdownPhase {
  stoppingSubscriptions,
  cancellingTurns,
  flushingPersistence,
  suspendingTasks,
  stoppingMcp,
  stoppingLsp,
  stopped;

  int get index1 => index + 1;
}

/// 一次关机进度的 canonical 状态；仅持久化刷新状态承载 pending commit 数。
sealed class StudioShutdownProgress {
  const StudioShutdownProgress();

  StudioShutdownPhase get phase => switch (this) {
    StoppingSubscriptionsProgress() =>
      StudioShutdownPhase.stoppingSubscriptions,
    CancellingTurnsProgress() => StudioShutdownPhase.cancellingTurns,
    FlushingPersistenceProgress() => StudioShutdownPhase.flushingPersistence,
    SuspendingTasksProgress() => StudioShutdownPhase.suspendingTasks,
    StoppingMcpProgress() => StudioShutdownPhase.stoppingMcp,
    StoppingLspProgress() => StudioShutdownPhase.stoppingLsp,
    StoppedProgress() => StudioShutdownPhase.stopped,
  };
}

final class StoppingSubscriptionsProgress extends StudioShutdownProgress {
  const StoppingSubscriptionsProgress();
}

final class CancellingTurnsProgress extends StudioShutdownProgress {
  const CancellingTurnsProgress();
}

final class FlushingPersistenceProgress extends StudioShutdownProgress {
  const FlushingPersistenceProgress({required this.pendingCommits});

  final int pendingCommits;
}

final class SuspendingTasksProgress extends StudioShutdownProgress {
  const SuspendingTasksProgress();
}

final class StoppingMcpProgress extends StudioShutdownProgress {
  const StoppingMcpProgress();
}

final class StoppingLspProgress extends StudioShutdownProgress {
  const StoppingLspProgress();
}

final class StoppedProgress extends StudioShutdownProgress {
  const StoppedProgress();
}
