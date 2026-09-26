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
/// Interaction、runtime、Todo、typed 活动与 typed 存储状态。有界消息窗口属于阅读面，
/// 只由 ChatView 维护；snapshot 不携带条目，也不与旧条目混合。
StudioState applyThreadSnapshot(StudioState current, ThreadWorkspace snapshot) {
  final threadId = snapshot.thread.id;
  if (threadId.isEmpty) return current;
  final previous = current.workspacesByThread[threadId];
  // Each accepted snapshot begins a new subscription generation. Its revision may be lower
  // than the old generation's live-notification revision; only that generation's frames may
  // advance it.
  final base = previous ?? snapshot.copyWith(items: const []);
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
    activity: snapshot.activity,
    storage: snapshot.storage,
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
  // 状态流不携带条目：正文只由 ChatView 窗口交付，这里只应用状态事实。
  final updated = switch (update) {
    ThreadTurnUpdate(:final turn) => _applyThreadTurn(
      workspace,
      revision,
      turn,
    ),
    ThreadActivityUpdate(:final activity) => applyThreadActivity(
      workspace,
      revision,
      activity,
    ),
    ThreadInteractionUpdate(:final interaction, :final pending) =>
      _updateThreadInteraction(workspace, revision, interaction, pending),
    ThreadRuntimeUpdate(:final runtime, :final todo) => workspace.copyWith(
      revision: revision,
      runtime: runtime,
      todo: todo,
    ),
    ThreadStorageUpdate(:final storage) => applyThreadStorage(
      workspace,
      revision,
      storage,
    ),
  };
  if (updated == null) {
    return StudioReduceResult(current, resyncThreadId: threadId);
  }
  final ui = _workspaceUi(current, threadId);
  final syncedUi = syncItemBodyState(ui, updated);
  return StudioReduceResult(
    identical(syncedUi, ui)
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
              threadId: syncedUi,
            },
          ),
  );
}

/// 应用后端 typed 活动变化。
///
/// 同一身份内只按 revision 前进（迟到/重复的旧版本被丢弃）；身份变化即换活动并重置
/// 已缓存详情；`null` 清除当前活动（Turn 结束）。
ThreadWorkspace? applyThreadActivity(
  ThreadWorkspace workspace,
  int revision,
  ThreadActivityView? activity,
) {
  if (activity == null) {
    return workspace.copyWith(revision: revision, activity: null);
  }
  final existing = workspace.activity;
  if (existing != null && existing.identity == activity.identity) {
    if (activity.revision < existing.revision) {
      // 迟到旧版本：不改变活动，只推进流水位。
      return workspace.copyWith(revision: revision);
    }
  }
  return workspace.copyWith(revision: revision, activity: activity);
}

/// 应用后端 typed 存储状态变化。
///
/// 与活动一样只按 typed 事实落地：`null` 表示当前没有可报告的存储事实（未知，不是健康），
/// 直接写入；界面据此显示“等待保存/明确暂停原因”，不自动恢复、不从错误文本推断故障类别。
ThreadWorkspace? applyThreadStorage(
  ThreadWorkspace workspace,
  int revision,
  ThreadStorageStateView? storage,
) {
  return workspace.copyWith(revision: revision, storage: storage);
}

/// ChatView owns the complete bounded reading window. The execution stream may
/// advance its own revision, but cannot add a second copy of these items.
StudioState applyChatWindowSnapshot(
  StudioState current,
  String threadId,
  StudioChatSnapshot snapshot,
) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null) return current;
  // 窗口是唯一正文 owner：条目直接来自权威窗口，不再叠加任何本地补齐副本或第二份事实。
  final items = [for (final entry in snapshot.items) entry.item];
  final next = workspace.copyWith(items: items);
  final ui = syncItemBodyState(_workspaceUi(current, threadId), next);
  final history = ui.history.copyWith(
    hasOlder: snapshot.hasOlder,
    hasNewer: snapshot.hasNewer,
    olderCursor: items.firstOrNull?.id,
    newerCursor: items.lastOrNull?.id,
    isLoading: false,
    errorMessage: null,
    newerError: null,
    detached: snapshot.focusedItemId != null,
    anchor: snapshot.focusedItemId == null
        ? null
        : TimelineAnchor(
            snapshot.focusedItemId!,
            ui.history.anchor?.offset ?? 0,
          ),
  );
  return current.copyWith(
    workspacesByThread: {...current.workspacesByThread, threadId: next},
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(history: history),
    },
  );
}

WorkspaceUiState _workspaceUi(StudioState state, String threadId) {
  return state.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
}

/// 由窗口条目的正文状态推导预览/加载/错误标记：条目本身是唯一事实源。
///
/// 历史页预览因此落到同一个状态；条目被按身份展开（窗口 `omittedBytes` 归零）
/// 或正文回到完整时，预览标记自然消失，不会残留成第二份事实。
///
/// 回源相关的 UI 状态（加载/待落盘/错误/缺席）只在条目仍然“需要回源”时才有意义：
/// 正文已经完整或已补齐的条目会连同旧标记一起清掉，避免出现指向已完整正文的
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

/// 用一页 canonical 历史替换或并入阅读窗口。
///
/// 这是窗口**之外**的显式历史读取（冷历史分页）；它读取权威条目本身，不叠加任何 state
/// snapshot 正文或第二份 live overlay。生产路径的正文只由 ChatView 窗口交付（含按身份
/// 补齐完整正文），因此本入口只对不带内容窗口的 API 生效。
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
  final merged = <String, ThreadItemView>{
    if (!replaceWindow)
      for (final item in workspace.items) item.id: item,
  };
  final pagePreviewOmitted = {
    for (final preview in page.previews) preview.itemId: preview.omittedBytes,
  };
  for (final item in pageItems) {
    final existing = merged[item.id];
    final selected = existing == null
        ? item
        : _preferPageItem(
                existing,
                item,
                pageWatermark: page.watermark,
                workspaceRevision: workspace.revision,
              ) ??
              existing;
    merged[item.id] = boundThreadItemBody(
      selected,
      previewOmittedUnits: pagePreviewOmitted[item.id] ?? 0,
    );
  }
  final items = merged.values.toList()..sort(_compareItems);
  final trimmed = items.length > maxTimelineWindowItems;
  var latestTurn = workspace.latestTurn;
  for (final entry in page.turns) {
    latestTurn = _newestTurn(latestTurn, entry.turn);
  }
  final confirmedItems = merged.values.toList()..sort(_compareItems);
  final next = _boundedTimeline(
    workspace.copyWith(items: confirmedItems, latestTurn: latestTurn),
    direction,
    anchorId: ui.history.anchor?.itemId,
  );
  final droppedOlder = next.items.firstOrNull?.id != items.firstOrNull?.id;
  final droppedNewer = next.items.lastOrNull?.id != items.lastOrNull?.id;
  final synced = syncItemBodyState(ui, next);
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
      ? next.items.firstOrNull?.id
      : page.olderCursor ?? next.items.firstOrNull?.id;
  final history = ui.history.copyWith(
    olderCursor: followBottom
        ? followOlderCursor
        : droppedOlder
        ? next.items.firstOrNull?.id
        : (replaceWindow || direction == TimelineDirection.older)
        ? page.olderCursor
        : ui.history.olderCursor,
    newerCursor: followBottom
        ? null
        : droppedNewer
        ? next.items.lastOrNull?.id
        : (replaceWindow || direction == TimelineDirection.newer)
        ? page.newerCursor
        : ui.history.newerCursor,
    hasOlder: followBottom
        ? followHasOlder
        : droppedOlder ||
              ((replaceWindow || direction == TimelineDirection.older)
                  ? page.olderCursor != null
                  : ui.history.hasOlder),
    hasNewer: followBottom
        ? droppedNewer
        : droppedNewer ||
              ((replaceWindow || direction == TimelineDirection.newer)
                  ? page.newerCursor != null
                  : ui.history.hasNewer),
    detached: followBottom
        ? false
        : replaceWindow ||
              ui.history.detached ||
              (trimmed && direction == TimelineDirection.older),
    isLoading: false,
    errorMessage:
        followBottom || replaceWindow || direction == TimelineDirection.older
        ? null
        : ui.history.errorMessage,
    newerError:
        followBottom || replaceWindow || direction == TimelineDirection.newer
        ? null
        : ui.history.newerError,
    databaseId: databaseId,
    appliedWriteSequence: appliedWriteSequence,
    previewedItemIds: synced.history.previewedItemIds,
    loadingItemIds: synced.history.loadingItemIds,
    itemBodyErrors: synced.history.itemBodyErrors,
    pendingItemBodyIds: synced.history.pendingItemBodyIds,
    unavailableItemIds: synced.history.unavailableItemIds,
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

/// 标记一次按身份正文补齐“已发出、但完整正文此刻还取不到”。
///
/// 身份仍在窗口内，只是数据源暂时给不出正文（实时预览先于历史事务 durable）。保留预览与
/// 重试入口，不做“永久不可回源”的结论，也不本地编造正文。
StudioState markItemBodyPending(
  StudioState current,
  String threadId,
  String itemId,
) {
  final ui = current.workspaceUiByThread[threadId];
  if (ui == null) return current;
  return current.copyWith(
    workspaceUiByThread: {
      ...current.workspaceUiByThread,
      threadId: ui.copyWith(
        history: ui.history.copyWith(
          loadingItemIds: {...ui.history.loadingItemIds}..remove(itemId),
          pendingItemBodyIds: {...ui.history.pendingItemBodyIds, itemId},
          itemBodyErrors: Map<String, String>.from(ui.history.itemBodyErrors)
            ..remove(itemId),
        ),
      ),
    },
  );
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

const int maxTimelineWindowItems = 100;

/// 有界窗口只裁剪窗口自己：不引入任何 overlay 或第二份事实源。
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
  return workspace.copyWith(items: items);
}

/// 丢弃当前窗口并请求权威 Latest 窗口（换 epoch，下一次窗口读取必须整窗替换）。
StudioState jumpTimelineToLatest(StudioState current, String threadId) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null) return current;
  final ui = current.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: workspace.copyWith(items: const []),
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

/// 切换/关闭 Thread 时释放该会话的窗口载荷。
///
/// 窗口条目与阅读相关的 UI 标记一起释放；阅读锚点保留为轻量身份，重新选中时围绕它
/// 重建当前窗口。
StudioState releaseThreadHistoryPayload(StudioState current, String threadId) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null) return current;
  final ui = current.workspaceUiByThread[threadId];
  final alreadyEmpty =
      workspace.items.isEmpty && !_historyWindowTouched(ui?.history);
  if (alreadyEmpty) return current;
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: workspace.copyWith(items: const []),
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

/// Live frames never mutate the SQL window. The writer owns durable content.
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
