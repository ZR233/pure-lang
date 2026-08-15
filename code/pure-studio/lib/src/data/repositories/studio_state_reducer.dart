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
    TaskDirectoryChangedPayload(:final state) => StudioReduceResult(
      applyTaskDirectory(current, state),
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
    ProviderUsageStateChangedPayload(:final state) => StudioReduceResult(
      applyProviderUsageState(current, state),
    ),
    UpdaterStateChangedPayload(:final state) => StudioReduceResult(
      applyUpdaterState(current, state),
    ),
    StalePayload() => StudioReduceResult(current),
  };
}

/// Authoritative snapshots replace the complete canonical workspace.
///
/// Subscription generation checks belong to the controller. Once a snapshot
/// reaches this reducer it wins over every locally accumulated delta.
StudioState applyThreadSnapshot(
  StudioState current,
  ThreadWorkspace workspace,
) {
  final threadId = workspace.thread.id;
  if (threadId.isEmpty) return current;
  final previous = current.workspacesByThread[threadId];
  if (previous != null && workspace.revision <= previous.revision) {
    return current;
  }
  final directoryThread = current.threads
      .where((thread) => thread.id == threadId)
      .firstOrNull;
  final workspaces =
      Map<String, ThreadWorkspace>.from(current.workspacesByThread)
        ..[threadId] = _sortedWorkspace(
          workspace.copyWith(thread: directoryThread ?? workspace.thread),
        );
  final workspaceUi = Map<String, WorkspaceUiState>.from(
    current.workspaceUiByThread,
  );
  workspaceUi[threadId] = (workspaceUi[threadId] ?? const WorkspaceUiState())
      .copyWith(syncState: AgentWorkspaceSyncState.ready);
  return current.copyWith(
    workspacesByThread: workspaces,
    workspaceUiByThread: workspaceUi,
  );
}

StudioReduceResult applyThreadUpdate(
  StudioState current, {
  required String threadId,
  required int revision,
  required ThreadWorkspaceUpdate update,
}) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null) {
    return StudioReduceResult(current, resyncThreadId: threadId);
  }
  if (revision <= workspace.revision) {
    return StudioReduceResult(current);
  }
  if (revision != workspace.revision + 1) {
    return StudioReduceResult(current, resyncThreadId: threadId);
  }

  final updated = switch (update) {
    ThreadTurnUpdate(:final turn) => workspace.copyWith(
      revision: revision,
      activeTurn: turn.state.isBusy ? turn : null,
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
  return StudioReduceResult(
    current.copyWith(
      workspacesByThread: {...current.workspacesByThread, threadId: updated},
    ),
  );
}

StudioState mergeThreadHistoryPage(
  StudioState current,
  String threadId,
  ThreadHistoryPage page,
) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null || page.items.isEmpty) return current;
  final byId = <String, ThreadItemView>{
    for (final item in workspace.items) item.id: item,
  };
  for (final item in page.items) {
    if (item.threadId != threadId) continue;
    final existing = byId[item.id];
    if (existing == null ||
        item.revision > existing.revision ||
        (item.contextDisposition == ThreadContextDisposition.rolledBack &&
            existing.contextDisposition == ThreadContextDisposition.active)) {
      byId[item.id] = item;
    }
  }
  final items = byId.values.toList()..sort(_compareItems);
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: workspace.copyWith(items: items),
    },
  );
}

/// 时间线已加载历史的驱逐上限；超过后从最旧一页开始丢弃。
const int maxLoadedHistoryItems = 500;

/// 驱逐最旧一页已加载历史：回退 load-older cursor 到该页的请求 cursor，
/// 再次向上滚动时以它回源数据库重取（内存未命中 → 数据库）。
StudioState evictOldestHistoryPage(StudioState current, String threadId) {
  final ui = current.workspaceUiByThread[threadId];
  final workspace = current.workspacesByThread[threadId];
  if (ui == null || workspace == null) return current;
  final history = ui.history;
  if (history.pageSizes.length <= 1) return current;
  final boundaryCursor = history.boundaryCursors.last;
  final removed = history.pageSizes.last;
  final remainingItems = (history.loadedItems - removed).clamp(0, 1 << 30);
  // items 按时间升序，最旧一页在列表头部。
  final evictedItems = workspace.items.length >= removed
      ? workspace.items.sublist(removed)
      : const <ThreadItemView>[];
  final nextHistory = ThreadHistoryPagingState(
    nextCursor: boundaryCursor,
    hasMore: true,
    isLoading: false,
    isLoaded: history.isLoaded,
    errorMessage: history.errorMessage,
    boundaryCursors: history.boundaryCursors.sublist(
      0,
      history.boundaryCursors.length - 1,
    ),
    pageSizes: history.pageSizes.sublist(0, history.pageSizes.length - 1),
    loadedItems: remainingItems,
  );
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: workspace.copyWith(items: evictedItems),
    },
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(history: nextHistory),
    },
  );
}

StudioState applySettingsState(
  StudioState current,
  SettingsStateSnapshot next,
) {
  if (!next.meta.isNewerThan(current.settingsState.meta)) return current;
  return current.copyWith(settingsState: next);
}

StudioState applyProjectDirectory(
  StudioState current,
  ProjectDirectoryState next,
) {
  if (!next.meta.isNewerThan(current.projectDirectory.meta)) return current;
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

StudioState applyTaskDirectory(StudioState current, TaskDirectoryState next) {
  if (!next.meta.isNewerThan(current.taskDirectory.meta)) return current;
  return current.copyWith(taskDirectory: next);
}

StudioState applyAgentDirectory(StudioState current, AgentDirectoryState next) {
  if (!next.meta.isNewerThan(current.agentDirectory.meta)) return current;
  return current.copyWith(agentDirectory: next);
}

StudioState applyRecoveryState(
  StudioState current,
  RecoveryStateSnapshot next,
) {
  if (!next.meta.isNewerThan(current.recoveryState.meta)) return current;
  return current.copyWith(recoveryState: next);
}

StudioState applyProviderUsageState(
  StudioState current,
  ProviderUsageStateSnapshot next,
) {
  if (!next.meta.isNewerThan(current.providerUsageState.meta)) return current;
  return current.copyWith(providerUsageState: next);
}

StudioState applySkillsState(StudioState current, SkillsStateSnapshot next) {
  final previous = current.skillsByProject[next.projectId];
  if (previous != null && !next.meta.isNewerThan(previous.meta)) return current;
  return current.copyWith(
    skillsByProject: {...current.skillsByProject, next.projectId: next},
  );
}

StudioState applyMcpState(StudioState current, McpStateSnapshot next) {
  if (!next.meta.isNewerThan(current.mcpState.meta)) return current;
  return current.copyWith(mcpState: next);
}

StudioState applyLspState(StudioState current, LspStateSnapshot next) {
  if (!next.meta.isNewerThan(current.lspState.meta)) return current;
  return current.copyWith(lspState: next);
}

StudioState applyUpdaterState(StudioState current, UpdaterStateSnapshot next) {
  if (!next.meta.isNewerThan(current.updaterState.meta)) return current;
  return current.copyWith(updaterState: next);
}

String defaultEffortForModel(
  StudioState current,
  String providerId,
  String model,
) {
  for (final provider in current.providers) {
    if (provider.id != providerId) continue;
    for (final candidate in provider.models) {
      if (candidate.slug == model && candidate.reasoningEfforts.isNotEmpty) {
        return candidate.reasoningEfforts.first;
      }
    }
  }
  return current.role('planner')?.effort ?? 'high';
}

String planFollowUpPrompt(
  PendingInteraction interaction,
  InteractionResolutionCommand resolution,
) {
  final content = resolution is PlanConfirmationResolutionCommand
      ? resolution.content?.trim() ?? ''
      : '';
  if (content.isNotEmpty) return content;
  final reason = switch (resolution) {
    PlanConfirmationResolutionCommand(:final reason) => reason?.trim() ?? '',
    ToolApprovalResolutionCommand(:final reason) => reason?.trim() ?? '',
    UserInputResolutionCommand() => '',
  };
  if (reason.isNotEmpty) return reason;
  return switch (interaction.payload) {
    PlanConfirmationInteractionPayload(:final content)
        when content.trim().isNotEmpty =>
      content.trim(),
    _ => interaction.body.trim(),
  };
}

ThreadWorkspace _sortedWorkspace(ThreadWorkspace workspace) {
  final items = [...workspace.items]..sort(_compareItems);
  return workspace.copyWith(items: items);
}

ThreadWorkspace? _upsertThreadItem(
  ThreadWorkspace workspace,
  int workspaceRevision,
  ThreadItemView incoming,
) {
  if (incoming.threadId != workspace.thread.id || incoming.id.isEmpty) {
    return null;
  }
  final items = [...workspace.items];
  final index = items.indexWhere((item) => item.id == incoming.id);
  if (index >= 0) {
    final existing = items[index];
    if (!_sameItemIdentity(existing, incoming) ||
        incoming.revision < existing.revision) {
      return null;
    }
    items[index] = incoming;
  } else {
    items.add(incoming);
  }
  items.sort(_compareItems);
  return workspace.copyWith(revision: workspaceRevision, items: items);
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
  if (_isTerminalItemStatus(item.status)) return null;
  if (delta.revision <= item.revision) {
    return workspace.copyWith(revision: workspaceRevision);
  }
  if (delta.revision != item.revision + 1) return null;
  items[index] = item.appendDelta(
    field: delta.field,
    delta: delta.delta,
    nextRevision: delta.revision,
  );
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
  return left.id == right.id &&
      left.threadId == right.threadId &&
      left.turnId == right.turnId &&
      left.ordinal == right.ordinal &&
      left.kind == right.kind &&
      left.createdAt == right.createdAt;
}

bool _isTerminalItemStatus(String status) {
  return const {
    'completed',
    'failed',
    'interrupted',
    'cancelled',
    'denied',
    'budgetLimited',
  }.contains(status);
}

int _compareItems(ThreadItemView left, ThreadItemView right) {
  final ordinal = left.ordinal.compareTo(right.ordinal);
  return ordinal != 0 ? ordinal : left.id.compareTo(right.id);
}
