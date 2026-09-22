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
      applyModelPerformanceState(current, state),
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

StudioState applyModelPerformanceState(
  StudioState current,
  ModelPerformanceSnapshotView next,
) {
  if (next.revision <= current.modelPerformance.revision) return current;
  return current.copyWith(modelPerformance: next);
}

StudioState applyPersistenceState(
  StudioState current,
  PersistenceStateSnapshot next,
) {
  if (next.revision <= current.persistenceState.revision) return current;
  return current.copyWith(persistenceState: next);
}

/// Thread snapshot 只替换当前状态：thread 身份、revision、activeTurn、pending
/// Interaction、runtime 与 Todo。有界 Timeline 窗口属于阅读面，只由历史页与实时
/// 通知维护；snapshot 不携带条目、Turn 摘要或回源锚点，也不与旧条目混合。
StudioState applyThreadSnapshot(StudioState current, ThreadWorkspace snapshot) {
  final threadId = snapshot.thread.id;
  if (threadId.isEmpty) return current;
  final previous = current.workspacesByThread[threadId];
  // Each accepted snapshot begins a new subscription generation. Its watermark may be lower
  // than the old generation's live-notification revision; only that generation's frames may
  // advance it. Retain terminal items until SQL confirms them, not stale running previews.
  final base =
      previous ??
      snapshot.copyWith(
        items: const [],
        liveItems: const {},
        timelineTurns: const {},
      );
  final directory = current.threads
      .where((thread) => thread.id == threadId)
      .firstOrNull;
  final next = base.copyWith(
    thread: directory ?? snapshot.thread,
    revision: snapshot.revision,
    liveItems: {
      for (final item in base.liveItems.values)
        if (item.isTerminal) item.id: item,
    },
    activeTurn: snapshot.activeTurn,
    interactions: snapshot.interactions,
    runtime: snapshot.runtime,
    todo: snapshot.todo,
    latestTurn: _newestTurn(base.latestTurn, snapshot.activeTurn),
  );
  final ui = current.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
  return current.copyWith(
    workspacesByThread: {...current.workspacesByThread, threadId: next},
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(syncState: AgentWorkspaceSyncState.ready),
    },
  );
}

/// 最近 Turn 事实：同 Turn 先比 revision（终态不被迟到的 busy 载荷拉回），
/// 不同 Turn 先比 updatedAt；同秒时用 canonical revision 判定先后。
StudioTurnView? _newestTurn(StudioTurnView? left, StudioTurnView? right) {
  if (left == null) return right;
  if (right == null) return left;
  if (left.turnId == right.turnId) {
    if (right.revision == left.revision) {
      return right.state.isBusy && left.state.isTerminal ? left : right;
    }
    return right.revision > left.revision ? right : left;
  }
  final timeOrder = right.updatedAt.compareTo(left.updatedAt);
  return timeOrder > 0 || (timeOrder == 0 && right.revision > left.revision)
      ? right
      : left;
}

StudioReduceResult applyThreadUpdate(
  StudioState current, {
  required String threadId,
  required int revision,
  required ThreadWorkspaceUpdate update,
  int? baseRevision,
}) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null) {
    return StudioReduceResult(current, resyncThreadId: threadId);
  }
  // base revision 是生产端声明的"上一条水位"：只要它不接在当前状态之后，
  // 说明中间有缺口（广播 lag、乱序或 epoch 切换），必须重同步而不是拼接增量。
  if (baseRevision != null && baseRevision != workspace.revision) {
    return StudioReduceResult(current, resyncThreadId: threadId);
  }
  if (revision <= workspace.revision) {
    return StudioReduceResult(current);
  }
  if (revision != workspace.revision + 1) {
    return StudioReduceResult(current, resyncThreadId: threadId);
  }
  final updated = switch (update) {
    ThreadTurnUpdate(:final turn) => _applyThreadTurn(
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
  final ui = _workspaceUi(current, threadId);
  final syncedUi = syncItemBodyState(ui, updated);
  final hasNewItem =
      update is ThreadItemUpsert &&
      !workspace.historyItems.any((item) => item.id == update.item.id) &&
      !workspace.liveItems.containsKey(update.item.id);
  final nextUi = hasNewItem && ui.history.detached
      ? syncedUi.copyWith(history: syncedUi.history.copyWith(hasNewer: true))
      : syncedUi;
  return StudioReduceResult(
    identical(nextUi, ui)
        ? current.copyWith(
            workspacesByThread: {
              ...current.workspacesByThread,
              threadId: updated,
            },
          )
        : current.copyWith(
            workspacesByThread: {
              ...current.workspacesByThread,
              threadId: updated,
            },
            workspaceUiByThread: {
              ...current.workspaceUiByThread,
              threadId: nextUi,
            },
          ),
  );
}

WorkspaceUiState _workspaceUi(StudioState state, String threadId) {
  return state.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
}

/// 由窗口条目的正文状态推导预览/加载/错误标记：条目本身是唯一事实源。
///
/// 历史页预览因此落到同一个状态；条目被显式回源（`bodyLoaded`）
/// 或正文回到完整时，预览标记自然消失，不会残留成第二份事实。
///
/// 回源相关的 UI 状态（加载/待落盘/错误/缺席）只在条目仍然“需要回源”时才有意义：
/// 正文已经完整或已显式加载的条目会连同旧标记一起清掉，避免出现指向已完整正文的
/// 过期重试入口。
WorkspaceUiState syncItemBodyState(
  WorkspaceUiState ui,
  ThreadWorkspace workspace,
) {
  final previewed = {
    for (final item in workspace.historyItems)
      if (item.bodyPreviewed) item.id,
  };
  final loading = ui.history.loadingItemIds.where(previewed.contains).toSet();
  final pending = ui.history.pendingItemBodyIds
      .where(previewed.contains)
      .toSet();
  final errors = {
    for (final entry in ui.history.itemBodyErrors.entries)
      if (previewed.contains(entry.key)) entry.key: entry.value,
  };
  final unavailable = ui.history.unavailableItemIds
      .where(previewed.contains)
      .toSet();
  if (_sameIds(previewed, ui.history.previewedItemIds) &&
      _sameIds(loading, ui.history.loadingItemIds) &&
      _sameIds(pending, ui.history.pendingItemBodyIds) &&
      _sameIds(unavailable, ui.history.unavailableItemIds) &&
      errors.length == ui.history.itemBodyErrors.length) {
    return ui;
  }
  return ui.copyWith(
    history: ui.history.copyWith(
      previewedItemIds: previewed,
      loadingItemIds: loading,
      itemBodyErrors: errors,
      pendingItemBodyIds: pending,
      unavailableItemIds: unavailable,
    ),
  );
}

bool _sameIds(Set<String> left, Set<String> right) {
  return left.length == right.length && left.containsAll(right);
}

/// SQL pages own the reading window; live frames own the independent overlay.
StudioState applyTimelinePage(
  StudioState current,
  String threadId,
  TimelinePage page,
  TimelineDirection direction, {
  bool replaceWindow = false,
  bool followBottom = false,
}) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null || page.threadId != threadId) return current;
  final ui = current.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
  final pageItems = [
    for (final item in page.items)
      if (item.threadId == threadId) item,
  ];
  final historyItems = <String, ThreadItemView>{
    if (!replaceWindow)
      for (final item in workspace.historyItems) item.id: item,
  };
  final pagePreviewOmitted = {
    for (final preview in page.previews) preview.itemId: preview.omittedBytes,
  };
  for (final item in pageItems) {
    final existing = historyItems[item.id];
    final merged = existing == null
        ? item
        : _preferPageItem(
                existing,
                item,
                pageWatermark: page.watermark,
                workspaceRevision: workspace.revision,
              ) ??
              existing;
    historyItems[item.id] = boundThreadItemBody(
      merged,
      previewOmittedUnits: pagePreviewOmitted[item.id] ?? 0,
    );
  }
  final items = historyItems.values.toList()..sort(_compareItems);
  final trimmed = items.length > maxTimelineWindowItems;
  var latestTurn = workspace.latestTurn;
  for (final entry in page.turns) {
    latestTurn = _newestTurn(latestTurn, entry.turn);
  }
  final overlay = Map<String, ThreadItemView>.from(workspace.liveItems);
  for (final item in pageItems) {
    final pending = overlay[item.id];
    if (pending == null ||
        !pending.isTerminal ||
        !item.isTerminal ||
        item.revision < pending.revision) {
      continue;
    }
    // Preserve the visible complete body when the SQL page only holds a preview.
    if (ui.history.anchor?.itemId == item.id ||
        (followBottom &&
            workspace.liveItems.values.lastOrNull?.id == item.id)) {
      historyItems[item.id] = pending.copyWith(
        contextDisposition: item.contextDisposition,
        bodyLoaded: true,
      );
    }
    overlay.remove(item.id);
  }
  final confirmedItems = historyItems.values.toList()..sort(_compareItems);
  final next = _boundedTimeline(
    workspace.copyWith(
      items: confirmedItems,
      liveItems: overlay,
      latestTurn: latestTurn,
      timelineTurns: {
        if (!replaceWindow) ...workspace.timelineTurns,
        for (final entry in page.turns) entry.turn.turnId: entry,
      },
    ),
    direction,
    anchorId: ui.history.anchor?.itemId,
  );
  final droppedOlder =
      next.historyItems.firstOrNull?.id != items.firstOrNull?.id;
  final droppedNewer = next.historyItems.lastOrNull?.id != items.lastOrNull?.id;
  final synced = syncItemBodyState(ui, next);
  final previewedItemIds = synced.history.previewedItemIds;
  final loadingItemIds = synced.history.loadingItemIds;
  final itemBodyErrors = synced.history.itemBodyErrors;
  final pendingItemBodyIds = synced.history.pendingItemBodyIds;
  final unavailableItemIds = synced.history.unavailableItemIds;
  final databaseId = page.databaseId.isEmpty
      ? ui.history.databaseId
      : page.databaseId;
  final appliedWriteSequence = replaceWindow
      ? page.watermark
      : (page.watermark > ui.history.appliedWriteSequence
            ? page.watermark
            : ui.history.appliedWriteSequence);
  final followHasOlder = droppedOlder || page.olderCursor != null;
  final followOlderCursor = !followHasOlder
      ? null
      : droppedOlder
      ? next.historyItems.firstOrNull?.id
      : page.olderCursor ?? next.historyItems.firstOrNull?.id;
  final history = followBottom
      ? ui.history.copyWith(
          olderCursor: followOlderCursor,
          newerCursor: null,
          hasOlder: followHasOlder,
          hasNewer: droppedNewer,
          detached: false,
          isLoading: false,
          errorMessage: null,
          newerError: null,
          databaseId: databaseId,
          appliedWriteSequence: appliedWriteSequence,
          previewedItemIds: previewedItemIds,
          loadingItemIds: loadingItemIds,
          itemBodyErrors: itemBodyErrors,
          pendingItemBodyIds: pendingItemBodyIds,
          unavailableItemIds: unavailableItemIds,
        )
      : ui.history.copyWith(
          olderCursor: droppedOlder
              ? next.historyItems.firstOrNull?.id
              : (replaceWindow || direction == TimelineDirection.older)
              ? page.olderCursor
              : ui.history.olderCursor,
          newerCursor: droppedNewer
              ? next.historyItems.lastOrNull?.id
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
          databaseId: databaseId,
          appliedWriteSequence: appliedWriteSequence,
          previewedItemIds: previewedItemIds,
          loadingItemIds: loadingItemIds,
          itemBodyErrors: itemBodyErrors,
          pendingItemBodyIds: pendingItemBodyIds,
          unavailableItemIds: unavailableItemIds,
        );
  return current.copyWith(
    workspacesByThread: {...current.workspacesByThread, threadId: next},
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(history: history),
    },
  );
}

/// Confirms off-window terminal items without changing the reader's SQL page or anchor.
StudioState confirmDetachedTimelineItems(
  StudioState current,
  String threadId,
  Iterable<(ThreadItemView, int)> persisted,
) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null || workspace.liveItems.isEmpty) return current;
  final visibleIndices = {
    for (var index = 0; index < workspace.historyItems.length; index++)
      workspace.historyItems[index].id: index,
  };
  final historyItems = workspace.historyItems.toList();
  final overlay = Map<String, ThreadItemView>.from(workspace.liveItems);
  final anchorId =
      current.workspaceUiByThread[threadId]?.history.anchor?.itemId;
  for (final (item, omittedUnits) in persisted) {
    final pending = overlay[item.id];
    if (pending != null &&
        pending.isTerminal &&
        item.isTerminal &&
        item.revision >= pending.revision) {
      final visibleIndex = visibleIndices[item.id];
      if (visibleIndex != null) {
        historyItems[visibleIndex] = anchorId == item.id
            ? pending.copyWith(
                contextDisposition: item.contextDisposition,
                bodyLoaded: true,
              )
            : boundThreadItemBody(item, previewOmittedUnits: omittedUnits);
      }
      overlay.remove(item.id);
    }
  }
  if (overlay.length == workspace.liveItems.length) return current;
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: workspace.copyWith(items: historyItems, liveItems: overlay),
    },
  );
}

/// 历史页条目与窗口内同身份条目的合并规则。
///
/// - 数据库 revision 更新：以数据库为准；
/// - revision 相同且数据库已是终态而窗口内条目仍在 streaming：以数据库终态为准
///   （落库终态是权威事实，必要时由 item identity 回源完整正文）；
/// - revision 相同、只有 contextDisposition 变化：保留内存载荷，只修正 disposition；
/// - 数据库更旧：保留内存载荷（内存可能是更新的 live 预览）。
///
/// 返回 null 表示当前窗口载荷更权威，不采纳该页条目。
ThreadItemView? _preferPageItem(
  ThreadItemView existing,
  ThreadItemView incoming, {
  required int pageWatermark,
  required int workspaceRevision,
}) {
  if (!_sameItemIdentity(existing, incoming)) return null;
  if (incoming.revision < existing.revision) return null;
  final stable = incoming.ordinal == existing.ordinal
      ? incoming
      : incoming.copyWith(ordinal: existing.ordinal);
  if (stable.revision > existing.revision) return stable;
  if (stable.isTerminal && !existing.isTerminal) return stable;
  if (pageWatermark >= workspaceRevision &&
      stable.contextDisposition != existing.contextDisposition) {
    return existing.copyWith(contextDisposition: stable.contextDisposition);
  }
  return null;
}

/// 标记一次 item identity 回源开始：加载状态对 UI 显式可见，并清除旧错误。
StudioState startItemBodyLoad(
  StudioState current,
  String threadId,
  String itemId,
) {
  final ui = current.workspaceUiByThread[threadId];
  if (ui == null ||
      !ui.history.previewedItemIds.contains(itemId) ||
      ui.history.loadingItemIds.contains(itemId)) {
    return current;
  }
  final loadingItemIds = {...ui.history.loadingItemIds, itemId};
  final itemBodyErrors = Map<String, String>.from(ui.history.itemBodyErrors)
    ..remove(itemId);
  final unavailableItemIds = {...ui.history.unavailableItemIds}..remove(itemId);
  return current.copyWith(
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(
        history: ui.history.copyWith(
          loadingItemIds: loadingItemIds,
          itemBodyErrors: itemBodyErrors,
          unavailableItemIds: unavailableItemIds,
        ),
      ),
    },
  );
}

/// 把一次完整正文回源结果合并进窗口：同身份且 revision 不更旧才替换载荷，
/// 随后清除该条目的预览标记、加载标记与错误。
///
/// 回源没有拿到完整正文时分两种情况：
/// - 身份仍在窗口内，只是数据源暂时给不出正文（实时预览先于历史事务 durable，或数据源
///   仍按同身份预览返回）：保留预览并标记为 `pendingItemBodyIds`，入口继续可见可重试，
///   不做“永久不可回源”的结论；
/// - 身份在当前窗口/缓存中确实不存在：这是真正的缺席，才记入 `unavailableItemIds`。
StudioState applyItemBodyPage(
  StudioState current,
  String threadId,
  String itemId,
  TimelinePage page,
) {
  final workspace = current.workspacesByThread[threadId];
  final ui = current.workspaceUiByThread[threadId];
  if (workspace == null || ui == null) return current;
  final incoming = page.items
      .where((item) => item.id == itemId && item.threadId == threadId)
      .firstOrNull;
  final existing = workspace.historyItems
      .where((item) => item.id == itemId)
      .firstOrNull;
  // 数据源仍把该条目按预览返回：回源没有真正取到完整正文，不能假装已回源。
  final stillPreviewed = page.previews.any(
    (preview) => preview.itemId == itemId,
  );
  if (stillPreviewed || (incoming == null && existing != null)) {
    // 条目仍在窗口/缓存里，所以身份仍然存在；只是完整正文此刻还取不到。保留预览与
    // 重试入口（pending），绝不用 unavailable 永久禁用。
    final loadingItemIds = {...ui.history.loadingItemIds}..remove(itemId);
    final itemBodyErrors = Map<String, String>.from(ui.history.itemBodyErrors)
      ..remove(itemId);
    final unavailableItemIds = {...ui.history.unavailableItemIds}
      ..remove(itemId);
    return current.copyWith(
      workspaceUiByThread: {
        ...current.workspaceUiByThread,
        threadId: ui.copyWith(
          history: ui.history.copyWith(
            loadingItemIds: loadingItemIds,
            itemBodyErrors: itemBodyErrors,
            pendingItemBodyIds: {...ui.history.pendingItemBodyIds, itemId},
            unavailableItemIds: unavailableItemIds,
          ),
        ),
      },
    );
  }
  final merged = _mergeItemBody(existing, incoming);
  final loadingItemIds = {...ui.history.loadingItemIds}..remove(itemId);
  final itemBodyErrors = Map<String, String>.from(ui.history.itemBodyErrors)
    ..remove(itemId);
  final pendingItemBodyIds = {...ui.history.pendingItemBodyIds}..remove(itemId);
  // incoming 为 null 且窗口里从未见过该身份：数据源明确不认识它，这才算真正缺席。
  final unavailableItemIds = incoming == null && existing == null
      ? {...ui.history.unavailableItemIds, itemId}
      : ({...ui.history.unavailableItemIds}..remove(itemId));
  final previewedItemIds = merged == null || incoming == null
      ? ui.history.previewedItemIds
      : ({...ui.history.previewedItemIds}..remove(itemId));
  final history = ui.history.copyWith(
    loadingItemIds: loadingItemIds,
    itemBodyErrors: itemBodyErrors,
    previewedItemIds: previewedItemIds,
    pendingItemBodyIds: pendingItemBodyIds,
    unavailableItemIds: unavailableItemIds,
  );
  final nextWorkspace = merged == null
      ? workspace
      : workspace.copyWith(
          items: existing == null
              ? ([...workspace.historyItems, merged]..sort(_compareItems))
              : [
                  for (final item in workspace.historyItems)
                    item.id == itemId ? merged : item,
                ],
        );
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: nextWorkspace,
    },
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(history: history),
    },
  );
}

/// 完整正文回源结果与窗口内同身份条目的合并规则：同身份且 revision 不更旧才替换，
/// 并保留窗口内已加载的 ordinal（ordinal 是不可变顺序事实）。
///
/// 返回 null 表示窗口内载荷更权威（或回源结果缺该条目），不替换。
ThreadItemView? _mergeItemBody(
  ThreadItemView? existing,
  ThreadItemView? incoming,
) {
  if (incoming == null) return null;
  if (existing == null) {
    // 显式回源得到的是完整正文：标记为用户已加载，页面预算不再压缩它。
    return incoming.copyWith(bodyOmittedUnits: 0, bodyLoaded: true);
  }
  if (!_sameItemIdentity(existing, incoming) ||
      incoming.revision < existing.revision) {
    return null;
  }
  final adopted = incoming.ordinal == existing.ordinal
      ? incoming
      : incoming.copyWith(ordinal: existing.ordinal);
  return adopted.copyWith(bodyOmittedUnits: 0, bodyLoaded: true);
}

/// 清除一次回源失败的显式错误状态；条目仍保留预览，允许再次回源。
StudioState failItemBodyLoad(
  StudioState current,
  String threadId,
  String itemId,
  String error,
) {
  final ui = current.workspaceUiByThread[threadId];
  if (ui == null) return current;
  final loadingItemIds = {...ui.history.loadingItemIds}..remove(itemId);
  final itemBodyErrors = Map<String, String>.from(ui.history.itemBodyErrors)
    ..[itemId] = error;
  return current.copyWith(
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(
        history: ui.history.copyWith(
          loadingItemIds: loadingItemIds,
          itemBodyErrors: itemBodyErrors,
        ),
      ),
    },
  );
}

/// 判断一页历史是否比当前窗口更旧或来自另一个数据库实体。
///
/// - 增量页（`replaceWindow == false`）若 database identity 与窗口不一致，说明窗口
///   来自已被重建/替换的数据库，必须拒绝并要求重读，而不是把两份历史拼在一起；
/// - 同一数据库上 watermark 早于窗口已采纳的 applied write sequence 也是过期页；
/// - "替换整个窗口"的读取（首窗、重连、跳回最新）可以采纳新的 database identity。
///
/// 返回 true 表示该页不得并入当前窗口。
bool timelinePageIsStale(
  ThreadHistoryWindow window,
  TimelinePage page, {
  required bool replaceWindow,
}) {
  final windowDatabase = window.databaseId;
  final pageDatabase = page.databaseId;
  final identityKnown = windowDatabase.isNotEmpty && pageDatabase.isNotEmpty;
  if (identityKnown && windowDatabase != pageDatabase) {
    return !replaceWindow;
  }
  if (identityKnown &&
      !replaceWindow &&
      page.watermark < window.appliedWriteSequence) {
    return true;
  }
  return false;
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
  final items = _overlayRolledBackItems(
    workspace.historyItems,
    page.items,
    threadId,
  );
  if (identical(items, workspace.historyItems)) return current;
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

/// Only SQL pages are bounded. Live output is owned by the overlay.
ThreadWorkspace _boundedTimeline(
  ThreadWorkspace workspace,
  TimelineDirection direction, {
  String? anchorId,
}) {
  final all = workspace.historyItems;
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
  final turns = items.map((item) => item.turnId).toSet();
  return workspace.copyWith(
    items: items,
    timelineTurns: {...workspace.timelineTurns}
      ..removeWhere((id, _) => !turns.contains(id)),
  );
}

/// Discard the detached SQL page; keep only live output until Latest arrives.
StudioState jumpTimelineToLatest(StudioState current, String threadId) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null) return current;
  final ui = current.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: workspace.copyWith(items: const [], timelineTurns: const {}),
    },
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(
        history: ThreadHistoryWindow(
          epoch: ui.history.epoch + 1,
          databaseId: ui.history.databaseId,
          appliedWriteSequence: ui.history.appliedWriteSequence,
        ),
      ),
    },
  );
}

/// 判断一个窗口是否仍持有任何需要释放的状态（游标、身份、预览/回源、错误或锚点）。
bool _historyWindowTouched(ThreadHistoryWindow? window) {
  if (window == null) return false;
  return window.hasOlder ||
      window.hasNewer ||
      window.olderCursor != null ||
      window.newerCursor != null ||
      window.errorMessage != null ||
      window.newerError != null ||
      window.databaseId.isNotEmpty ||
      window.appliedWriteSequence != 0 ||
      window.previewedItemIds.isNotEmpty ||
      window.loadingItemIds.isNotEmpty ||
      window.pendingItemBodyIds.isNotEmpty ||
      window.itemBodyErrors.isNotEmpty ||
      window.unavailableItemIds.isNotEmpty;
}

/// 切换/关闭 Thread 时释放该会话的历史载荷。
///
/// SQL window, live overlay and Turn summaries are released together on switch.
/// 阅读锚点保留为轻量身份；重新选中时围绕它从 SQL 重建当前阅读位置。
StudioState releaseThreadHistoryPayload(StudioState current, String threadId) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null) return current;
  final ui = current.workspaceUiByThread[threadId];
  final alreadyEmpty =
      workspace.historyItems.isEmpty &&
      workspace.liveItems.isEmpty &&
      workspace.timelineTurns.isEmpty &&
      !_historyWindowTouched(ui?.history);
  if (alreadyEmpty) return current;
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: workspace.copyWith(
        items: const [],
        liveItems: const {},
        timelineTurns: const {},
      ),
    },
    workspaceUiByThread: ui == null
        ? current.workspaceUiByThread
        : {
            ...current.workspaceUiByThread,
            threadId: ui.copyWith(
              history: ThreadHistoryWindow(
                anchor: ui.history.anchor,
                detached: ui.history.detached,
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

/// Live frames never mutate the SQL window. The writer owns durable content.
ThreadWorkspace? _upsertThreadItem(
  ThreadWorkspace workspace,
  int workspaceRevision,
  ThreadItemView incoming,
) {
  if (incoming.threadId != workspace.thread.id || incoming.id.isEmpty) {
    return null;
  }
  final existing =
      workspace.liveItems[incoming.id] ??
      workspace.historyItems
          .where((item) => item.id == incoming.id)
          .firstOrNull;
  if (existing != null) {
    if (!_sameItemIdentity(existing, incoming) ||
        incoming.revision < existing.revision) {
      return workspace.copyWith(revision: workspaceRevision);
    }
    if (existing.isTerminal && incoming.revision <= existing.revision) {
      return workspace.copyWith(revision: workspaceRevision);
    }
  }
  return workspace.copyWith(
    revision: workspaceRevision,
    liveItems: {
      ...workspace.liveItems,
      incoming.id: existing == null || incoming.ordinal == existing.ordinal
          ? incoming
          : incoming.copyWith(ordinal: existing.ordinal),
    },
  );
}

ThreadWorkspace? _appendThreadItemDelta(
  ThreadWorkspace workspace,
  int workspaceRevision,
  ThreadItemDeltaView delta,
) {
  final item = workspace.liveItems[delta.itemId];
  if (item == null || item.isTerminal) return null;
  if (delta.revision <= item.revision) {
    return workspace.copyWith(revision: workspaceRevision);
  }
  if (delta.revision != item.revision + 1) return null;
  final nextItem = item.appendDelta(
    delta: delta.state,
    nextRevision: delta.revision,
  );
  if (nextItem == null) return null;
  return workspace.copyWith(
    revision: workspaceRevision,
    liveItems: {...workspace.liveItems, nextItem.id: nextItem},
  );
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

/// Turn 通知是当前状态事实：busy Turn 进入 activeTurn，终态 Turn 清空它并更新
/// 最近 Turn 事实。迟到的 busy 载荷不能把已终态 Turn 拉回运行中。
ThreadWorkspace _applyThreadTurn(
  ThreadWorkspace workspace,
  int revision,
  StudioTurnView turn,
) {
  final observed = workspace.latestTurn;
  if (observed != null &&
      observed.turnId == turn.turnId &&
      observed.state.isTerminal &&
      turn.state.isBusy) {
    return workspace.copyWith(revision: revision);
  }
  final active = workspace.activeTurn;
  return workspace.copyWith(
    revision: revision,
    activeTurn: turn.state.isBusy
        ? turn
        : (active != null && active.turnId == turn.turnId ? null : active),
    latestTurn: _newestTurn(observed, turn),
  );
}
