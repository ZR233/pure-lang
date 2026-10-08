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
    this.sshAlias,
  });

  final String id;
  final String name;
  final String path;
  final String? sshAlias;
}

class StudioThread {
  const StudioThread({
    required this.id,
    required this.projectId,
    required this.title,
    required this.mode,
    required this.updatedAt,
    this.createdAt,
    this.lastUserMessageAt,
    this.parentThreadId,
    this.rootThreadId = '',
    this.agentPath = '',
    this.role = 'planner',
    this.status = ThreadStatusView.idle,
    this.archived = false,
    this.workspaceMode = ThreadWorkspaceMode.local,
    required this.workspacePath,
  });

  final String id;
  final String projectId;
  final String title;
  final ThreadModeId mode;
  final DateTime? createdAt;
  final DateTime updatedAt;

  /// 服务端最近一次成功受理用户消息的时间；null 表示尚未有已受理用户消息。
  ///
  /// 只由成功受理推进；模型回复、工具、运行状态、标题与配置更新不改变它。
  final DateTime? lastUserMessageAt;
  final String? parentThreadId;
  final String rootThreadId;
  final String agentPath;
  final String role;
  final ThreadStatusView status;
  final bool archived;

  /// Canonical 会话工作区模式，只读来自 Thread 目录事实。
  final ThreadWorkspaceMode workspaceMode;

  /// Canonical 会话工作区地址；`local` 为项目目录，`worktree` 为工作树路径。
  ///
  /// 唯一决定会话级外部「打开工作区」入口（VS Code、Zed 与终端）的打开目标，
  /// GUI 不推导工作树布局。
  final String workspacePath;

  bool get isRoot => parentThreadId == null;

  bool get isAgent => parentThreadId != null;

  /// The former planner child role is retained for history, never execution.
  bool get isRetiredAgent => isAgent && role == 'planner';

  String get effectiveRootThreadId => rootThreadId.isEmpty ? id : rootThreadId;

  DateTime get effectiveCreatedAt => createdAt ?? updatedAt;

  /// 会话目录排序键：最近用户消息时间，缺失时回落到创建时间（design/19 §19.1）。
  DateTime get directorySortTime => lastUserMessageAt ?? effectiveCreatedAt;

  /// 会话目录的 canonical 降序比较器：目录排序键降序，同秒按 ID 降序破平。
  static int compareDirectoryOrder(StudioThread a, StudioThread b) {
    final time = b.directorySortTime.compareTo(a.directorySortTime);
    return time != 0 ? time : b.id.compareTo(a.id);
  }

  StudioThread copyWith({
    String? title,
    ThreadModeId? mode,
    DateTime? createdAt,
    DateTime? lastUserMessageAt,
    DateTime? updatedAt,
    String? parentThreadId,
    String? rootThreadId,
    String? agentPath,
    String? role,
    ThreadStatusView? status,
    bool? archived,
    ThreadWorkspaceMode? workspaceMode,
    String? workspacePath,
  }) {
    return StudioThread(
      id: id,
      projectId: projectId,
      title: title ?? this.title,
      mode: mode ?? this.mode,
      createdAt: createdAt ?? this.createdAt,
      lastUserMessageAt: lastUserMessageAt ?? this.lastUserMessageAt,
      updatedAt: updatedAt ?? this.updatedAt,
      parentThreadId: parentThreadId ?? this.parentThreadId,
      rootThreadId: rootThreadId ?? this.rootThreadId,
      agentPath: agentPath ?? this.agentPath,
      role: role ?? this.role,
      status: status ?? this.status,
      archived: archived ?? this.archived,
      workspaceMode: workspaceMode ?? this.workspaceMode,
      workspacePath: workspacePath ?? this.workspacePath,
    );
  }
}

/// 一次目录分页查询的结果页；`revision` 是目录领域水位。
class ThreadDirectoryPage {
  const ThreadDirectoryPage({
    required this.threads,
    this.nextCursor,
    this.revision = 0,
  });

  final List<StudioThread> threads;
  final String? nextCursor;
  final int revision;

  bool get hasMore => nextCursor != null;
}

/// 侧栏会话目录的有界分页窗口。
///
/// 只保留已加载页的条目；触底通过 `nextCursor` 继续加载，目录增量按身份
/// 原位合并（新会话前置、归档移除），未加载条目的增量直接忽略。
/// `revision` 是目录领域水位：基线与增量都只按它前进，旧事实被拒绝。
class ThreadDirectoryWindow {
  const ThreadDirectoryWindow({
    this.threads = const [],
    this.nextCursor,
    this.hasMore = false,
    this.isLoading = false,
    this.revision = 0,
  });

  final List<StudioThread> threads;
  final String? nextCursor;
  final bool hasMore;
  final bool isLoading;
  final int revision;

  ThreadDirectoryWindow copyWith({
    List<StudioThread>? threads,
    Object? nextCursor = _sentinel,
    bool? hasMore,
    bool? isLoading,
    int? revision,
  }) {
    return ThreadDirectoryWindow(
      threads: threads ?? this.threads,
      nextCursor: identical(nextCursor, _sentinel)
          ? this.nextCursor
          : nextCursor as String?,
      hasMore: hasMore ?? this.hasMore,
      isLoading: isLoading ?? this.isLoading,
      revision: revision ?? this.revision,
    );
  }

  /// 基线首页合并：首页内容以基线为准，已加载的更远（更旧）页保留不清空。
  ///
  /// 基线是订阅建立后的 canonical 首页；它不能冒充全量目录，也不得把用户已
  /// 触底加载的更旧页裁掉。旧基线（`revision` 不前进）原样返回。
  ThreadDirectoryWindow applyBaselinePage(
    ThreadDirectoryPage page, {
    required int revision,
  }) {
    if (revision <= this.revision) return this;
    final baseIds = {for (final thread in page.threads) thread.id};
    final last = page.threads.lastOrNull;
    final retainedOlder = [
      for (final thread in threads)
        if (!baseIds.contains(thread.id) &&
            (last == null || _sortsAfter(thread, last)))
          thread,
    ];
    return copyWith(
      threads: [...page.threads, ...retainedOlder],
      nextCursor: page.nextCursor,
      hasMore: page.hasMore,
      revision: revision,
    );
  }

  /// 增量合并：已加载条目原位替换；比当前窗口最新条目更新的前置；
  /// 其余（窗口未覆盖的更旧条目）忽略。
  ThreadDirectoryWindow applyDelta({
    required int revision,
    required List<StudioThread> upserted,
    required List<String> removed,
  }) {
    if (revision < this.revision) return this;
    if (upserted.isEmpty && removed.isEmpty) {
      return revision == this.revision ? this : copyWith(revision: revision);
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
          ..sort(StudioThread.compareDirectoryOrder);
    final prependable = newThreads.where((thread) {
      if (retained.isEmpty) return true;
      final first = retained.first;
      return thread.directorySortTime.isAfter(first.directorySortTime) ||
          (thread.directorySortTime.isAtSameMomentAs(first.directorySortTime) &&
              thread.id.compareTo(first.id) > 0);
    }).toList();
    return copyWith(threads: [...prependable, ...retained], revision: revision);
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
      // 追加页携带查询时的目录水位；只前进不回退（页本身不驱动目录事件）。
      revision: page.revision > revision ? page.revision : revision,
    );
  }
}

/// 目录序为 `(directorySortTime, id)` 倒序；[candidate] 是否排在 [anchor] 之后（更旧）。
///
/// 与 [StudioThread.compareDirectoryOrder] 使用同一 canonical 排序键（最近用户消息时间，
/// 缺失回落到创建时间），保证分页窗口的前置/保留判定与目录排序一致。
bool _sortsAfter(StudioThread candidate, StudioThread anchor) {
  final byTime = candidate.directorySortTime.compareTo(
    anchor.directorySortTime,
  );
  return byTime < 0 || (byTime == 0 && candidate.id.compareTo(anchor.id) < 0);
}

const Object _sentinel = Object();

/// 关机阶段的只读分类；canonical 状态由 [StudioShutdownProgress] 的 sealed variant 表达。
enum StudioShutdownPhase {
  stoppingSubscriptions,
  cancellingTurns,
  flushingPersistence,
  stoppingAgents,
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
    StoppingAgentsProgress() => StudioShutdownPhase.stoppingAgents,
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

final class StoppingAgentsProgress extends StudioShutdownProgress {
  const StoppingAgentsProgress();
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

enum DirectoryFilter { all, running, attention }

class DirectoryQuery {
  const DirectoryQuery({
    this.projectId,
    this.search,
    this.archived = false,
    this.filter = DirectoryFilter.all,
  });
  final String? projectId;
  final String? search;
  final bool archived;
  final DirectoryFilter filter;

  bool matches(StudioThread thread, List<StudioProject> projects) {
    if (!thread.isRoot ||
        thread.archived != archived ||
        (projectId != null && thread.projectId != projectId)) {
      return false;
    }
    if (filter == DirectoryFilter.running &&
        (!thread.status.isActive ||
            thread.status == ThreadStatusView.waitingInteraction)) {
      return false;
    }
    if (filter == DirectoryFilter.attention &&
        thread.status != ThreadStatusView.waitingInteraction &&
        thread.status != ThreadStatusView.faulted) {
      return false;
    }
    final term = search?.trim().toLowerCase() ?? '';
    final project = projects.where((p) => p.id == thread.projectId).firstOrNull;
    return term.isEmpty ||
        thread.title.toLowerCase().contains(term) ||
        (project != null &&
            '${project.name} ${project.path}'.toLowerCase().contains(term));
  }
}
