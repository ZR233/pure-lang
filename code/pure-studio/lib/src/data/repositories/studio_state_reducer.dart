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

/// Authoritative snapshots replace the complete canonical workspace.
///
/// Subscription generation checks belong to the controller. Once a snapshot
/// reaches this reducer it wins over every locally accumulated delta.
/// A replacing snapshot is also the rebuild point of the timeline history
/// window: items ← snapshot window, [ThreadHistoryWindow.epoch] increments to
/// invalidate in-flight history responses, and `hasOlder` derives from the
/// snapshot's history cursor. An equal-revision snapshot keeps the workspace
/// instance and the window untouched — revision-gap enforcement makes local
/// state at revision N identical to the canonical N.
StudioState applyThreadSnapshot(
  StudioState current,
  ThreadWorkspace workspace, {
  String? historyCursor,
}) {
  final threadId = workspace.thread.id;
  if (threadId.isEmpty) return current;
  final previous = current.workspacesByThread[threadId];
  if (previous != null) {
    if (workspace.revision < previous.revision) return current;
    if (workspace.revision == previous.revision) {
      return _resolveWorkspaceSyncReady(current, threadId);
    }
  }
  final directoryThread = current.threads
      .where((thread) => thread.id == threadId)
      .firstOrNull;
  final workspaces =
      Map<String, ThreadWorkspace>.from(current.workspacesByThread)
        ..[threadId] = _sortedWorkspace(
          workspace.copyWith(thread: directoryThread ?? workspace.thread),
        );
  final ui = current.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
  final workspaceUi = Map<String, WorkspaceUiState>.from(
    current.workspaceUiByThread,
  );
  workspaceUi[threadId] = ui.copyWith(
    syncState: AgentWorkspaceSyncState.ready,
    history: ThreadHistoryWindow(
      hasOlder: historyCursor != null,
      isLoading: false,
      epoch: ui.history.epoch + 1,
      errorMessage: null,
    ),
  );
  return current.copyWith(
    workspacesByThread: workspaces,
    workspaceUiByThread: workspaceUi,
  );
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

/// 历史页落地（窗口向旧扩展）：items 幂等合并、hasOlder 推进、超限收缩，
/// 一次归约完成。调用方（controller）已校验响应属于当前窗口代际。
StudioState applyThreadHistoryPage(
  StudioState current,
  String threadId,
  ThreadHistoryPage page,
) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null) return current;
  final ui = current.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
  var items = workspace.items;
  if (page.items.isNotEmpty) {
    items =
        mergeThreadItems(workspace, [
          for (final item in page.items)
            if (item.threadId == threadId) item,
        ])?.items ??
        workspace.items;
    items = _overlayRolledBackItems(items, page.items, threadId);
  }
  var next = current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: workspace.copyWith(items: items),
    },
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(
        history: ThreadHistoryWindow(
          hasOlder: page.nextCursor != null,
          isLoading: false,
          epoch: ui.history.epoch,
          errorMessage: null,
        ),
      ),
    },
  );
  return enforceTimelineWindowLimit(next, threadId);
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

/// 时间线窗口的 item 上限；超过后从最旧方向裁剪。
const int maxTimelineWindowItems = 500;

/// 窗口收缩：items 超过上限时裁掉最旧一端。被裁内容仍可回源——回源锚点从
/// 裁剪后的 `items.first.turnId` 派生，因此只需把 hasOlder 置回 true。
StudioState enforceTimelineWindowLimit(StudioState current, String threadId) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null || workspace.items.length <= maxTimelineWindowItems) {
    return current;
  }
  final items = workspace.items.sublist(
    workspace.items.length - maxTimelineWindowItems,
  );
  final ui = current.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: workspace.copyWith(items: items),
    },
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(
        history: ui.history.hasOlder
            ? ui.history
            : ThreadHistoryWindow(
                hasOlder: true,
                isLoading: ui.history.isLoading,
                epoch: ui.history.epoch,
                errorMessage: ui.history.errorMessage,
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
