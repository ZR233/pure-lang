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
  if (previous != null && snapshot.revision < previous.revision) {
    return current;
  }
  // 首次订阅没有可复用的窗口：快照本身必须是不带条目的当前状态。
  final base =
      previous ??
      snapshot.copyWith(
        items: const [],
        cachedItems: const {},
        latestItemIds: const [],
        timelineTurns: const {},
      );
  final directory = current.threads
      .where((thread) => thread.id == threadId)
      .firstOrNull;
  final next = base.copyWith(
    thread: directory ?? snapshot.thread,
    revision: snapshot.revision,
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
/// 不同 Turn 以 updatedAt 判定先后。
StudioTurnView? _newestTurn(StudioTurnView? left, StudioTurnView? right) {
  if (left == null) return right;
  if (right == null) return left;
  if (left.turnId == right.turnId) {
    if (right.revision == left.revision) {
      return right.state.isBusy && left.state.isTerminal ? left : right;
    }
    return right.revision > left.revision ? right : left;
  }
  return right.updatedAt.isAfter(left.updatedAt) ? right : left;
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
  final followBottom = !_workspaceUi(current, threadId).history.detached;
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
      followBottom: followBottom,
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
  final eviction = _liveEvictionHistory(syncedUi, workspace, updated);
  final nextUi = eviction == null
      ? syncedUi
      : syncedUi.copyWith(history: eviction);
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

/// 实时流导致的窗口淘汰必须同时前移历史窗口的旧边界。
///
/// 跟随底部时 `_boundedTimeline(..., TimelineDirection.newer)` 会把最旧的常驻条目挤出
/// 窗口与缓存；如果只有 `applyTimelinePage` 会更新 `hasOlder`/`olderCursor`，被实时裁剪
/// 过的窗口就永远分不出更旧的 SQL 页（`onLoadOlder` 一直为 null，用户滚到顶也回不去）。
///
/// 规则：
/// - 触发条件是**窗口最旧条目真的离开窗口与缓存**（真实淘汰），不是"有新数据流入"；
///   因此不会因为尚未持久化的新帧到达就宣称存在更旧历史；
/// - 新的旧边界就是淘汰后的第一条常驻条目本身（canonical item identity，与
///   `applyTimelinePage` 在自身裁剪时写入的原始身份形式一致），由存储侧按该 identity
///   解析 ordinal，页面仍只读 SQL；
/// - 数据库身份、已采纳写水位、newer 边界、锚点、epoch 与 detached 都不动：这里只表达
///   "阅读窗口的旧边界前移"，不引入新页、不伪造水位、不改变用户位置；
/// - 已由 SQL 页携带的游标在旧边界未移动时被完整保留（见 `jumpTimelineToLatest`）。
ThreadHistoryWindow? _liveEvictionHistory(
  WorkspaceUiState ui,
  ThreadWorkspace previous,
  ThreadWorkspace next,
) {
  final previousFirst = previous.items.firstOrNull;
  final nextFirst = next.items.firstOrNull;
  if (previousFirst == null ||
      nextFirst == null ||
      previousFirst.id == nextFirst.id) {
    return null;
  }
  // 旧首条仍在窗口或实时尾部/缓存里，说明只是窗口方向变化而非淘汰。
  if (next.items.any((item) => item.id == previousFirst.id) ||
      next.cachedItems.containsKey(previousFirst.id)) {
    return null;
  }
  if (ui.history.hasOlder && ui.history.olderCursor == nextFirst.id) {
    return null;
  }
  return ui.history.copyWith(hasOlder: true, olderCursor: nextFirst.id);
}

/// 由窗口条目的正文状态推导预览/加载/错误标记：条目本身是唯一事实源。
///
/// 实时有界预览与历史页预览因此落到同一个状态；条目被显式回源（`bodyLoaded`）
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
    for (final item in workspace.items)
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

/// 历史页只拥有它声明的范围。[replaceWindow] 表示用新的权威窗口（首窗、重连或
/// 跳回最新）替换整个窗口；否则按方向合并。活跃预览不被较旧的历史载荷覆盖。
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
  final pageIds = {for (final item in pageItems) item.id};
  final firstPageOrdinal = pageItems.firstOrNull?.ordinal;
  final lastPageOrdinal = pageItems.lastOrNull?.ordinal;
  final cache = {
    ...workspace.cachedItems,
    for (final item in workspace.items) item.id: item,
  };
  // 页声明的单条预览省略量：页正文未超预算时归零，超预算时由客户端预算接管。
  final pagePreviewOmitted = {
    for (final preview in page.previews) preview.itemId: preview.omittedBytes,
  };
  for (final item in pageItems) {
    final existing = cache[item.id];
    if (existing == null) {
      cache[item.id] = boundThreadItemBody(
        item,
        previewOmittedUnits: pagePreviewOmitted[item.id] ?? 0,
      );
      continue;
    }
    final merged = _preferPageItem(
      existing,
      item,
      pageWatermark: page.watermark,
      workspaceRevision: workspace.revision,
    );
    if (merged == null) continue;
    if (existing.bodyLoaded && merged.revision <= existing.revision) {
      // 用户已显式回源的完整正文不被同 revision 的页载荷退回预览。
      continue;
    }
    cache[item.id] = boundThreadItemBody(
      merged,
      previewOmittedUnits: pagePreviewOmitted[item.id] ?? 0,
    );
  }
  // 替换整个窗口时仍保留页范围之外的实时条目：跟随底部时留在窗口，已离开
  // 底部时只进实时尾部，避免移动用户正在阅读的范围。
  final liveOutside = [
    for (final item in workspace.items)
      if (!pageIds.contains(item.id) &&
          firstPageOrdinal != null &&
          lastPageOrdinal != null &&
          (item.ordinal < firstPageOrdinal || item.ordinal > lastPageOrdinal))
        item.id,
  ];
  final tail = [
    if (replaceWindow && !followBottom) ...liveOutside,
    ...workspace.latestItemIds.where((id) => !pageIds.contains(id)),
  ];
  final ids = {
    if (!replaceWindow) ...workspace.items.map((item) => item.id),
    if (replaceWindow && followBottom) ...liveOutside,
    ...pageIds,
  };
  final items = [for (final id in ids) cache[id]!]..sort(_compareItems);
  final trimmed = items.length > maxTimelineWindowItems;
  var latestTurn = workspace.latestTurn;
  for (final entry in page.turns) {
    latestTurn = _newestTurn(latestTurn, entry.turn);
  }
  final next = _boundedTimeline(
    workspace.copyWith(
      items: items,
      cachedItems: cache,
      latestItemIds: tail,
      latestTurn: latestTurn,
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
  // 窗口身份：database identity 与 applied write sequence 一起表达"窗口的水位"；
  // 数据库重建或水位回退时窗口整体过期，由页携带的身份纠正（见 timelinePageIsStale）。
  // 预览/加载状态只由条目的正文状态推导：窗口内条目是唯一事实源，实时有界预览与
  // 历史页预览落到同一状态，显式回源完成后标记自然消失。
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
  // 跟随底部的窗口（首窗/重连/跳回最新）只在**确实存在更旧历史**时声明旧边界。
  //
  // 窗口里有条目本身不构成"可以继续向旧翻页"的证据：页既没有给出更旧游标、也没有
  // 裁剪出更旧范围时，`olderCursor` 必须保持为空。否则一个没有更旧历史的阅读窗口会
  // 带着一个凭空而来的旧边界（离开底部或被实时流推进后仍在状态里），把"窗口里有内容"
  // 误表示成"旧边界已前移"。
  //
  // 真裁剪（页范围比窗口更旧）用裁剪后的第一条常驻身份——与实时淘汰写游标的形式一致；
  // 边界未移动时保留页携带的游标 token，交给存储侧解析身份与水位，不退化成原始身份。
  final followHasOlder = droppedOlder || page.olderCursor != null;
  final followOlderCursor = !followHasOlder
      ? null
      : droppedOlder
      ? next.items.firstOrNull?.id
      : page.olderCursor ?? next.items.firstOrNull?.id;
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
  final existing =
      workspace.cachedItems[itemId] ??
      workspace.items.where((item) => item.id == itemId).firstOrNull;
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
              ? ([...workspace.items, merged]..sort(_compareItems))
              : [
                  for (final item in workspace.items)
                    item.id == itemId ? merged : item,
                ],
          cachedItems: {...workspace.cachedItems, merged.id: merged},
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

/// 最新尾部独立上限：detached 读者的实时尾部最多 400 条，与阅读窗口（500）分开计费。
///
/// 两者共享一个上限会让尾部比设计契约多出 100 条——离开底部的读者会多保留 100 条
/// 永远不会进入窗口的载荷（design/19-studio-ui.md 的“500 窗口 + 400 尾部”）。
const int maxLiveTailItems = 400;

/// 实时尾部有界：超出上限时只保留最新的 [maxLiveTailItems] 条（尾部语义是“比窗口新”）。
List<String> _boundedTailIds(List<String> ids) {
  if (ids.length <= maxLiveTailItems) return ids;
  return ids.sublist(ids.length - maxLiveTailItems);
}

/// 有界 Timeline 窗口：超出上限时按方向淘汰远端（[TimelineDirection.older]
/// 保留更旧一侧、[TimelineDirection.newer] 保留更新一侧），并保证 [anchorId]
/// 仍在窗口内。实时尾部只保留窗口之外的条目。
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
  final windowIds = {for (final item in items) item.id};
  final tail = _boundedTailIds([
    for (final id in workspace.latestItemIds)
      if (!windowIds.contains(id)) id,
  ]);
  final ids = {...windowIds, ...tail};
  final cache = {
    ...workspace.cachedItems,
    for (final item in items) item.id: item,
  }..removeWhere((id, _) => !ids.contains(id));
  final turns = cache.values.map((item) => item.turnId).toSet();
  return workspace.copyWith(
    items: [for (final item in items) cache[item.id]!],
    cachedItems: cache,
    latestItemIds: tail,
    timelineTurns: {...workspace.timelineTurns}
      ..removeWhere((id, _) => !turns.contains(id)),
  );
}

/// 回到最新：先用实时尾部本地回显（随后由控制器的权威最新窗口替换），并提升
/// epoch 使在途的旧分页响应失效。
StudioState jumpTimelineToLatest(StudioState current, String threadId) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null) return current;
  final ui = current.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
  final items = [...workspace.items, ...workspace.latestItems];
  final windowIds = {for (final item in items) item.id};
  final previewedItemIds = ui.history.previewedItemIds
      .where(windowIds.contains)
      .toSet();
  final loadingItemIds = ui.history.loadingItemIds
      .where(windowIds.contains)
      .toSet();
  final pendingItemBodyIds = ui.history.pendingItemBodyIds
      .where(windowIds.contains)
      .toSet();
  final itemBodyErrors = {
    for (final entry in ui.history.itemBodyErrors.entries)
      if (windowIds.contains(entry.key)) entry.key: entry.value,
  };
  final unavailableItemIds = ui.history.unavailableItemIds
      .where(windowIds.contains)
      .toSet();
  // 跳到最新时窗口的旧边界没有移动（首条仍是原来的首条），因此必须保留下方已由 SQL
  // 页携带的游标 token，而不是把它丢掉退化成原始 item identity。
  final olderEdgeMoved =
      workspace.items.firstOrNull?.id != items.firstOrNull?.id;
  final hasOlder = ui.history.hasOlder || olderEdgeMoved;
  final olderCursor = !hasOlder
      ? null
      : olderEdgeMoved
      ? items.firstOrNull?.id
      : ui.history.olderCursor ?? items.firstOrNull?.id;
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: _boundedTimeline(
        workspace.copyWith(items: items, latestItemIds: const []),
        TimelineDirection.newer,
      ),
    },
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(
        history: ThreadHistoryWindow(
          isLoading: ui.history.isLoading,
          direction: ui.history.direction,
          hasOlder: hasOlder,
          olderCursor: olderCursor,
          epoch: ui.history.epoch + 1,
          databaseId: ui.history.databaseId,
          appliedWriteSequence: ui.history.appliedWriteSequence,
          previewedItemIds: previewedItemIds,
          loadingItemIds: loadingItemIds,
          itemBodyErrors: itemBodyErrors,
          pendingItemBodyIds: pendingItemBodyIds,
          unavailableItemIds: unavailableItemIds,
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
      window.epoch != 0 ||
      window.detached ||
      window.anchor != null ||
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
/// 有界 Timeline 窗口、实时尾部、缓存载荷与 Turn 摘要全部丢弃，预览/回源状态一并
/// 清空，因此内存只随当前窗口增长；Thread 身份、交互、runtime、Todo 与 composer/UI
/// 状态保留。重新选中该会话时由新订阅与首窗读取重建窗口。
StudioState releaseThreadHistoryPayload(StudioState current, String threadId) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null) return current;
  final ui = current.workspaceUiByThread[threadId];
  final alreadyEmpty =
      workspace.items.isEmpty &&
      workspace.cachedItems.isEmpty &&
      workspace.latestItemIds.isEmpty &&
      workspace.timelineTurns.isEmpty &&
      !_historyWindowTouched(ui?.history);
  if (alreadyEmpty) return current;
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: workspace.copyWith(
        items: const [],
        cachedItems: const {},
        latestItemIds: const [],
        timelineTurns: const {},
      ),
    },
    workspaceUiByThread: ui == null
        ? current.workspaceUiByThread
        : {
            ...current.workspaceUiByThread,
            threadId: ui.copyWith(history: const ThreadHistoryWindow()),
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

/// Timeline item 的唯一合并规则（实时帧与历史页共用）：
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

/// 实时 Item 只进入有界窗口（跟随底部）或实时尾部（已离开底部）；身份不可变、
/// 旧 revision 不覆盖，载荷来源与历史页完全一致（同一身份 + revision 规则）。
ThreadWorkspace? _upsertThreadItem(
  ThreadWorkspace workspace,
  int workspaceRevision,
  ThreadItemView incoming, {
  required bool followBottom,
}) {
  if (incoming.threadId != workspace.thread.id || incoming.id.isEmpty) {
    return null;
  }
  return _insertLiveItem(
    workspace,
    incoming,
    followBottom: followBottom,
  ).copyWith(revision: workspaceRevision);
}

ThreadWorkspace _insertLiveItem(
  ThreadWorkspace workspace,
  ThreadItemView incoming, {
  required bool followBottom,
}) {
  final existing = workspace.cachedItems[incoming.id];
  if (existing != null) {
    if (!_sameItemIdentity(existing, incoming) ||
        incoming.revision < existing.revision) {
      return workspace;
    }
    final adopted = incoming.ordinal == existing.ordinal
        ? incoming
        : incoming.copyWith(ordinal: existing.ordinal);
    // 用户已显式回源的完整正文不被同 revision 的实时/快照载荷退回预览。
    final merged = existing.bodyLoaded && adopted.revision <= existing.revision
        ? existing
        : boundThreadItemBody(adopted);
    return workspace.copyWith(
      items: [
        for (final item in workspace.items)
          item.id == merged.id ? merged : item,
      ],
      cachedItems: {...workspace.cachedItems, merged.id: merged},
    );
  }
  final bounded = boundThreadItemBody(incoming);
  if (!followBottom) {
    return _boundLiveTail(
      workspace.copyWith(
        cachedItems: {...workspace.cachedItems, bounded.id: bounded},
        latestItemIds: [...workspace.latestItemIds, bounded.id],
      ),
    );
  }
  final merged = mergeThreadItems(workspace, [bounded]);
  if (merged == null) return workspace;
  return _boundedTimeline(merged, TimelineDirection.newer);
}

/// 实时尾部有界：超过 [maxLiveTailItems] 时淘汰最旧的尾部载荷，窗口内条目的载荷不受影响。
ThreadWorkspace _boundLiveTail(ThreadWorkspace workspace) {
  final bounded = _boundedTailIds(workspace.latestItemIds);
  if (bounded.length == workspace.latestItemIds.length) return workspace;
  final kept = bounded.toSet();
  final windowIds = {for (final item in workspace.items) item.id};
  return workspace.copyWith(
    latestItemIds: bounded,
    cachedItems: {...workspace.cachedItems}
      ..removeWhere((id, _) => !kept.contains(id) && !windowIds.contains(id)),
  );
}

ThreadWorkspace? _appendThreadItemDelta(
  ThreadWorkspace workspace,
  int workspaceRevision,
  ThreadItemDeltaView delta,
) {
  final items = [...workspace.items];
  final index = items.indexWhere((item) => item.id == delta.itemId);
  if (index < 0) {
    // Delta 只命中窗口或实时尾部中的未终态 Item；未知 Item 说明流与窗口
    // 已不连续，必须重新订阅并由数据库窗口校正。
    final tail = workspace.cachedItems[delta.itemId];
    if (tail == null || tail.isTerminal) return null;
    if (delta.revision <= tail.revision) {
      return workspace.copyWith(revision: workspaceRevision);
    }
    if (delta.revision != tail.revision + 1) return null;
    final nextTail = tail.appendDelta(
      delta: delta.state,
      nextRevision: delta.revision,
    );
    if (nextTail == null) return null;
    return workspace.copyWith(
      revision: workspaceRevision,
      cachedItems: {...workspace.cachedItems, nextTail.id: nextTail},
    );
  }
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
  return workspace.copyWith(
    revision: workspaceRevision,
    items: items,
    cachedItems: {...workspace.cachedItems, nextItem.id: nextItem},
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
