import 'package:flutter/foundation.dart' show listEquals;

import '../../domain/models/studio_models.dart';
import '../frb/studio_api.dart';

class StudioReduceResult {
  const StudioReduceResult(this.state, {this.resyncThreadId});

  final StudioState state;
  final String? resyncThreadId;
}

/// Product stream reducer.
///
/// Thread-local turn, item, interaction, and runtime facts are deliberately
/// excluded. They are applied only through [applyThreadSnapshot] and
/// [applyThreadUpdate].
StudioReduceResult reduceStudioEvent(
  StudioState current,
  StudioBridgeEvent event,
) {
  return switch (event.payload) {
    ProjectDirectoryChangedPayload(:final state) => StudioReduceResult(
      applyProjectDirectory(current, state),
    ),
    ThreadDirectoryChangedPayload(:final upserted, :final removed) =>
      StudioReduceResult(
        applyThreadDirectoryDelta(
          current,
          upserted: upserted,
          removed: removed,
        ),
      ),
    AgentDirectoryChangedPayload(:final state) => StudioReduceResult(
      applyAgentDirectory(current, state),
    ),
    SettingsStateChangedPayload(:final state) => StudioReduceResult(
      applySettingsState(current, state),
    ),
    RecoveryStateChangedPayload(:final state) => StudioReduceResult(
      applyRecoveryState(current, state),
    ),
    McpStateChangedPayload(:final state) => StudioReduceResult(
      applyMcpState(current, state),
    ),
    LspStateChangedPayload(:final state) => StudioReduceResult(
      applyLspState(current, state),
    ),
    SkillsStateChangedPayload(:final state) => StudioReduceResult(
      applySkillsState(current, state),
    ),
    ThreadModeCatalogChangedPayload(:final state) => StudioReduceResult(
      applyThreadModeCatalog(current, state),
    ),
    ProviderUsageStateChangedPayload(:final state) => StudioReduceResult(
      applyProviderUsageState(current, state),
    ),
    ModelPerformanceStateChangedPayload(:final state) => StudioReduceResult(
      state.revision <= current.modelPerformance.revision
          ? current
          : current.copyWith(modelPerformance: state),
    ),
    UpdaterStateChangedPayload(:final state) => StudioReduceResult(
      applyUpdaterState(current, state),
    ),
    PersistenceStateChangedPayload(:final state) => StudioReduceResult(
      applyPersistenceState(current, state),
    ),
    StalePayload() => StudioReduceResult(current),
  };
}

StudioState applyPersistenceState(
  StudioState current,
  PersistenceStateSnapshot next,
) {
  if (next.revision <= current.persistenceState.revision) return current;
  return current.copyWith(persistenceState: next);
}

/// Snapshot authority applies to runtime/preview facts, not the reading range.
StudioState applyThreadSnapshot(
  StudioState current,
  ThreadWorkspace workspace, {
  String? historyCursor,
}) {
  final threadId = workspace.thread.id;
  if (threadId.isEmpty) return current;
  final previous = current.workspacesByThread[threadId];
  if (previous != null && workspace.revision < previous.revision) {
    return current;
  }
  final ui = current.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
  final initialized =
      previous != null &&
      (previous.cachedItems.isNotEmpty || previous.revision > 0);
  final history = ui.history;
  final sameRevision = previous?.revision == workspace.revision;
  final base = previous ?? workspace.copyWith(items: const []);
  final combined = base.copyWith(
    items: {
      ...base.cachedItems,
      for (final item in base.items) item.id: item,
    }.values.toList(),
  );
  final merged = sameRevision
      ? _mergeStreamingPreview(combined, workspace)
      : mergeThreadItems(combined, workspace.items)!;
  final cache = {for (final item in merged.items) item.id: item};
  final tail = workspace.items.length > 400
      ? workspace.items.sublist(workspace.items.length - 400)
      : workspace.items;
  final latestIds = [for (final item in tail) item.id];
  final items = initialized && history.detached && base.items.isNotEmpty
      ? [for (final item in base.items) cache[item.id] ?? item]
      : merged.items;
  final directory = current.threads
      .where((thread) => thread.id == threadId)
      .firstOrNull;
  final nextWorkspace = _boundedTimeline(
    (sameRevision ? base : workspace).copyWith(
      thread: directory ?? workspace.thread,
      items: items,
      cachedItems: cache,
      latestItemIds: latestIds,
      observedLastTurn: sameRevision ? base.lastTurn : workspace.lastTurn,
      timelineTurns: {...base.timelineTurns, ...workspace.timelineTurns},
    ),
    TimelineDirection.newer,
    anchorId: history.detached ? history.anchor?.itemId : null,
  );
  final trimmed = items.length > maxTimelineWindowItems;
  final nextHistory = history.copyWith(
    olderCursor: trimmed
        ? nextWorkspace.items.firstOrNull?.id
        : initialized
        ? history.olderCursor
        : historyCursor,
    newerCursor: history.detached ? nextWorkspace.items.lastOrNull?.id : null,
    hasOlder: initialized
        ? history.hasOlder || trimmed
        : historyCursor != null || trimmed,
    hasNewer:
        history.detached &&
        (history.hasNewer ||
            (nextWorkspace.items.lastOrNull?.id != tail.lastOrNull?.id)),
  );
  // Preserve object identity when an equal-watermark frame changes no preview.
  if (sameRevision && identical(merged, combined) && initialized) {
    return _resolveWorkspaceSyncReady(current, threadId);
  }
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: nextWorkspace,
    },
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(
        syncState: AgentWorkspaceSyncState.ready,
        history: nextHistory,
      ),
    },
  );
}

ThreadWorkspace _mergeStreamingPreview(
  ThreadWorkspace previous,
  ThreadWorkspace incoming,
) {
  final items = {for (final item in previous.items) item.id: item};
  var changed = false;
  for (final item in incoming.items) {
    final streaming = switch (item.state) {
      ThreadTextItemStateView(lifecycle: StreamingThreadContentView()) ||
      ThreadThinkingItemStateView(lifecycle: StreamingThreadContentView()) ||
      ThreadToolItemStateView(
        lifecycle: RunningThreadToolView() || CancellingThreadToolView(),
      ) => true,
      _ => false,
    };
    if (!streaming) continue;
    final old = items[item.id];
    if (old != null && old.isTerminal) continue;
    final unchanged = switch ((old?.state, item.state)) {
      (
        ThreadTextItemStateView(text: final before),
        ThreadTextItemStateView(text: final after),
      ) =>
        before == after,
      (
        ThreadThinkingItemStateView(
          summary: final beforeSummary,
          content: final beforeContent,
        ),
        ThreadThinkingItemStateView(
          summary: final afterSummary,
          content: final afterContent,
        ),
      ) =>
        listEquals(beforeSummary, afterSummary) &&
            listEquals(beforeContent, afterContent),
      (
        ThreadToolItemStateView(
          lifecycle: RunningThreadToolView(streamedOutput: final before),
        ),
        ThreadToolItemStateView(
          lifecycle: RunningThreadToolView(streamedOutput: final after),
        ),
      ) =>
        before == after,
      (
        ThreadToolItemStateView(
          lifecycle: CancellingThreadToolView(streamedOutput: final before),
        ),
        ThreadToolItemStateView(
          lifecycle: CancellingThreadToolView(streamedOutput: final after),
        ),
      ) =>
        before == after,
      _ => false,
    };
    if (!unchanged) {
      items[item.id] = item;
      changed = true;
    }
  }
  return changed
      ? _sortedWorkspace(previous.copyWith(items: items.values.toList()))
      : previous;
}

StudioState _resolveWorkspaceSyncReady(StudioState state, String threadId) {
  final ui = state.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
  if (ui.syncState == AgentWorkspaceSyncState.ready) return state;
  return state.copyWith(
    workspaceUiByThread: {
      ...state.workspaceUiByThread,
      threadId: ui.copyWith(syncState: AgentWorkspaceSyncState.ready),
    },
  );
}

StudioReduceResult applyThreadUpdate(
  StudioState current, {
  required String threadId,
  required int revision,
  required ThreadWorkspaceUpdate update,
}) {
  var workspace = current.workspacesByThread[threadId];
  if (workspace == null) {
    return StudioReduceResult(current, resyncThreadId: threadId);
  }
  if (revision <= workspace.revision) {
    return StudioReduceResult(current);
  }
  if (revision != workspace.revision + 1) {
    return StudioReduceResult(current, resyncThreadId: threadId);
  }

  workspace = _sortedWorkspace(
    workspace.copyWith(
      items: {
        ...workspace.cachedItems,
        for (final item in workspace.items) item.id: item,
      }.values.toList(),
    ),
  );
  final updated = switch (update) {
    ThreadTurnUpdate(:final turn) => _applyCanonicalTurn(
      workspace,
      revision,
      turn,
    ),
    ThreadItemUpsert(:final item) => _upsertThreadItem(
      workspace,
      revision,
      item,
    ),
    ThreadItemDeltaUpdate(:final delta) => _appendThreadItemDelta(
      workspace,
      revision,
      delta,
    ),
    ThreadInteractionUpdate(:final interaction, :final pending) =>
      _updateThreadInteraction(workspace, revision, interaction, pending),
    ThreadRuntimeUpdate(:final runtime, :final todo) => workspace.copyWith(
      revision: revision,
      runtime: runtime,
      todo: todo,
    ),
  };
  if (updated == null) {
    return StudioReduceResult(current, resyncThreadId: threadId);
  }
  final tail = updated.items.length > 400
      ? updated.items.sublist(updated.items.length - 400)
      : updated.items;
  return StudioReduceResult(
    applyThreadSnapshot(
      current,
      updated.copyWith(items: tail),
      historyCursor: updated.items.length > tail.length ? tail.first.id : null,
    ),
  );
}

/// A page owns only its declared range. It never replaces active previews.
StudioState applyTimelinePage(
  StudioState current,
  String threadId,
  TimelinePage page,
  TimelineDirection direction, {
  bool replaceWindow = false,
}) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null || page.threadId != threadId) return current;
  final ui = current.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
  final cache = {
    ...workspace.cachedItems,
    for (final item in workspace.items) item.id: item,
  };
  for (final item in page.items) {
    if (item.threadId != threadId) continue;
    final existing = cache[item.id];
    if (existing == null ||
        (existing.isTerminal && item.revision > existing.revision)) {
      cache[item.id] = item;
    }
    if (existing != null &&
        page.watermark >= workspace.revision &&
        item.contextDisposition != cache[item.id]!.contextDisposition) {
      cache[item.id] = cache[item.id]!.copyWith(
        contextDisposition: item.contextDisposition,
      );
    }
  }
  final ids = {
    if (!replaceWindow) ...workspace.items.map((item) => item.id),
    ...page.items
        .where((item) => item.threadId == threadId)
        .map((item) => item.id),
  };
  final items = [for (final id in ids) cache[id]!]..sort(_compareItems);
  final trimmed = items.length > maxTimelineWindowItems;
  final next = _boundedTimeline(
    workspace.copyWith(
      items: items,
      cachedItems: cache,
      timelineTurns: {
        ...workspace.timelineTurns,
        for (final entry in page.turns) entry.turn.turnId: entry,
      },
    ),
    direction,
    anchorId: ui.history.anchor?.itemId,
  );
  final droppedOlder = next.items.firstOrNull?.id != items.firstOrNull?.id;
  final droppedNewer = next.items.lastOrNull?.id != items.lastOrNull?.id;
  return current.copyWith(
    workspacesByThread: {...current.workspacesByThread, threadId: next},
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(
        history: ui.history.copyWith(
          olderCursor: droppedOlder
              ? next.items.firstOrNull?.id
              : (replaceWindow || direction == TimelineDirection.older)
              ? page.olderCursor
              : ui.history.olderCursor,
          newerCursor: droppedNewer
              ? next.items.lastOrNull?.id
              : (replaceWindow || direction == TimelineDirection.newer)
              ? page.newerCursor
              : ui.history.newerCursor,
          hasOlder:
              droppedOlder ||
              ((replaceWindow || direction == TimelineDirection.older)
                  ? page.olderCursor != null
                  : ui.history.hasOlder),
          hasNewer:
              droppedNewer ||
              ((replaceWindow || direction == TimelineDirection.newer)
                  ? page.newerCursor != null
                  : ui.history.hasNewer),
          detached:
              replaceWindow ||
              ui.history.detached ||
              (trimmed && direction == TimelineDirection.older),
          isLoading: false,
          errorMessage: (replaceWindow || direction == TimelineDirection.older)
              ? null
              : ui.history.errorMessage,
          newerError: (replaceWindow || direction == TimelineDirection.newer)
              ? null
              : ui.history.newerError,
        ),
      ),
    },
  );
}

/// 恢复事实注入：历史页携带的 rolledBack 标记优先于窗口内任何条目（标记
/// 来自恢复投影的 rolled-back 范围，只有 DB 历史查询会给出，且可能落在
/// 比 内存投影更旧的 revision 上）。这不是翻页事件，不改变窗口分页状态。
StudioState applyRecoveredDispositions(
  StudioState current,
  String threadId,
  ThreadHistoryPage page,
) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null) return current;
  final items = _overlayRolledBackItems(workspace.items, page.items, threadId);
  if (identical(items, workspace.items)) return current;
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: workspace.copyWith(items: items),
    },
  );
}

/// rolledBack 条目（按 id）强制覆盖窗口内同 id 条目的 disposition；
/// 内存投影不产生 rolledBack 事实，故覆盖不受 revision 门槛约束。
List<ThreadItemView> _overlayRolledBackItems(
  List<ThreadItemView> items,
  List<ThreadItemView> pageItems,
  String threadId,
) {
  final rolledBackIds = {
    for (final item in pageItems)
      if (item.threadId == threadId &&
          item.contextDisposition == ThreadContextDisposition.rolledBack)
        item.id,
  };
  if (rolledBackIds.isEmpty) return items;
  return [
    for (final item in items)
      if (rolledBackIds.contains(item.id) &&
          item.contextDisposition == ThreadContextDisposition.active)
        item.copyWith(contextDisposition: ThreadContextDisposition.rolledBack)
      else
        item,
  ];
}

const int maxTimelineWindowItems = 500;

ThreadWorkspace _boundedTimeline(
  ThreadWorkspace workspace,
  TimelineDirection direction, {
  String? anchorId,
}) {
  final all = workspace.items;
  var start =
      all.length <= maxTimelineWindowItems ||
          direction == TimelineDirection.older
      ? 0
      : all.length - maxTimelineWindowItems;
  final anchor = all.indexWhere((item) => item.id == anchorId);
  if (anchor >= 0) {
    if (anchor < start) start = anchor;
    if (anchor >= start + maxTimelineWindowItems) {
      start = anchor - maxTimelineWindowItems + 1;
    }
  }
  final items = all.sublist(
    start,
    (start + maxTimelineWindowItems).clamp(start, all.length),
  );
  final ids = {...items.map((item) => item.id), ...workspace.latestItemIds};
  final cache = {
    ...workspace.cachedItems,
    for (final item in items) item.id: item,
  }..removeWhere((id, _) => !ids.contains(id));
  final turns = cache.values.map((item) => item.turnId).toSet();
  return workspace.copyWith(
    items: [for (final item in items) cache[item.id]!],
    cachedItems: cache,
    timelineTurns: {...workspace.timelineTurns}
      ..removeWhere((id, _) => !turns.contains(id)),
  );
}

StudioState jumpTimelineToLatest(StudioState current, String threadId) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null) return current;
  final ui = current.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
  final items = workspace.latestItems;
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: _boundedTimeline(
        workspace.copyWith(items: items),
        TimelineDirection.newer,
      ),
    },
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(
        history: ThreadHistoryWindow(
          isLoading: ui.history.isLoading,
          direction: ui.history.direction,
          hasOlder:
              ui.history.hasOlder ||
              workspace.items.firstOrNull?.id != items.firstOrNull?.id,
          epoch: ui.history.epoch + 1,
        ),
      ),
    },
  );
}

StudioState applySettingsState(
  StudioState current,
  SettingsStateSnapshot next,
) {
  return _applyObservedSnapshot(
    current,
    current.settingsState,
    next,
    (snapshot) => current.copyWith(settingsState: snapshot),
  );
}

StudioState applyProjectDirectory(
  StudioState current,
  ProjectDirectoryState next,
) {
  if (next.revision <= current.projectDirectory.revision) return current;
  final projectIds = next.values.map((project) => project.id).toSet();
  final selectedProjectId = projectIds.contains(current.selectedProjectId)
      ? current.selectedProjectId
      : ([
          ...next.values,
        ]..sort((a, b) => a.id.compareTo(b.id))).firstOrNull?.id;
  return current.copyWith(
    projectDirectory: next,
    selectedProjectId: selectedProjectId,
  );
}

StudioState applyThreadDirectoryDelta(
  StudioState current, {
  required List<StudioThread> upserted,
  required List<String> removed,
}) {
  final window = current.threadDirectory.applyDelta(
    upserted: upserted,
    removed: removed,
  );
  final upsertedById = {for (final thread in upserted) thread.id: thread};
  final removedSet = removed.toSet();
  // 分页窗口不是完整目录：选中线程只在被显式移除（归档/清理）时才回退，
  // "不在已加载窗口内"不代表线程不存在，不得触发选择切换。
  var selectedThreadId = current.selectedThreadId;
  if (selectedThreadId != null && removedSet.contains(selectedThreadId)) {
    final roots = window.threads
        .where(
          (thread) =>
              thread.isRoot && thread.projectId == current.selectedProjectId,
        )
        .toList();
    selectedThreadId = roots.firstOrNull?.id;
  }
  // 只为窗口内的线程重绑 directory 引用；窗口外的 workspace 原样保留。
  final workspaces = {
    for (final entry in current.workspacesByThread.entries)
      if (!removedSet.contains(entry.key))
        entry.key: upsertedById[entry.key] == null
            ? entry.value
            : entry.value.copyWith(thread: upsertedById[entry.key]),
  };
  final workspaceUi = Map<String, WorkspaceUiState>.from(
    current.workspaceUiByThread,
  )..removeWhere((id, _) => removedSet.contains(id));
  return current.copyWith(
    threadDirectory: window,
    selectedThreadId: selectedThreadId,
    workspacesByThread: workspaces,
    workspaceUiByThread: workspaceUi,
  );
}

/// 触底加载的下一页追加进分页窗口（按身份去重，不覆盖已加载 revision）。
StudioState appendThreadDirectoryPage(
  StudioState current,
  ThreadDirectoryPage page,
) {
  return current.copyWith(
    threadDirectory: current.threadDirectory.appendPage(page),
  );
}

StudioState setThreadDirectoryLoading(StudioState current, bool isLoading) {
  if (current.threadDirectory.isLoading == isLoading) return current;
  return current.copyWith(
    threadDirectory: current.threadDirectory.copyWith(isLoading: isLoading),
  );
}

StudioState applyAgentDirectory(StudioState current, AgentDirectoryState next) {
  return _applyObservedSnapshot(
    current,
    current.agentDirectory,
    next,
    (snapshot) => current.copyWith(agentDirectory: snapshot),
  );
}

StudioState applyRecoveryState(
  StudioState current,
  RecoveryStateSnapshot next,
) {
  return _applyObservedSnapshot(
    current,
    current.recoveryState,
    next,
    (snapshot) => current.copyWith(recoveryState: snapshot),
  );
}

StudioState applyProviderUsageState(
  StudioState current,
  ProviderUsageStateSnapshot next,
) {
  return _applyObservedSnapshot(
    current,
    current.providerUsageState,
    next,
    (snapshot) => current.copyWith(providerUsageState: snapshot),
  );
}

StudioState applySkillsState(StudioState current, SkillsStateSnapshot next) {
  final previous = current.skillsByProject[next.projectId];
  if (previous != null && next.revision <= previous.revision) return current;
  return current.copyWith(
    skillsByProject: {...current.skillsByProject, next.projectId: next},
  );
}

StudioState applyThreadModeCatalog(
  StudioState current,
  ThreadModeCatalogView next,
) {
  if (next.revision <= current.threadModeCatalog.revision) return current;
  return current.copyWith(threadModeCatalog: next);
}

StudioState applyMcpState(StudioState current, McpStateSnapshot next) {
  return _applyObservedSnapshot(
    current,
    current.mcpState,
    next,
    (snapshot) => current.copyWith(mcpState: snapshot),
  );
}

StudioState applyLspState(StudioState current, LspStateSnapshot next) {
  return _applyObservedSnapshot(
    current,
    current.lspState,
    next,
    (snapshot) => current.copyWith(lspState: snapshot),
  );
}

StudioState applyUpdaterState(StudioState current, UpdaterStateSnapshot next) {
  if (next.revision <= current.updaterState.revision) return current;
  return current.copyWith(updaterState: next);
}

StudioState _applyObservedSnapshot<T extends ObservedStateSnapshot<dynamic>>(
  StudioState current,
  T previous,
  T next,
  StudioState Function(T snapshot) replace,
) {
  if (next.revision <= previous.revision) return current;
  return replace(next);
}

String planFollowUpPrompt(
  PendingInteraction interaction,
  InteractionResolutionCommand resolution,
) {
  final reason = switch (resolution) {
    ToolApprovalResolutionCommand(:final reason) => reason?.trim() ?? '',
    UserInputResolutionCommand() => '',
  };
  if (reason.isNotEmpty) return reason;
  return interaction.body.trim();
}

ThreadWorkspace _sortedWorkspace(ThreadWorkspace workspace) {
  final items = [...workspace.items]..sort(_compareItems);
  return workspace.copyWith(items: items);
}

/// Timeline item 的唯一合并规则（live 帧、snapshot、历史页共用）：
/// 身份 = itemId + threadId + turnId + kind；同 id 时仅当 incoming.revision
/// >= existing 才替换；新 id 插入后按 (ordinal, id) 全序排序。ordinal 是
/// Rust 事件总线一次性分配的不可变顺序事实，不参与身份比较。
ThreadWorkspace? mergeThreadItems(
  ThreadWorkspace workspace,
  List<ThreadItemView> incomingItems,
) {
  if (incomingItems.isEmpty) return workspace;
  var changed = false;
  final items = [...workspace.items];
  for (final incoming in incomingItems) {
    if (incoming.threadId != workspace.thread.id || incoming.id.isEmpty) {
      continue;
    }
    final index = items.indexWhere((item) => item.id == incoming.id);
    if (index >= 0) {
      final existing = items[index];
      if (!_sameItemIdentity(existing, incoming) ||
          incoming.revision < existing.revision) {
        continue;
      }
      // 防御性不可变：ordinal 由 Rust 总线一次性分配，替换载荷时保留已加载
      // 值，忽略迟到载荷中的 ordinal 漂移。
      items[index] = incoming.ordinal == existing.ordinal
          ? incoming
          : incoming.copyWith(ordinal: existing.ordinal);
      changed = true;
    } else {
      items.add(incoming);
      changed = true;
    }
  }
  if (!changed) return workspace;
  items.sort(_compareItems);
  return workspace.copyWith(items: items);
}

ThreadWorkspace? _upsertThreadItem(
  ThreadWorkspace workspace,
  int workspaceRevision,
  ThreadItemView incoming,
) {
  final merged = mergeThreadItems(workspace, [incoming]);
  if (merged == null) return null;
  return merged.copyWith(revision: workspaceRevision);
}

ThreadWorkspace? _appendThreadItemDelta(
  ThreadWorkspace workspace,
  int workspaceRevision,
  ThreadItemDeltaView delta,
) {
  final items = [...workspace.items];
  final index = items.indexWhere((item) => item.id == delta.itemId);
  if (index < 0) return null;
  final item = items[index];
  if (item.isTerminal) return null;
  if (delta.revision <= item.revision) {
    return workspace.copyWith(revision: workspaceRevision);
  }
  if (delta.revision != item.revision + 1) return null;
  final nextItem = item.appendDelta(
    delta: delta.state,
    nextRevision: delta.revision,
  );
  if (nextItem == null) return null;
  items[index] = nextItem;
  return workspace.copyWith(revision: workspaceRevision, items: items);
}

ThreadWorkspace _updateThreadInteraction(
  ThreadWorkspace workspace,
  int revision,
  PendingInteraction interaction,
  bool pending,
) {
  final interactions = [...workspace.interactions];
  final index = interactions.indexWhere((item) => item.id == interaction.id);
  if (!pending) {
    if (index >= 0) interactions.removeAt(index);
  } else if (index >= 0) {
    interactions[index] = interaction;
  } else {
    interactions.add(interaction);
  }
  return workspace.copyWith(revision: revision, interactions: interactions);
}

bool _sameItemIdentity(ThreadItemView left, ThreadItemView right) {
  // 身份只由稳定标识构成；ordinal 不可变（总线分配）、createdAt/revision 属
  // 可更新事实，都不参与身份判定。
  return left.id == right.id &&
      left.threadId == right.threadId &&
      left.turnId == right.turnId &&
      left.kind == right.kind;
}

int _compareItems(ThreadItemView left, ThreadItemView right) {
  final ordinal = left.ordinal.compareTo(right.ordinal);
  return ordinal != 0 ? ordinal : left.id.compareTo(right.id);
}

ThreadWorkspace _applyCanonicalTurn(
  ThreadWorkspace workspace,
  int revision,
  StudioTurnView turn,
) {
  final last = workspace.lastTurn;
  if (last != null &&
      (turn.revision <= last.revision ||
          (last.turnId == turn.turnId &&
              last.state.isTerminal &&
              turn.state.isBusy))) {
    return workspace.copyWith(revision: revision);
  }
  return workspace.copyWith(
    revision: revision,
    activeTurn: turn.state.isBusy ? turn : null,
    observedLastTurn: turn,
  );
}
