import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/foundation.dart'
    show debugPrint, kDebugMode, visibleForTesting;
import 'package:flutter/scheduler.dart' show SchedulerBinding;
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../../domain/models/studio_models.dart';
import '../../platform/clipboard_image_reader.dart';
import '../frb/studio_api.dart';
import 'studio_api_provider.dart';
import 'studio_state_reducer.dart';
import 'studio_stream_coordinators.dart';

part 'studio_controller.g.dart';

Duration? _disableStudioRetry(int retryCount, Object error) => null;

@Riverpod(keepAlive: true, retry: _disableStudioRetry)
class StudioController extends _$StudioController {
  static bool _startupProjectActivated = false;

  late ProductStreamCoordinator _productCoordinator;
  late ThreadStreamCoordinator _threadCoordinator;
  final Set<String> _historyRequests = {};
  final Map<String, int> _windowLoadGeneration = {};
  final Map<String, Timer> _terminalRefreshTimers = {};
  final List<(ThreadNotificationFrame, String, int)> _pendingThreadDeltas = [];
  StudioChatWindow? _chatWindow;
  String? _chatWindowThreadId;
  String? _chatFocusedItemId;
  int _chatWindowOperation = 0;
  Future<void>? _openingChatWindow;
  bool _deltaFrameScheduled = false;
  int _debugThreadFrameCount = 0;
  int _debugThreadFrameMicros = 0;
  int _debugThreadFrameMaxMicros = 0;
  int _debugThreadFrameStartedAt = 0;

  /// 每个会话已观测到的实时广播 epoch；临时 map，断开或重订阅时释放。
  final Map<String, int> _streamEpochByThread = {};
  final Set<String> _archivingThreadIds = {};
  final Set<String> _renamingThreadIds = {};

  StudioApi get _api => ref.read(studioApiProvider);

  bool _isInitialized(StudioState? current) => current != null;

  @override
  Future<StudioState> build() async {
    _productCoordinator = ProductStreamCoordinator(
      _api,
      _handleProductEvent,
      _onProductStreamTerminated,
    );
    _threadCoordinator = ThreadStreamCoordinator(
      _api,
      _handleThreadFrame,
      _markThreadDisconnected,
    );
    ref.onDispose(() {
      _closeChatWindow();
      unawaited(_productCoordinator.dispose());
      unawaited(_threadCoordinator.dispose());
      _windowLoadGeneration.clear();
      _streamEpochByThread.clear();
      _historyRequests.clear();
      for (final timer in _terminalRefreshTimers.values) {
        timer.cancel();
      }
      _terminalRefreshTimers.clear();
      _pendingThreadDeltas.clear();
    });
    final startupWatch = Stopwatch()..start();
    final catalog = await _api.loadProviderCatalog();
    final snapshot = await _api.readStudioState();
    final bootstrapped = _resolveSelection(
      _attachProviderCatalog(snapshot, catalog),
      previous: null,
      intent: _BootstrapSelection(
        preferredProjectId: snapshot.selectedProjectId,
        preferredThreadId: snapshot.selectedThreadId,
      ),
    );
    _productCoordinator.start();
    // 启动只读取全局配置、工作区与会话目录，并恢复“选择”；首个 GUI 帧
    // 不加载会话状态或历史。GUI 在首帧之后通过 openSelectedThread 打开当前会话，
    // 不自动恢复模型或工具执行。
    _activateStartupProject(bootstrapped);
    debugPrint(
      'startup_stage=controller_ready elapsed_ms=${startupWatch.elapsedMilliseconds}',
    );
    return bootstrapped;
  }

  /// 重置启动激活 guard，仅用于隔离测试。
  @visibleForTesting
  static void resetStartupProjectActivation() {
    _startupProjectActivated = false;
  }

  void _activateStartupProject(StudioState bootstrapped) {
    if (_startupProjectActivated) return;
    final projectId = bootstrapped.selectedProjectId;
    if (projectId == null ||
        bootstrapped.recoveryIssue(
              scope: RecoveryIssueScope.project,
              projectId: projectId,
            ) !=
            null) {
      return;
    }
    _startupProjectActivated = true;
    unawaited(_activateProjectInBackground(projectId));
  }

  Future<void> _activateProjectInBackground(String projectId) async {
    try {
      await _api.activateProject(projectId);
    } catch (_) {
      // 启动激活是后台任务：失败由事件流表达，不阻断启动。
    }
  }

  Future<void> openProject(String path) async {
    if (!_isInitialized(state.value)) return;
    final project = await _api.openProject(path);
    await _api.activateProject(project.id);
    await _reloadProductState(selection: _ProjectDefaultSelection(project.id));
  }

  /// 打开远端项目的结果合同。
  ///
  /// 返回 true 仅当请求真正执行、同步后的 canonical project snapshot 成功采用，
  /// 且 adopted 状态中被选中的项目正是刚打开的项目：其 id 匹配、`sshAlias`
  /// 等于请求的别名、path 与后端返回的 canonical 项目路径一致。后端 snapshot
  /// 只拥有 Project 目录，不拥有 Flutter 当前选择；选择由显式 intent 在采用时解析。
  /// controller 尚未初始化、打开失败或 snapshot 未包含该 canonical Project 都返回
  /// false。调用方只有收到 true 才应关闭打开窗口；false 时应保留窗口与输入并允许重试。
  Future<bool> openRemoteProject(
    String alias,
    String path, {
    String? name,
  }) async {
    final current = state.value;
    if (!_isInitialized(current)) return false;
    StudioProject project;
    try {
      project = await _api.openRemoteProject(alias, path);
      if (name != null && name != project.name) {
        project = await _api.renameProject(project.id, name);
      }
      await _api.activateProject(project.id);
    } on Object {
      return false;
    }
    if (!ref.mounted) return false;
    return _adoptSelectedProject(
      expectedId: project.id,
      expectedAlias: alias,
      expectedPath: project.path,
    );
  }

  /// 读取并采用 canonical product snapshot，把 [expectedId] 解析为选中项目。
  ///
  /// 后端 snapshot 不携带 Flutter selection；`_resolveSelection` 只会在 canonical
  /// Project 目录中存在 [expectedId] 时采用该项目。采用完成后从 [state.value] 读取
  /// 实际被选中的项目，并验证其 id、`sshAlias` 与请求别名一致、path 与后端
  /// 返回的 canonical 路径一致。
  ///
  /// 返回 false 表示 snapshot 读取/采用失败，或 adopted state 中不存在与请求身份
  /// 完全一致的选中项目。
  Future<bool> _adoptSelectedProject({
    required String expectedId,
    required String expectedAlias,
    required String expectedPath,
  }) async {
    final current = state.value;
    final StudioState incoming;
    try {
      final snapshot = await _api.readStudioState();
      incoming = _resolveSelection(
        snapshot,
        previous: current,
        intent: _ProjectDefaultSelection(expectedId),
      );
    } on Object {
      return false;
    }
    if (!ref.mounted) return false;
    try {
      await _adoptProductState(incoming);
    } on Object {
      return false;
    }
    final adopted = state.value;
    final selectedProject = adopted?.projects
        .where((project) => project.id == adopted.selectedProjectId)
        .firstOrNull;
    return selectedProject != null &&
        selectedProject.id == expectedId &&
        selectedProject.sshAlias == expectedAlias &&
        selectedProject.path == expectedPath;
  }

  Future<void> selectProject(String projectId) async {
    final current = state.value;
    if (current == null ||
        current.selectedProjectId == projectId ||
        current.recoveryIssue(
              scope: RecoveryIssueScope.project,
              projectId: projectId,
            ) !=
            null) {
      return;
    }
    await _api.activateProject(projectId);
    await _reloadProductState(selection: _ProjectDefaultSelection(projectId));
  }

  Future<void> beginNewThread() async {
    final current = state.value;
    final projectId = current?.selectedProjectId;
    if (current == null ||
        projectId == null ||
        current.recoveryIssue(
              scope: RecoveryIssueScope.project,
              projectId: projectId,
            ) !=
            null) {
      return;
    }
    state = AsyncData(
      current.copyWith(
        selectedThreadId: null,
        newThreadComposerByProject: {
          ...current.newThreadComposerByProject,
          projectId: const ComposerThreadState.idle(),
        },
      ),
    );
    await _subscribeThread(null);
  }

  void setNewThreadMode(ThreadModeId mode) {
    final current = state.value;
    final projectId = current?.selectedProjectId;
    if (current == null ||
        projectId == null ||
        current.selectedThreadId != null ||
        current.newThreadMode == mode) {
      return;
    }
    state = AsyncData(
      current.copyWith(
        newThreadModeByProject: {
          ...current.newThreadModeByProject,
          projectId: mode,
        },
      ),
    );
  }

  /// Saves the root session workspace-mode choice as the selected Project's input
  /// draft. The choice is not a second source of truth: it is only forwarded to
  /// `startNewThread` and never rewrites an existing Thread's canonical mode.
  void setNewThreadWorkspaceMode(ThreadWorkspaceMode mode) {
    final current = state.value;
    final projectId = current?.selectedProjectId;
    if (current == null ||
        projectId == null ||
        current.selectedThreadId != null ||
        current.newThreadWorkspaceMode == mode) {
      return;
    }
    state = AsyncData(
      current.copyWith(
        newThreadWorkspaceModeByProject: {
          ...current.newThreadWorkspaceModeByProject,
          projectId: mode,
        },
      ),
    );
  }

  /// Adds explicitly queried cold directory identities without replacing live facts
  /// or applying a project-scoped cursor to the global directory window.
  void includeDirectoryThreads(List<StudioThread> threads) {
    final current = state.value;
    if (current == null) return;
    final known = current.threads.map((thread) => thread.id).toSet();
    final missing = threads
        .where((thread) => !thread.archived && known.add(thread.id))
        .toList();
    if (missing.isEmpty) return;
    final directory = [...current.threadDirectory.threads, ...missing]
      ..sort((a, b) {
        final date = b.updatedAt.compareTo(a.updatedAt);
        return date != 0 ? date : b.id.compareTo(a.id);
      });
    state = AsyncData(
      current.copyWith(
        threadDirectory: current.threadDirectory.copyWith(threads: directory),
      ),
    );
  }

  Future<void> restoreThread(String threadId) async {
    final thread = await _api.restoreThread(threadId);
    await _reloadProductState(
      selection: _ExactThreadSelection(
        projectId: thread.projectId,
        threadId: thread.id,
      ),
    );
    // 恢复已归档会话是显式用户动作：选中后直接打开它。
    await openThread(thread.id);
  }

  Future<void> archiveThread(String threadId) async {
    final current = state.value;
    final thread = current?.threads
        .where((candidate) => candidate.id == threadId && candidate.isRoot)
        .firstOrNull;
    if (current == null ||
        !_isInitialized(current) ||
        thread == null ||
        current.recoveryIssue(
              scope: RecoveryIssueScope.thread,
              threadId: threadId,
            ) !=
            null) {
      return;
    }
    if (!_archivingThreadIds.add(threadId)) return;
    try {
      final result = await _api.archiveThread(threadId);
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest == null) return;
      final previousThreadId = latest.selectedThreadId;
      var next = _applyArchiveResult(latest, result);
      // 归档移除了这些会话：它们的 epoch/首窗 generation/在途标记一并释放。
      for (final removedId in result.removedThreadIds) {
        _releaseThreadSession(removedId);
      }
      if (previousThreadId != next.selectedThreadId) {
        // 归档后自动落到的新根会话同样在这里打开：订阅与阅读面保持一致。
        next = _markThreadOpened(next, next.selectedThreadId);
      }
      state = AsyncData(next);
      if (previousThreadId != next.selectedThreadId) {
        await _subscribeThread(next.selectedThreadId);
      }
    } finally {
      _archivingThreadIds.remove(threadId);
    }
  }

  Future<StudioThread?> renameThread(String threadId, String title) async {
    final current = state.value;
    final thread = current?.threads
        .where((candidate) => candidate.id == threadId && candidate.isRoot)
        .firstOrNull;
    if (current == null ||
        !_isInitialized(current) ||
        thread == null ||
        current.recoveryIssue(
              scope: RecoveryIssueScope.thread,
              threadId: threadId,
            ) !=
            null ||
        !_renamingThreadIds.add(threadId)) {
      return null;
    }
    try {
      final renamed = await _api.renameThread(threadId, title);
      if (!ref.mounted) return renamed;
      final latest = state.value;
      if (latest == null) return renamed;
      state = AsyncData(
        applyThreadDirectoryDelta(
          latest,
          upserted: [renamed],
          removed: const [],
        ),
      );
      return renamed;
    } finally {
      _renamingThreadIds.remove(threadId);
    }
  }

  Future<void> archiveProject(String projectId) async {
    final current = state.value;
    if (current == null ||
        !_isInitialized(current) ||
        (current.isBusy && current.selectedProjectId == projectId)) {
      return;
    }
    await _api.archiveProject(projectId);
    await _reloadProductState(
      selection: current.selectedProjectId == projectId
          ? const _ProjectDefaultSelection(null)
          : const _PreserveSelection(),
    );
  }

  Future<void> selectThread(String threadId) => _selectThread(threadId);

  Future<void> selectAgentThread(String threadId) async {
    final current = state.value;
    if (current == null || current.selectedThreadId == threadId) return;
    final target = current.threads
        .where((thread) => thread.id == threadId)
        .firstOrNull;
    if (target == null ||
        current.recoveryIssue(
              scope: RecoveryIssueScope.thread,
              threadId: threadId,
            ) !=
            null) {
      return;
    }
    final root = current.selectedRootThread;
    if (root != null && target.effectiveRootThreadId != root.id) return;
    await _selectThread(threadId);
  }

  Future<void> _selectThread(String threadId) async {
    final current = state.value;
    if (current == null ||
        !current.threads.any((thread) => thread.id == threadId) ||
        current.recoveryIssue(
              scope: RecoveryIssueScope.thread,
              threadId: threadId,
            ) !=
            null) {
      return;
    }
    if (current.selectedThreadId == threadId) {
      // 已选中但可能尚未打开：显式点击同一会话即打开它，而不是永远停在未打开状态。
      await openThread(threadId);
      return;
    }
    final previousThreadId = current.selectedThreadId;
    // 切换即释放上一个会话的历史载荷与全部会话级 map：整体内存随当前/近期窗口有界，
    // 回到该会话时由新的订阅与首窗读取重建。
    final base = previousThreadId == null
        ? current
        : releaseThreadHistoryPayload(current, previousThreadId);
    if (previousThreadId != null) {
      _releaseThreadSession(previousThreadId);
    }
    state = AsyncData(
      _withWorkspaceUi(
        base.copyWith(
          selectedThreadId: threadId,
          selectedProjectId: current.threads
              .firstWhere((thread) => thread.id == threadId)
              .projectId,
          // 显式点击会话即打开它：标记为已打开，随后建立订阅。
          openedThreadIds: {...base.openedThreadIds, threadId},
        ),
        threadId,
        (ui) => ui.copyWith(syncState: AgentWorkspaceSyncState.loading),
      ),
    );
    await _subscribeThread(threadId);
  }

  Future<void> retryThreadLoad(String threadId) async {
    final current = state.value;
    if (current == null || current.selectedThreadId != threadId) return;
    state = AsyncData(
      _withWorkspaceUi(
        current,
        threadId,
        (ui) => ui.copyWith(syncState: AgentWorkspaceSyncState.loading),
      ),
    );
    await _subscribeThread(threadId);
  }

  /// 打开一个已选中的会话（GUI 首帧之后或用户显式操作）。
  ///
  /// 打开流程：读取一次当前状态、建立该会话的事件接收端，并在首个权威帧之后读取首个
  /// 历史窗口。打开不等于恢复执行——不会重发模型请求、重跑工具或续跑未完成工作流；
  /// 已打开或正在打开时为空操作。
  Future<void> openThread(String threadId) async {
    final current = state.value;
    if (current == null || current.selectedThreadId != threadId) return;
    if (current.openedThreadIds.contains(threadId) ||
        _workspaceUi(current, threadId).syncState ==
            AgentWorkspaceSyncState.loading) {
      return;
    }
    state = AsyncData(
      _withWorkspaceUi(
        current.copyWith(
          openedThreadIds: {...current.openedThreadIds, threadId},
        ),
        threadId,
        (ui) => ui.copyWith(syncState: AgentWorkspaceSyncState.loading),
      ),
    );
    await _subscribeThread(threadId);
  }

  /// 打开当前选中的会话；没有选中会话时不做任何事。
  ///
  /// 供 GUI 首帧、未打开占位视图与测试使用，语义与 [openThread] 完全一致。
  Future<void> openSelectedThread() async {
    final threadId = state.value?.selectedThreadId;
    if (threadId == null) return;
    await openThread(threadId);
  }

  /// 用户交互（输入、提交、滚动、跳转、切换模式）触发的懒打开。
  ///
  /// GUI 首帧之后也会打开当时选中的会话；在此之前发生交互时沿同一路径建立订阅，
  /// 首个权威帧再驱动历史首窗。已打开或未选中该会话时为空操作。
  Future<void> _ensureThreadOpen(String threadId) async {
    final current = state.value;
    if (current == null ||
        current.selectedThreadId != threadId ||
        current.openedThreadIds.contains(threadId)) {
      return;
    }
    await openThread(threadId);
  }

  Future<void> _subscribeThread(String? threadId) async {
    if (_chatWindowThreadId != threadId) _closeChatWindow();
    final generation = _threadCoordinator.switchThread(threadId);
    if (!ref.mounted || threadId == null) return;
    // 重订阅开启新的广播生命周期：旧 epoch 立即失效。
    _streamEpochByThread.remove(threadId);
    final current = state.value;
    if (current == null || current.selectedThreadId != threadId) return;
    state = AsyncData(
      _withWorkspaceUi(
        current,
        threadId,
        (ui) => ui.copyWith(subscriptionGeneration: generation),
      ),
    );
    // 首个历史窗口的读取由“首个权威帧”驱动（见 [_handleThreadFrame] 的 snapshot 分支）：
    // Dart 的 `.listen()` 返回只代表已发起订阅，原生接收端注册与固定持久化屏障必须由
    // 首个 snapshot 帧证明完成后，SQL 才能被当作权威窗口读取。
  }

  /// 释放一个会话级的临时 map：切换、归档/关闭或 controller 销毁时调用，避免
  /// epoch / 首窗 generation / 在途请求标记随访问过的会话无限增长。
  void _releaseThreadSession(String threadId) {
    if (_chatWindowThreadId == threadId) _closeChatWindow();
    _windowLoadGeneration.remove(threadId);
    _streamEpochByThread.remove(threadId);
    _historyRequests.remove(threadId);
    _terminalRefreshTimers.remove(threadId)?.cancel();
    _pendingThreadDeltas.removeWhere((entry) => entry.$2 == threadId);
  }

  void _closeChatWindow() {
    _chatWindowOperation++;
    _openingChatWindow = null;
    _chatWindowThreadId = null;
    _chatFocusedItemId = null;
    final window = _chatWindow;
    _chatWindow = null;
    if (window != null) unawaited(window.close());
  }

  Future<void> _openChatWindow(String threadId) {
    final reader = _api;
    if (reader is! ChatWindowReader) {
      return Future<void>.value();
    }
    if (_chatWindowThreadId == threadId && _chatWindow != null) {
      return Future<void>.value();
    }
    final opening = _openingChatWindow;
    if (opening != null && _chatWindowThreadId == threadId) return opening;
    final started = _startChatWindow(reader as ChatWindowReader, threadId);
    _openingChatWindow = started;
    return started.whenComplete(() {
      if (identical(_openingChatWindow, started)) _openingChatWindow = null;
    });
  }

  Future<void> _startChatWindow(
    ChatWindowReader reader,
    String threadId,
  ) async {
    _chatWindowThreadId = threadId;
    final operation = ++_chatWindowOperation;
    StudioChatWindow? window;
    try {
      window = await reader.openChatWindow(threadId);
      var initial = await window.initial();
      final anchor = state.value?.workspaceUiByThread[threadId]?.history.anchor;
      if (anchor != null && !anchor.followingBottom) {
        initial = await window.focus(anchor.itemId);
      }
      if (!_acceptChatWindow(threadId, operation)) return;
      _chatWindow = window;
      _adoptChatWindow(threadId, initial);
      unawaited(_pullChatWindow(threadId, window));
      window = null;
    } catch (error) {
      if (_acceptChatWindow(threadId, operation)) {
        _chatWindowThreadId = null;
        final current = state.value;
        if (current != null) {
          state = AsyncData(
            _withWorkspaceUi(
              current,
              threadId,
              (ui) => ui.copyWith(
                history: ui.history.copyWith(errorMessage: error.toString()),
              ),
            ),
          );
        }
      }
    } finally {
      if (window != null) await window.close();
    }
  }

  bool _acceptChatWindow(String threadId, int operation) =>
      ref.mounted &&
      _chatWindowThreadId == threadId &&
      _chatWindowOperation == operation &&
      state.value?.selectedThreadId == threadId;

  void _adoptChatWindow(String threadId, StudioChatSnapshot snapshot) {
    final current = state.value;
    if (current == null || current.selectedThreadId != threadId) return;
    _chatFocusedItemId = snapshot.focusedItemId;
    state = AsyncData(applyChatWindowSnapshot(current, threadId, snapshot));
  }

  Future<void> _pullChatWindow(String threadId, StudioChatWindow window) async {
    while (_chatWindow == window && ref.mounted) {
      final operation = _chatWindowOperation;
      try {
        final snapshot = await window.next();
        if (snapshot == null) {
          if (_chatWindow == window && _acceptChatWindow(threadId, operation)) {
            _closeChatWindow();
            final current = state.value;
            if (current != null) {
              state = AsyncData(
                _withWorkspaceUi(
                  current,
                  threadId,
                  (ui) => ui.copyWith(
                    history: ui.history.copyWith(
                      errorMessage: 'Chat window connection closed',
                    ),
                  ),
                ),
              );
            }
          }
          return;
        }
        if (_chatWindow == window && _acceptChatWindow(threadId, operation)) {
          _adoptChatWindow(threadId, snapshot);
        }
      } catch (error) {
        if (_chatWindow == window) {
          final current = state.value;
          if (current != null && current.selectedThreadId == threadId) {
            state = AsyncData(
              _withWorkspaceUi(
                current,
                threadId,
                (ui) => ui.copyWith(
                  history: ui.history.copyWith(errorMessage: error.toString()),
                ),
              ),
            );
          }
          _closeChatWindow();
        }
        return;
      }
    }
  }

  /// 把某个会话标记回“未打开”；仅用于非显式选择变化，不触发订阅或释放。
  StudioState _markThreadUnopened(StudioState current, String? threadId) {
    if (threadId == null || !current.openedThreadIds.contains(threadId)) {
      return current;
    }
    return current.copyWith(
      openedThreadIds: {...current.openedThreadIds}..remove(threadId),
    );
  }

  /// 把一个会话标记为“已打开”（调用方随后已建立或即将建立订阅）。
  StudioState _markThreadOpened(StudioState current, String? threadId) {
    if (threadId == null || current.openedThreadIds.contains(threadId)) {
      return current;
    }
    return current.copyWith(
      openedThreadIds: {...current.openedThreadIds, threadId},
    );
  }

  Future<void> loadOlderHistory(String threadId) async {
    await _ensureThreadOpen(threadId);
    await _loadTimelinePage(threadId, TimelineDirection.older);
  }

  Future<void> loadNewerHistory(String threadId) async {
    await _ensureThreadOpen(threadId);
    await _loadTimelinePage(threadId, TimelineDirection.newer);
  }

  /// 读取一条完整条目正文：原生 bridge 与 demo 直接按 identity 读取完整 payload；
  /// 其它实现退化为围绕该身份的一页，仍由同一身份/revision 规则决定是否采纳。
  Future<TimelinePage> _readTimelineItemBody(String threadId, String itemId) {
    final api = _api;
    // 声明式模式绑定：只有实现该可选能力的 API 才走按 identity 的完整正文回源，
    // 其它实现（测试替身等）退化为围绕该身份的一页，不编造完整载荷。
    if (api case final TimelineItemBodyReader reader) {
      return reader.readTimelineItem(threadId, itemId);
    }
    return api.listTimelineItems(
      threadId,
      kind: TimelineQueryKind.around,
      itemId: itemId,
      limit: 1,
    );
  }

  /// 按 item identity 回源一条被页面预览预算截断的完整正文。
  ///
  /// 加载期间在窗口状态里显式标记该条目；回源结果与窗口共享 database identity 与
  /// watermark，因此只按同一身份合并，窗口过期时改为重读权威窗口而不是拼接旧载荷。
  Future<void> loadItemBody(String threadId, String itemId) async {
    await _ensureThreadOpen(threadId);
    final current = state.value;
    if (current == null ||
        current.selectedThreadId != threadId ||
        !current.workspacesByThread.containsKey(threadId) ||
        !_workspaceUi(
          current,
          threadId,
        ).history.previewedItemIds.contains(itemId)) {
      return;
    }
    if (_workspaceUi(
      current,
      threadId,
    ).history.loadingItemIds.contains(itemId)) {
      return;
    }
    state = AsyncData(startItemBodyLoad(current, threadId, itemId));
    final chatWindow = _chatWindowThreadId == threadId ? _chatWindow : null;
    if (chatWindow != null) {
      try {
        final item = await chatWindow.readItem(itemId);
        if (!ref.mounted || _chatWindow != chatWindow) return;
        final latest = state.value;
        if (latest != null && latest.selectedThreadId == threadId) {
          state = AsyncData(
            applyChatWindowItemBody(latest, threadId, itemId, item),
          );
        }
      } catch (error) {
        if (ref.mounted && _chatWindow == chatWindow && state.value != null) {
          state = AsyncData(
            failItemBodyLoad(state.value!, threadId, itemId, error.toString()),
          );
        }
      }
      return;
    }
    try {
      final page = await _readTimelineItemBody(threadId, itemId);
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest == null ||
          latest.selectedThreadId != threadId ||
          !latest.workspacesByThread.containsKey(threadId)) {
        return;
      }
      if (timelinePageIsStale(
        _workspaceUi(latest, threadId).history,
        page,
        // 按 identity 回源只能并入同一数据库实体：身份不同或水位回退都拒绝。
        replaceWindow: false,
      )) {
        // 回源页来自已被替换的数据库实体：不并入，改读权威窗口。
        state = AsyncData(
          _withWorkspaceUi(latest, threadId, (ui) {
            final loadingItemIds = {...ui.history.loadingItemIds}
              ..remove(itemId);
            return ui.copyWith(
              history: ui.history.copyWith(loadingItemIds: loadingItemIds),
            );
          }),
        );
        unawaited(
          _reloadTimelineWindow(
            threadId,
            _threadCoordinator.generation,
            force: true,
          ),
        );
        return;
      }
      state = AsyncData(applyItemBodyPage(latest, threadId, itemId, page));
    } catch (error) {
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest == null) return;
      state = AsyncData(
        failItemBodyLoad(latest, threadId, itemId, error.toString()),
      );
    }
  }

  void updateTimelineAnchor(String threadId, TimelineAnchor anchor) {
    final current = state.value;
    if (current == null) return;
    // 先落地阅读锚点，再（未打开时）激活会话：激活读取的是含锚点的最新状态，
    // 因此打开写下的 opened/subscriptionGeneration 不会被本方法用旧快照覆盖。
    state = AsyncData(
      _withWorkspaceUi(
        current,
        threadId,
        (ui) => ui.copyWith(
          history: ui.history.copyWith(
            anchor: anchor,
            detached: !anchor.followingBottom,
          ),
        ),
      ),
    );
    // 滚动/定位是显式交互：未打开的会话在此激活。
    unawaited(_ensureThreadOpen(threadId));
    if (!anchor.followingBottom &&
        _chatWindowThreadId == threadId &&
        _chatWindow != null &&
        _chatFocusedItemId == null) {
      unawaited(_focusChatWindow(threadId, anchor.itemId));
    }
  }

  Future<void> jumpToLatest(String threadId) async {
    if (state.value == null) return;
    await _ensureThreadOpen(threadId);
    final current = state.value;
    if (current == null || current.selectedThreadId != threadId) return;
    state = AsyncData(jumpTimelineToLatest(current, threadId));
    if (_chatWindowThreadId == threadId && _chatWindow != null) {
      await _focusChatWindow(threadId, null);
      return;
    }
    await _reloadTimelineWindow(
      threadId,
      _threadCoordinator.generation,
      force: true,
    );
  }

  /// 订阅建立后的权威窗口读取：首窗与重连直接用数据库最新窗口替换阅读范围，
  /// 已离开底部（存在非跟随锚点）时改为围绕锚点读取，保持用户位置。
  ///
  /// 只有成功读取后才把该 generation 记为已读取，因此在更早的读取被跳过
  /// （例如 owner 尚未激活）时，同一 generation 的首帧仍会补做一次。
  Future<void> _reloadTimelineWindow(
    String threadId,
    int generation, {
    bool force = false,
  }) async {
    final current = state.value;
    if (current == null ||
        generation != _threadCoordinator.generation ||
        current.selectedThreadId != threadId ||
        _workspaceUi(current, threadId).subscriptionGeneration != generation ||
        (!force && _windowLoadGeneration[threadId] == generation)) {
      return;
    }
    if (_api is ChatWindowReader) {
      await _openChatWindow(threadId);
      return;
    }
    final anchor = _workspaceUi(current, threadId).history.anchor;
    final loaded = anchor != null && !anchor.followingBottom
        ? await _loadTimelinePage(
            threadId,
            TimelineDirection.older,
            aroundItemId: anchor.itemId,
          )
        : await _loadTimelinePage(
            threadId,
            TimelineDirection.newer,
            resetWindow: true,
          );
    if (loaded) {
      _windowLoadGeneration[threadId] = generation;
    }
  }

  /// 读取一页历史窗口；返回是否真正发起了请求。
  Future<bool> _loadTimelinePage(
    String threadId,
    TimelineDirection direction, {
    String? aroundItemId,
    bool resetWindow = false,
  }) async {
    if (_chatWindowThreadId == threadId && _chatWindow != null) {
      if (aroundItemId != null) {
        await _focusChatWindow(threadId, aroundItemId);
      } else if (resetWindow) {
        await _focusChatWindow(threadId, null);
      } else {
        await _loadChatWindow(threadId, direction);
      }
      return true;
    }
    if (_api is ChatWindowReader) {
      await _openChatWindow(threadId);
      if (_chatWindowThreadId == threadId && _chatWindow != null) {
        return _loadTimelinePage(
          threadId,
          direction,
          aroundItemId: aroundItemId,
          resetWindow: resetWindow,
        );
      }
      return false;
    }
    final current = state.value;
    if (current == null) return false;
    final workspace = current.workspacesByThread[threadId];
    if (workspace == null) return false;
    final history = _workspaceUi(current, threadId).history;
    final older = direction == TimelineDirection.older;
    final anchor =
        aroundItemId ??
        (older
            ? history.olderCursor ?? workspace.historyItems.firstOrNull?.id
            : history.newerCursor ?? workspace.historyItems.lastOrNull?.id);
    if (history.isLoading || _historyRequests.contains(threadId)) return false;
    if (!resetWindow &&
        (anchor == null ||
            (aroundItemId == null &&
                !(older ? history.hasOlder : history.hasNewer)))) {
      return false;
    }
    final epoch = history.epoch;
    final replaceWindow = resetWindow || aroundItemId != null;
    var stale = false;
    _historyRequests.add(threadId);
    state = AsyncData(
      _withWorkspaceUi(
        current,
        threadId,
        (ui) => ui.copyWith(
          history: ui.history.copyWith(
            isLoading: true,
            direction: direction,
            errorMessage: older ? null : ui.history.errorMessage,
            newerError: older ? ui.history.newerError : null,
          ),
        ),
      ),
    );
    try {
      final page = await _api.listTimelineItems(
        threadId,
        kind: resetWindow
            ? TimelineQueryKind.latest
            : aroundItemId != null
            ? TimelineQueryKind.around
            : older
            ? TimelineQueryKind.before
            : TimelineQueryKind.after,
        itemId: resetWindow ? null : anchor,
      );
      if (!ref.mounted) return true;
      final latest = state.value;
      if (latest == null ||
          !latest.workspacesByThread.containsKey(threadId) ||
          _workspaceUi(latest, threadId).history.epoch != epoch) {
        return true;
      }
      // 页面必须与当前窗口属于同一数据库且水位不早于已采纳水位；否则只读不并入。
      if (timelinePageIsStale(
        _workspaceUi(latest, threadId).history,
        page,
        replaceWindow: replaceWindow,
      )) {
        stale = true;
        state = AsyncData(
          _withWorkspaceUi(
            latest,
            threadId,
            (ui) => ui.copyWith(history: ui.history.copyWith(isLoading: false)),
          ),
        );
      } else {
        state = AsyncData(
          applyTimelinePage(
            latest,
            threadId,
            page,
            direction,
            replaceWindow: replaceWindow,
            followBottom: resetWindow,
          ),
        );
      }
    } catch (error) {
      if (!ref.mounted) return true;
      final latest = state.value;
      if (latest == null ||
          !latest.workspacesByThread.containsKey(threadId) ||
          _workspaceUi(latest, threadId).history.epoch != epoch) {
        return true;
      }
      state = AsyncData(
        _withWorkspaceUi(
          latest,
          threadId,
          (ui) => ui.copyWith(
            history: ui.history.copyWith(
              isLoading: false,
              errorMessage: older ? error.toString() : ui.history.errorMessage,
              newerError: older ? ui.history.newerError : error.toString(),
            ),
          ),
        ),
      );
    } finally {
      _historyRequests.remove(threadId);
      if (ref.mounted) {
        final latest = state.value;
        if (latest != null &&
            latest.workspacesByThread.containsKey(threadId) &&
            _workspaceUi(latest, threadId).history.epoch != epoch) {
          state = AsyncData(
            _withWorkspaceUi(
              latest,
              threadId,
              (ui) =>
                  ui.copyWith(history: ui.history.copyWith(isLoading: false)),
            ),
          );
        }
      }
    }
    if (stale && ref.mounted) {
      // 过期页不并入；用最新窗口替换当前窗口，采纳新的数据库身份与水位。
      unawaited(
        _loadTimelinePage(threadId, TimelineDirection.newer, resetWindow: true),
      );
      return false;
    }
    return true;
  }

  Future<void> _focusChatWindow(String threadId, String? itemId) async {
    final window = _chatWindow;
    if (window == null || _chatWindowThreadId != threadId) return;
    if (_chatFocusedItemId == itemId) return;
    final operation = ++_chatWindowOperation;
    try {
      final snapshot = await window.focus(itemId);
      if (_chatWindow == window && _acceptChatWindow(threadId, operation)) {
        _adoptChatWindow(threadId, snapshot);
      }
    } catch (error) {
      if (_chatWindow == window && _acceptChatWindow(threadId, operation)) {
        final current = state.value;
        if (current != null) {
          state = AsyncData(
            _withWorkspaceUi(
              current,
              threadId,
              (ui) => ui.copyWith(
                history: ui.history.copyWith(errorMessage: error.toString()),
              ),
            ),
          );
        }
      }
    }
  }

  Future<void> _loadChatWindow(
    String threadId,
    TimelineDirection direction,
  ) async {
    final window = _chatWindow;
    if (window == null || _chatWindowThreadId != threadId) return;
    if (!_historyRequests.add(threadId)) return;
    final operation = ++_chatWindowOperation;
    final current = state.value;
    if (current != null) {
      state = AsyncData(
        _withWorkspaceUi(
          current,
          threadId,
          (ui) => ui.copyWith(
            history: ui.history.copyWith(isLoading: true, direction: direction),
          ),
        ),
      );
    }
    try {
      final snapshot = await window.load(direction);
      if (_chatWindow == window && _acceptChatWindow(threadId, operation)) {
        _adoptChatWindow(threadId, snapshot);
      }
    } catch (error) {
      if (_chatWindow == window && _acceptChatWindow(threadId, operation)) {
        final latest = state.value;
        if (latest != null) {
          state = AsyncData(
            _withWorkspaceUi(
              latest,
              threadId,
              (ui) => ui.copyWith(
                history: ui.history.copyWith(
                  isLoading: false,
                  errorMessage: direction == TimelineDirection.older
                      ? error.toString()
                      : ui.history.errorMessage,
                  newerError: direction == TimelineDirection.newer
                      ? error.toString()
                      : ui.history.newerError,
                ),
              ),
            ),
          );
        }
      }
    } finally {
      _historyRequests.remove(threadId);
    }
  }

  void _scheduleTerminalRefresh(String threadId) {
    if (_terminalRefreshTimers.containsKey(threadId)) return;
    _terminalRefreshTimers[threadId] = Timer(
      const Duration(milliseconds: 48),
      () async {
        _terminalRefreshTimers.remove(threadId);
        final current = state.value;
        if (!ref.mounted ||
            current == null ||
            current.selectedThreadId != threadId ||
            current.workspacesByThread[threadId]?.liveItems.values.any(
                  (item) => item.isTerminal,
                ) !=
                true) {
          return;
        }
        if (current.selectedWorkspaceUi.history.isLoading ||
            _historyRequests.contains(threadId)) {
          _scheduleTerminalRefresh(threadId);
          return;
        }
        if (current.selectedWorkspaceUi.history.detached) {
          await _confirmDetachedTerminalItems(threadId);
          return;
        }
        await _loadTimelinePage(
          threadId,
          TimelineDirection.newer,
          resetWindow: true,
        );
      },
    );
  }

  Future<void> _confirmDetachedTerminalItems(String threadId) async {
    final current = state.value;
    if (current == null) return;
    final history = current.selectedWorkspaceUi.history;
    final pending = current.workspacesByThread[threadId]?.liveItems.values
        .where((item) => item.isTerminal)
        .toList();
    if (pending == null || pending.isEmpty) return;
    final ids = {for (final item in pending) item.id};
    final oldestOrdinal = pending
        .map((item) => item.ordinal)
        .reduce((a, b) => a < b ? a : b);
    try {
      var page = await _api.listTimelineItems(
        threadId,
        kind: TimelineQueryKind.latest,
      );
      final databaseId = page.databaseId;
      final confirmed = <(ThreadItemView, int)>[];
      String? previousCursor;
      while (page.threadId == threadId && page.databaseId == databaseId) {
        final omitted = {
          for (final preview in page.previews)
            preview.itemId: preview.omittedBytes,
        };
        confirmed.addAll(
          page.items
              .where((item) => ids.contains(item.id))
              .map((item) => (item, omitted[item.id] ?? 0)),
        );
        if (page.items.isEmpty || page.items.first.ordinal <= oldestOrdinal) {
          break;
        }
        final cursor = page.olderCursor;
        if (cursor == null || cursor == previousCursor) break;
        previousCursor = cursor;
        page = await _api.listTimelineItems(
          threadId,
          kind: TimelineQueryKind.before,
          itemId: cursor,
        );
      }
      final latest = state.value;
      if (!ref.mounted ||
          latest == null ||
          latest.selectedThreadId != threadId ||
          !latest.selectedWorkspaceUi.history.detached ||
          latest.selectedWorkspaceUi.history.epoch != history.epoch ||
          (history.databaseId.isNotEmpty && history.databaseId != databaseId)) {
        return;
      }
      state = AsyncData(
        confirmDetachedTimelineItems(latest, threadId, confirmed),
      );
    } catch (error) {
      // A failed confirmation cannot evict the in-memory body. The next page
      // load or terminal notification can retry using the same SQL identity.
      debugPrint('timeline terminal confirmation failed: $error');
    }
  }

  /// 侧栏触底加载下一页会话目录；内存未命中时由 bridge 从数据库分页取回。
  /// 测试入口：显式触发一次 product reload（等价 StalePayload 路径）。
  @visibleForTesting
  Future<void> debugReloadForTest() => _reloadProductState();

  Future<void> loadMoreThreads() async {
    final current = state.value;
    if (current == null) return;
    final directory = current.threadDirectory;
    if (directory.isLoading || !directory.hasMore) return;
    state = AsyncData(setThreadDirectoryLoading(current, true));
    try {
      final page = await _api.listThreadsPage(cursor: directory.nextCursor);
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest == null) return;
      state = AsyncData(appendThreadDirectoryPage(latest, page));
    } catch (error) {
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest == null) return;
      state = AsyncData(setThreadDirectoryLoading(latest, false));
      ref.read(directoryLoadErrorProvider.notifier).set(error.toString());
    }
  }

  void updateComposer(String threadId, String value) {
    final current = state.value;
    if (current == null || current.selectedThreadId != threadId) return;
    // 先落地草稿，再（未打开时）激活会话：激活基于含草稿的最新状态，避免本方法随后
    // 用打开前的旧快照覆盖 opened/subscriptionGeneration，导致首窗读取与实时帧被丢弃。
    state = AsyncData(
      _withWorkspaceUi(
        current,
        threadId,
        (ui) => ui.copyWith(composer: ui.composer.updateDraft(value)),
      ),
    );
    // 在输入区输入是显式交互：未打开的会话在此激活。
    unawaited(_ensureThreadOpen(threadId));
  }

  void updateNewThreadComposer(String value) {
    final current = state.value;
    final projectId = current?.selectedProjectId;
    if (current == null ||
        projectId == null ||
        current.selectedThreadId != null) {
      return;
    }
    final composer =
        current.newThreadComposerByProject[projectId] ??
        const ComposerThreadState.idle();
    state = AsyncData(
      current.copyWith(
        newThreadComposerByProject: {
          ...current.newThreadComposerByProject,
          projectId: composer.updateDraft(value),
        },
      ),
    );
  }

  Future<void> addLocalAttachments(
    List<String> paths, {
    String? threadId,
  }) async {
    if (paths.isEmpty) return;
    await _admitAttachments([
      for (final path in paths) AttachmentDraftSource.localFile(path),
    ], threadId: threadId);
  }

  Future<void> addClipboardImage(Uint8List pngBytes, {String? threadId}) async {
    if (pngBytes.isEmpty) {
      reportComposerFailure(
        StateError('Clipboard image is empty.'),
        threadId: threadId,
      );
      return;
    }
    StagedClipboardImage? staged;
    try {
      staged = await ref.read(clipboardImageStagerProvider).stage(pngBytes);
      await addLocalAttachments([staged.path], threadId: threadId);
    } catch (error) {
      reportComposerFailure(error, threadId: threadId);
    } finally {
      await staged?.dispose();
    }
  }

  void reportComposerFailure(Object error, {String? threadId}) {
    final current = state.value;
    if (current == null ||
        (threadId != null && current.selectedThreadId != threadId) ||
        (threadId == null && current.selectedThreadId != null)) {
      return;
    }
    state = AsyncData(
      threadId == null
          ? current.copyWith(
              newThreadComposerByProject: {
                ...current.newThreadComposerByProject,
                ?current.selectedProjectId: current.newThreadComposer
                    .reportFailure(error),
              },
            )
          : _withWorkspaceUi(
              current,
              threadId,
              (ui) => ui.copyWith(composer: ui.composer.reportFailure(error)),
            ),
    );
  }

  Future<void> addRemoteAttachment(String url, {String? threadId}) async {
    await _admitAttachments([
      AttachmentDraftSource.remoteUrl(url.trim()),
    ], threadId: threadId);
  }

  Future<void> _admitAttachments(
    List<AttachmentDraftSource> sources, {
    String? threadId,
  }) async {
    final current = state.value;
    final projectId = current?.selectedProjectId;
    if (current == null ||
        (threadId != null && current.selectedThreadId != threadId) ||
        (threadId == null &&
            (projectId == null || current.selectedThreadId != null))) {
      return;
    }
    final composer = threadId == null
        ? current.newThreadComposer
        : _workspaceUi(current, threadId).composer;
    if (composer.isSubmissionPending) return;
    List<AttachmentDraftView> admitted = const [];
    try {
      admitted = await _api.admitAttachmentDrafts(
        threadId == null
            ? AttachmentAdmissionContext.newThread(current.newThreadMode)
            : AttachmentAdmissionContext.existingThread(threadId),
        sources,
      );
      admitted = await Future.wait([
        for (final draft in admitted)
          if (draft.modality == AttachmentModalityView.image)
            _api
                .readAttachmentDraft(draft.id)
                .then((bytes) => draft.copyWith(previewBytes: bytes))
          else
            Future.value(draft),
      ]);
    } catch (error) {
      await Future.wait([
        for (final draft in admitted) _api.removeAttachmentDraft(draft.id),
      ]);
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest == null) return;
      state = AsyncData(
        threadId == null
            ? latest.copyWith(
                newThreadComposerByProject: {
                  ...latest.newThreadComposerByProject,
                  ?projectId: latest.newThreadComposer.reportFailure(error),
                },
              )
            : _withWorkspaceUi(
                latest,
                threadId,
                (ui) => ui.copyWith(composer: ui.composer.reportFailure(error)),
              ),
      );
      return;
    }
    if (!ref.mounted) return;
    final latest = state.value;
    if (latest == null) return;
    final active = threadId == null
        ? latest.newThreadComposer
        : _workspaceUi(latest, threadId).composer;
    if (active.isSubmissionPending) {
      await Future.wait([
        for (final draft in admitted) _api.removeAttachmentDraft(draft.id),
      ]);
      return;
    }
    final updated = active.updateAttachments([
      ...active.attachments,
      ...admitted,
    ]);
    state = AsyncData(
      threadId == null
          ? latest.copyWith(
              newThreadComposerByProject: {
                ...latest.newThreadComposerByProject,
                ?projectId: updated,
              },
            )
          : _withWorkspaceUi(
              latest,
              threadId,
              (ui) => ui.copyWith(composer: updated),
            ),
    );
  }

  Future<void> removeAttachmentDraft(String draftId, {String? threadId}) async {
    final current = state.value;
    if (current == null) return;
    final composer = threadId == null
        ? current.newThreadComposer
        : _workspaceUi(current, threadId).composer;
    if (composer.isSubmissionPending ||
        !composer.attachments.any((attachment) => attachment.id == draftId)) {
      return;
    }
    try {
      if (!await _api.removeAttachmentDraft(draftId)) {
        throw StateError('Attachment draft is unavailable.');
      }
    } catch (error) {
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest == null) return;
      state = AsyncData(
        threadId == null
            ? latest.copyWith(
                newThreadComposerByProject: {
                  ...latest.newThreadComposerByProject,
                  ?latest.selectedProjectId: latest.newThreadComposer
                      .reportFailure(error),
                },
              )
            : _withWorkspaceUi(
                latest,
                threadId,
                (ui) => ui.copyWith(composer: ui.composer.reportFailure(error)),
              ),
      );
      return;
    }
    if (!ref.mounted) return;
    final latest = state.value;
    if (latest == null) return;
    final active = threadId == null
        ? latest.newThreadComposer
        : _workspaceUi(latest, threadId).composer;
    final updated = active.updateAttachments([
      for (final attachment in active.attachments)
        if (attachment.id != draftId) attachment,
    ]);
    state = AsyncData(
      threadId == null
          ? latest.copyWith(
              newThreadComposerByProject: {
                ...latest.newThreadComposerByProject,
                ?latest.selectedProjectId: updated,
              },
            )
          : _withWorkspaceUi(
              latest,
              threadId,
              (ui) => ui.copyWith(composer: updated),
            ),
    );
  }

  Future<void> submitNewThreadComposer() async {
    final current = state.value;
    final projectId = current?.selectedProjectId;
    final composer =
        current?.newThreadComposer ?? const ComposerThreadState.idle();
    final prompt = composer.draft.trim();
    if (current == null ||
        !_isInitialized(current) ||
        projectId == null ||
        current.selectedThreadId != null ||
        current.recoveryIssue(
              scope: RecoveryIssueScope.project,
              projectId: projectId,
            ) !=
            null ||
        (prompt.isEmpty && composer.attachments.isEmpty) ||
        composer.isSubmissionPending) {
      return;
    }

    final submitting = composer.beginSubmission();
    final submissionRevision = submitting.submissionRevision;
    state = AsyncData(
      current.copyWith(
        newThreadComposerByProject: {
          ...current.newThreadComposerByProject,
          projectId: submitting,
        },
      ),
    );

    final StartNewThreadResult result;
    try {
      result = await _api.startNewThread(
        projectId,
        StudioPromptInput(
          inputId: submitting.inputId!,
          text: prompt,
          attachmentDraftIds: [
            for (final attachment in composer.attachments) attachment.id,
          ],
        ),
        current.newThreadMode,
        workspaceMode: current.newThreadWorkspaceMode.id,
      );
    } catch (error) {
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest == null) return;
      final active =
          latest.newThreadComposerByProject[projectId] ??
          const ComposerThreadState.idle();
      final failed = active.fail(error, submissionRevision: submissionRevision);
      state = AsyncData(
        latest.copyWith(
          newThreadComposerByProject: {
            ...latest.newThreadComposerByProject,
            projectId: failed,
          },
        ),
      );
      return;
    }
    if (!ref.mounted) return;
    final latest = state.value;
    if (latest == null) return;

    final active =
        latest.newThreadComposerByProject[projectId] ??
        const ComposerThreadState.idle();
    Object? validationError;
    if (result.thread.projectId != projectId) {
      validationError = StateError(
        'new Thread project ${result.thread.projectId} does not match $projectId',
      );
    } else if (result.receipt.threadId != result.thread.id) {
      validationError = StateError(
        'submit receipt thread ${result.receipt.threadId} does not match '
        '${result.thread.id}',
      );
    }
    if (validationError != null) {
      final failed = active.fail(
        validationError,
        submissionRevision: submissionRevision,
      );
      state = AsyncData(
        latest.copyWith(
          newThreadComposerByProject: {
            ...latest.newThreadComposerByProject,
            projectId: failed,
          },
        ),
      );
      return;
    }

    final accepted = submitting.accept(
      result.receipt,
      submissionRevision: submissionRevision,
    );
    final shouldSelect =
        latest.selectedProjectId == projectId &&
        latest.selectedThreadId == null &&
        active is SubmittingComposerThreadState &&
        active.submissionRevision == submissionRevision;
    var next = applyThreadDirectoryDelta(
      latest,
      upserted: [result.thread],
      removed: const [],
    );
    next = _withWorkspaceUi(
      next,
      result.thread.id,
      (ui) => ui.copyWith(
        composer: accepted,
        syncState: AgentWorkspaceSyncState.loading,
      ),
    );
    next = next.copyWith(
      selectedThreadId: shouldSelect
          ? result.thread.id
          : latest.selectedThreadId,
      // 新建会话是显式用户动作：创建后直接打开它。
      openedThreadIds: shouldSelect
          ? {...next.openedThreadIds, result.thread.id}
          : next.openedThreadIds,
      newThreadComposerByProject: {
        ...next.newThreadComposerByProject,
        projectId: shouldSelect ? const ComposerThreadState.idle() : active,
      },
    );
    state = AsyncData(next);
    if (shouldSelect) {
      await _subscribeThread(result.thread.id);
    }
  }

  Future<void> submitComposer(String threadId) async {
    // 提交是显式交互：未打开的会话先激活，随后才受理输入。
    await _ensureThreadOpen(threadId);
    final current = state.value;
    final composer = current == null
        ? const ComposerThreadState.idle()
        : _workspaceUi(current, threadId).composer;
    final prompt = composer.draft.trim();
    if (current == null ||
        !_isInitialized(current) ||
        current.selectedThreadId != threadId ||
        (prompt.isEmpty && composer.attachments.isEmpty) ||
        composer.isSubmissionPending) {
      return;
    }
    final submitting = composer.beginSubmission();
    final input = StudioPromptInput(
      inputId: submitting.inputId!,
      text: prompt,
      attachmentDraftIds: [
        for (final attachment in composer.attachments) attachment.id,
      ],
    );
    Future<SubmitPromptReceipt> submit() => _api.submitPrompt(threadId, input);
    await _submitThreadInput(current, threadId, submitting, submit);
  }

  Future<void> _submitThreadInput(
    StudioState current,
    String threadId,
    ComposerThreadState submitting,
    Future<SubmitPromptReceipt> Function() submit,
  ) async {
    final submissionRevision = submitting.submissionRevision;
    state = AsyncData(
      _withWorkspaceUi(
        current,
        threadId,
        (ui) => ui.copyWith(composer: submitting),
      ),
    );
    final SubmitPromptReceipt receipt;
    try {
      receipt = await submit();
    } catch (error) {
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest == null) return;
      final active = _workspaceUi(latest, threadId).composer;
      final failed = active.fail(error, submissionRevision: submissionRevision);
      state = AsyncData(
        _withWorkspaceUi(
          latest,
          threadId,
          (ui) => ui.copyWith(composer: failed),
        ),
      );
      return;
    }
    if (!ref.mounted) return;
    final latest = state.value;
    if (latest == null) return;
    final active = _workspaceUi(latest, threadId).composer;
    if (receipt.threadId != threadId) {
      final failed = active.fail(
        StateError(
          'submit receipt thread ${receipt.threadId} does not match $threadId',
        ),
        submissionRevision: submissionRevision,
      );
      state = AsyncData(
        _withWorkspaceUi(
          latest,
          threadId,
          (ui) => ui.copyWith(composer: failed),
        ),
      );
      return;
    }
    final accepted = active.accept(
      receipt,
      submissionRevision: submissionRevision,
    );
    final next = _withWorkspaceUi(
      latest,
      threadId,
      (ui) => ui.copyWith(composer: accepted),
    );
    state = AsyncData(next);
  }

  Future<void> stop(String threadId) async {
    final current = state.value;
    final turn = current?.workspacesByThread[threadId]?.activeTurn;
    if (current == null ||
        current.selectedThreadId != threadId ||
        turn == null ||
        !turn.state.isBusy) {
      return;
    }
    await _api.interruptTurn(threadId, turn.turnId);
  }

  Future<void> setPermissionMode(PermissionMode mode) async {
    await _saveConfigSettings(
      (revision) => _api.saveRuntimePermissionMode(revision, mode),
    );
  }

  Future<void> setThreadMode(ThreadModeId mode) async {
    final current = state.value;
    final thread = current?.selectedThread;
    if (current == null ||
        !_isInitialized(current) ||
        thread == null ||
        !thread.isRoot ||
        thread.mode == mode ||
        thread.status != ThreadStatusView.idle ||
        current.runtime.hasActiveWorkflow) {
      return;
    }
    // 切换会话模式是显式交互：未打开的会话先激活。
    await _ensureThreadOpen(thread.id);
    await _api.setThreadMode(threadId: thread.id, mode: mode);
    if (!ref.mounted) return;
    final latest = state.value;
    if (latest == null) return;
    await _reloadProductState(
      selection: _ExactThreadSelection(
        projectId: latest.selectedProjectId,
        threadId: thread.id,
      ),
    );
    if (latest.selectedThreadId == thread.id) {
      await _subscribeThread(thread.id);
    }
  }

  Future<void> setModelRole({
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
  }) async {
    final current = state.value;
    if (current == null) return;
    final target = current.providers
        .where((provider) => provider.id == providerId)
        .expand((provider) => provider.allModels)
        .where((candidate) => candidate.slug == model)
        .firstOrNull;
    final role = current.role(roleKey);
    if (role != null &&
        role.providerId == providerId &&
        role.model == model &&
        (effort == null || role.effort == effort)) {
      return;
    }
    final next = await _api.setModelRole(
      expectedSettingsRevision: current.settingsRevision,
      roleKey: roleKey,
      providerId: providerId,
      model: model,
      effort: effort ?? target?.reasoningEfforts.firstOrNull,
    );
    final latest = state.value;
    if (latest != null) state = AsyncData(applySettingsState(latest, next));
  }

  Future<void> setModeModelRoute({
    required ThreadModeId mode,
    required String providerId,
    required String model,
    String? effort,
  }) async {
    final current = state.value;
    if (current == null || current.newThreadMode != mode) return;
    final target = _findModel(current, providerId, model);
    if (target == null ||
        !_acceptsAttachments(current.newThreadComposer, target)) {
      return;
    }
    final route = current.modeModelRoutes
        .where((candidate) => candidate.modeId == mode)
        .firstOrNull;
    if (route != null &&
        route.providerId == providerId &&
        route.model == model &&
        (effort == null || route.effort == effort)) {
      return;
    }
    final next = await _api.setModeModelRoute(
      expectedSettingsRevision: current.settingsRevision,
      mode: mode,
      providerId: providerId,
      model: model,
      effort: effort ?? target.reasoningEfforts.firstOrNull,
    );
    final latest = state.value;
    if (latest != null) state = AsyncData(applySettingsState(latest, next));
  }

  Future<void> setThreadModelRoute({
    required String providerId,
    required String model,
    String? effort,
  }) async {
    final current = state.value;
    final thread = current?.selectedThread;
    final workspace = current?.selectedWorkspace;
    if (current == null ||
        thread == null ||
        workspace == null ||
        !thread.isRoot ||
        thread.status != ThreadStatusView.idle ||
        current.runtime.hasActiveWorkflow) {
      return;
    }
    final target = _findModel(current, providerId, model);
    if (target == null || !_acceptsAttachments(current.composer, target)) {
      return;
    }
    final expectedThreadRevision = workspace.revision;
    final response = await _api.setThreadModelRoute(
      threadId: thread.id,
      expectedThreadRevision: expectedThreadRevision,
      expectedSettingsRevision: current.settingsRevision,
      providerId: providerId,
      model: model,
      effort: effort ?? target.reasoningEfforts.firstOrNull,
    );
    final latest = state.value;
    if (latest == null) return;
    var next = applySettingsState(latest, response.settings);
    final latestWorkspace = next.workspacesByThread[thread.id];
    if (latestWorkspace != null) {
      next = next.copyWith(
        workspacesByThread: {
          ...next.workspacesByThread,
          thread.id: latestWorkspace.copyWith(
            revision: latestWorkspace.revision > expectedThreadRevision
                ? latestWorkspace.revision
                : expectedThreadRevision + 1,
            runtime: response.runtime,
          ),
        },
      );
    }
    if (response.warning case final warning?) {
      next = _withWorkspaceUi(
        next,
        thread.id,
        (ui) => ui.copyWith(
          composer: ui.composer.reportFailure(StateError(warning)),
        ),
      );
    }
    state = AsyncData(next);
  }

  ProviderModelView? _findModel(
    StudioState current,
    String providerId,
    String model,
  ) {
    return current.providers
        .where((provider) => provider.id == providerId)
        .expand((provider) => provider.allModels)
        .where((candidate) => candidate.slug == model)
        .firstOrNull;
  }

  bool _acceptsAttachments(
    ComposerThreadState composer,
    ProviderModelView target,
  ) {
    final supported = target.inputCapabilities
        .map((capability) => capability.modality)
        .toSet();
    final conflicts = composer.attachments
        .where(
          (attachment) => !supported.contains(switch (attachment.modality) {
            AttachmentModalityView.image => ModelModalityView.image,
            AttachmentModalityView.video => ModelModalityView.video,
            AttachmentModalityView.file => ModelModalityView.file,
          }),
        )
        .toList();
    if (conflicts.isEmpty) return true;
    final error = StateError(
      'Cannot switch model: ${conflicts.map((item) => item.filename).join(', ')} is not supported.',
    );
    final current = state.value;
    if (current == null) return false;
    state = AsyncData(
      current.selectedThreadId == null
          ? current.copyWith(
              newThreadComposerByProject: {
                ...current.newThreadComposerByProject,
                ?current.selectedProjectId: composer.reportFailure(error),
              },
            )
          : _withWorkspaceUi(
              current,
              current.selectedThreadId!,
              (ui) => ui.copyWith(composer: composer.reportFailure(error)),
            ),
    );
    return false;
  }

  Future<void> saveProviderSettings(ProviderSettingsCommand command) async {
    await _saveConfigSettings(
      (revision) => _api.saveProviderSettings(revision, command),
    );
  }

  Future<void> saveInstructionsSettings(
    InstructionsSettingsCommand command,
  ) async {
    await _saveConfigSettings(
      (revision) => _api.saveInstructionsSettings(revision, command),
    );
  }

  Future<void> saveSkillsSettings(SkillsSettingsCommand command) async {
    await _saveConfigSettings(
      (revision) => _api.saveSkillsSettings(revision, command),
    );
  }

  Future<void> saveMcpSettings(McpSettingsCommand command) async {
    await _saveConfigSettings(
      (revision) => _api.saveMcpSettings(revision, command),
    );
  }

  Future<void> saveGeneralSettings(GeneralSettingsCommand command) async {
    await _saveConfigSettings((revision) {
      final general = state.requireValue.general;
      return _api.saveGeneralSettings(
        revision,
        GeneralSettingsCommand(
          followActiveTurn: command.followActiveTurn,
          compactTimeline: command.compactTimeline,
          sidebarWidth: command.sidebarWidth ?? general.sidebarWidth,
          pinnedThreadIds: command.pinnedThreadIds ?? general.pinnedThreadIds,
          pinnedProjectIds:
              command.pinnedProjectIds ?? general.pinnedProjectIds,
        ),
      );
    });
  }

  Future<void> saveWebSearchSettings(WebSearchSettingsCommand command) async {
    await _saveConfigSettings(
      (revision) => _api.saveWebSearchSettings(revision, command),
    );
  }

  Future<void> saveDeepSeekWebSearchSettings(
    DeepSeekWebSearchSettingsCommand command,
  ) async {
    await _saveConfigSettings(
      (revision) => _api.saveDeepSeekWebSearchSettings(revision, command),
    );
  }

  Future<void> setSystemAgentEnabled({
    required String profileId,
    required bool enabled,
  }) async {
    await _saveConfigSettings(
      (revision) => _api.setSystemAgentEnabled(
        expectedSettingsRevision: revision,
        profileId: profileId,
        enabled: enabled,
      ),
    );
  }

  Future<void> saveUserAgentProfile(AgentProfileDraft draft) async {
    await _saveConfigSettings(
      (revision) => _api.saveUserAgentProfile(revision, draft),
    );
  }

  Future<void> retryRecovery() async {
    final next = await _api.retryRecovery();
    final current = state.value;
    if (current != null) state = AsyncData(applyRecoveryState(current, next));
  }

  Future<void> cleanupPreservedWorktree(WorktreeRecoveryPreview worktree) =>
      _api.cleanupPreservedWorktree(
        ownerKind: worktree.ownerKind.id,
        ownerThreadId: worktree.ownerThreadId,
        expectedLeaseRevision: worktree.leaseRevision,
      );

  Future<void> _saveConfigSettings(
    Future<SettingsStateSnapshot> Function(int revision) request,
  ) async {
    final current = state.value;
    if (current == null) return;
    final next = await request(current.settingsRevision);
    final latest = state.value;
    if (latest != null) state = AsyncData(applySettingsState(latest, next));
  }

  Future<void> refreshProviderUsages() async {
    final current = state.value;
    if (current == null) return;
    final usageState = await _api.checkProviderUsage();
    final latest = state.value;
    if (latest != null) {
      state = AsyncData(applyProviderUsageState(latest, usageState));
    }
  }

  Future<void> refreshSkillsState() async {
    final projectId = state.value?.selectedProjectId;
    if (projectId == null) return;
    final snapshot = await _api.readSkillsState(projectId);
    final latest = state.value;
    if (latest != null) state = AsyncData(applySkillsState(latest, snapshot));
  }

  Future<List<String>> discoverSkills() async {
    final projectId = state.value?.selectedProjectId;
    if (projectId == null) return const [];
    final snapshot = await _api.discoverSkills(projectId);
    final latest = state.value;
    if (latest != null) state = AsyncData(applySkillsState(latest, snapshot));
    return snapshot.skills;
  }

  Future<SkillSearchResultView?> searchSkills(
    String query, {
    int limit = 50,
  }) async {
    final projectId = state.value?.selectedProjectId;
    if (projectId == null) return null;
    return _api.searchSkills(projectId, query, limit: limit);
  }

  Future<void> refreshMcpState() async {
    await _applyMcpCommand(_api.readMcpState);
  }

  Future<void> resetMcpServer(String serverId) async {
    await _applyMcpCommand(() => _api.resetMcpServer(serverId));
  }

  Future<void> resetAllMcp() async {
    await _applyMcpCommand(_api.resetAllMcp);
  }

  Future<void> _applyMcpCommand(
    Future<McpStateSnapshot> Function() command,
  ) async {
    if (state.value == null) return;
    final snapshot = await command();
    final latest = state.value;
    if (latest != null) state = AsyncData(applyMcpState(latest, snapshot));
  }

  Future<void> refreshLspState() async {
    await _applyLspCommand(_api.readLspState);
  }

  Future<void> probeLspServer() async {
    final projectId = state.value?.selectedProjectId;
    if (projectId == null) return;
    await _applyLspCommand(() => _api.probeLspServer(projectId));
  }

  Future<void> repairLspServer(String serverId) async {
    final projectId = state.value?.selectedProjectId;
    if (projectId == null) return;
    await _applyLspCommand(() => _api.repairLspServer(projectId, serverId));
  }

  Future<void> resetLspServer(String serverId) async {
    final projectId = state.value?.selectedProjectId;
    if (projectId == null) return;
    await _applyLspCommand(() => _api.resetLspServer(projectId, serverId));
  }

  Future<void> resetLspWorkspace() async {
    final projectId = state.value?.selectedProjectId;
    if (projectId == null) return;
    await _applyLspCommand(() => _api.resetLspWorkspace(projectId));
  }

  Future<void> _applyLspCommand(
    Future<LspStateSnapshot> Function() command,
  ) async {
    if (state.value == null) return;
    final snapshot = await command();
    final latest = state.value;
    if (latest != null) state = AsyncData(applyLspState(latest, snapshot));
  }

  void retryInitialization() => ref.invalidateSelf();

  Future<void> retryPersistence() async {
    final current = state.value;
    if (current == null) return;
    final persistence = await _api.retryPersistence();
    if (!ref.mounted) return;
    final latest = state.value;
    if (latest == null) return;
    state = AsyncData(applyPersistenceState(latest, persistence));
  }

  Future<PersistenceQueueSnapshot> retryThreadHistory(
    String threadId,
    int faultGeneration,
  ) => _api.retryThreadHistory(threadId, faultGeneration);

  /// 读取进程级持久化队列压力；只在当前 API 实现该观测能力时返回，否则为未知。
  ///
  /// 该值用于诊断展示，不写入会话状态、也不驱动任何执行：它是协调器已观测到的真实
  /// 队列压力，调用方据此区分“落后量已知”与“无法观测”，而不是编造本地计数。
  Future<PersistenceQueueSnapshot?> readPersistenceQueue() async {
    final api = _api;
    // 声明式模式绑定：未实现该观测能力的 API 返回 null（未知），不编造本地计数。
    if (api case final PersistenceQueueReader reader) {
      return reader.readPersistenceQueue();
    }
    return null;
  }

  Future<void> resolveActiveInteraction(
    String threadId,
    String interactionId,
    InteractionResolutionCommand resolution,
  ) async {
    final current = state.value;
    final workspace = current?.workspacesByThread[threadId];
    final interaction = current?.activeInteraction;
    if (current == null ||
        current.selectedThreadId != threadId ||
        workspace == null ||
        interaction == null ||
        interaction.id != interactionId) {
      throw _interactionConflict();
    }
    final interactionsBeforeResponse = {
      for (final candidate in workspace.interactions) candidate.id,
    };
    await _api.respondInteraction(interactionId, resolution);
    if (!ref.mounted) return;
    final latest = state.value;
    final active = latest?.workspacesByThread[threadId];
    if (latest == null || active == null) return;
    final pending = [...active.interactions]
      ..sort(
        (left, right) =>
            interactionPriority(left.kind)
                .compareTo(interactionPriority(right.kind)),
      );
    final replacement = pending.firstOrNull;
    if (replacement != null &&
        replacement.id != interactionId &&
        !interactionsBeforeResponse.contains(replacement.id)) {
      throw _interactionConflict();
    }
    state = AsyncData(
      latest.copyWith(
        workspacesByThread: {
          ...latest.workspacesByThread,
          threadId: active.copyWith(
            interactions: active.interactions
                .where((candidate) => candidate.id != interactionId)
                .toList(),
          ),
        },
      ),
    );
  }

  StudioFailure _interactionConflict() => const StudioFailure(
    code: StudioFailureCode.conflict,
    message: 'The displayed interaction is no longer current',
    retryable: false,
    correlationId: 'client-interaction-conflict',
  );

  void _handleProductEvent(Object event) {
    final current = state.value;
    if (current == null || event is! StudioBridgeEvent) return;
    if (event.payload is StalePayload) {
      unawaited(_reloadProductState());
      return;
    }
    final previousThreadId = current.selectedThreadId;
    var next = reduceStudioEvent(current, event).state;
    // 归档/关闭会从 workspaces 移除该会话：立即释放它的会话级 map。
    for (final threadId in current.workspacesByThread.keys) {
      if (!next.workspacesByThread.containsKey(threadId)) {
        _releaseThreadSession(threadId);
      }
    }
    if (previousThreadId != next.selectedThreadId) {
      if (previousThreadId != null) {
        _releaseThreadSession(previousThreadId);
        next = releaseThreadHistoryPayload(next, previousThreadId);
      }
      // 非显式选择变化不继承“已打开”：新选中会话保持未打开，等待用户交互（§6.1）。
      next = _markThreadUnopened(next, next.selectedThreadId);
      state = AsyncData(next);
      return;
    }
    state = AsyncData(next);
    if (event.payload is PersistenceStateChangedPayload &&
        next.persistenceState.state.pendingCommits == 0) {
      final threadId = next.selectedThreadId;
      if (threadId != null &&
          next.workspacesByThread[threadId]?.liveItems.values.any(
                (item) => item.isTerminal,
              ) ==
              true) {
        _scheduleTerminalRefresh(threadId);
      }
    }
  }

  /// Product 流终止（bridge 的 failure/closed）不是正常结束：读取一次 canonical
  /// snapshot 重同步；协调器随后有界重订阅，内存状态与实时更新都不会静默停摆。
  void _onProductStreamTerminated() {
    unawaited(_reloadProductState());
  }

  Future<void> _reloadProductState({
    _SelectionIntent selection = const _PreserveSelection(),
  }) async {
    try {
      final current = state.value;
      await _adoptProductState(
        _resolveSelection(
          await _api.readStudioState(),
          previous: current,
          intent: selection,
        ),
      );
    } on Object {
      // Product stream will retry on the next explicit action or app reload.
    }
  }

  void _handleThreadFrame(
    ThreadStreamFrame frame,
    String threadId,
    int generation,
  ) {
    final stopwatch = kDebugMode ? (Stopwatch()..start()) : null;
    try {
      if (frame case ThreadNotificationFrame(update: ThreadItemDeltaUpdate())) {
        _pendingThreadDeltas.add((frame, threadId, generation));
        if (!_deltaFrameScheduled) {
          _deltaFrameScheduled = true;
          SchedulerBinding.instance.scheduleFrameCallback((_) {
            _deltaFrameScheduled = false;
            _flushThreadDeltas();
          });
        }
        return;
      }
      _flushThreadDeltas();
      _applyThreadFrame(frame, threadId, generation);
    } finally {
      if (stopwatch != null) {
        final elapsed = stopwatch.elapsedMicroseconds;
        _debugThreadFrameCount++;
        _debugThreadFrameMicros += elapsed;
        if (elapsed > _debugThreadFrameMaxMicros) {
          _debugThreadFrameMaxMicros = elapsed;
        }
        final now = DateTime.now().millisecondsSinceEpoch;
        if (_debugThreadFrameStartedAt == 0) _debugThreadFrameStartedAt = now;
        if (now - _debugThreadFrameStartedAt >= 500) {
          debugPrint(
            'timeline_frame_work count=$_debugThreadFrameCount '
            'total_us=$_debugThreadFrameMicros max_us=$_debugThreadFrameMaxMicros',
          );
          _debugThreadFrameCount = 0;
          _debugThreadFrameMicros = 0;
          _debugThreadFrameMaxMicros = 0;
          _debugThreadFrameStartedAt = now;
        }
      }
    }
  }

  void _flushThreadDeltas() {
    if (_pendingThreadDeltas.isEmpty || !ref.mounted) return;
    final pending = List<(ThreadNotificationFrame, String, int)>.of(
      _pendingThreadDeltas,
    );
    _pendingThreadDeltas.clear();
    var next = state.value;
    for (final (frame, threadId, generation) in pending) {
      if (next == null ||
          generation != _threadCoordinator.generation ||
          next.selectedThreadId != threadId ||
          _workspaceUi(next, threadId).subscriptionGeneration != generation) {
        continue;
      }
      final epoch = frame.epoch;
      final knownEpoch = _streamEpochByThread[threadId];
      if (epoch != null && knownEpoch != null && knownEpoch != epoch) {
        unawaited(_resyncThread(threadId, generation));
        return;
      }
      if (epoch != null) _streamEpochByThread[threadId] = epoch;
      final reduced = applyThreadUpdate(
        next,
        threadId: threadId,
        revision: frame.revision,
        update: frame.update,
        baseRevision: frame.baseRevision,
        chatWindowOwnsItems:
            _chatWindowThreadId == threadId && _chatWindow != null,
        filteredItemRevisions: _api is ChatWindowReader,
      );
      if (reduced.resyncThreadId != null) {
        unawaited(_resyncThread(threadId, generation));
        return;
      }
      next = reduced.state;
    }
    if (next != null && !identical(next, state.value)) {
      state = AsyncData(next);
    }
  }

  void _applyThreadFrame(
    ThreadStreamFrame frame,
    String threadId,
    int generation,
  ) {
    final frameThreadId = switch (frame) {
      ThreadSnapshotFrame(:final workspace) => workspace.thread.id,
      ThreadNotificationFrame(:final threadId) => threadId,
      ThreadResyncRequiredFrame(:final threadId) => threadId,
    };
    if (frameThreadId != threadId) return;
    final current = state.value;
    if (current == null ||
        generation != _threadCoordinator.generation ||
        current.selectedThreadId != threadId ||
        _workspaceUi(current, threadId).subscriptionGeneration != generation) {
      return;
    }
    switch (frame) {
      case ThreadSnapshotFrame(:final workspace):
        // 首帧只替换当前状态；历史条目由订阅建立后的窗口读取提供。生产端把
        // (重)订阅建立表达为首个 snapshot：这里以它为界读取一次权威窗口，覆盖
        // owner 尚未激活时被跳过的首窗读取，并采纳数据库身份/水位。同一订阅世代
        // 只读一次，避免每个 snapshot 重复全窗读取。
        state = AsyncData(applyThreadSnapshot(current, workspace));
        unawaited(_reloadTimelineWindow(threadId, generation));
      case ThreadNotificationFrame(:final revision, :final update):
        final epoch = frame.epoch;
        final knownEpoch = _streamEpochByThread[threadId];
        if (epoch != null && knownEpoch != null && knownEpoch != epoch) {
          // 生产端连续广播生命周期切换：旧 epoch 的帧全部作废，重新订阅。
          unawaited(_resyncThread(threadId, generation));
          return;
        }
        if (epoch != null) {
          _streamEpochByThread[threadId] = epoch;
        }
        final reduced = applyThreadUpdate(
          current,
          threadId: threadId,
          revision: revision,
          update: update,
          baseRevision: frame.baseRevision,
          chatWindowOwnsItems:
              _chatWindowThreadId == threadId && _chatWindow != null,
          filteredItemRevisions: _api is ChatWindowReader,
        );
        if (reduced.resyncThreadId != null) {
          unawaited(_resyncThread(threadId, generation));
          return;
        }
        state = AsyncData(reduced.state);
        if (_chatWindow == null) {
          if (update case ThreadItemUpsert(:final item) when item.isTerminal) {
            _scheduleTerminalRefresh(threadId);
          }
        }
      case ThreadResyncRequiredFrame():
        unawaited(_resyncThread(threadId, generation));
    }
  }

  Future<void> _resyncThread(String threadId, int generation) async {
    if (generation != _threadCoordinator.generation ||
        state.value?.selectedThreadId != threadId) {
      return;
    }
    _markThreadDisconnected(threadId, generation);
    // A gap invalidates the stream now. The new subscription owns recovery;
    // subsequent old frames cannot postpone it by resetting the retry timer.
    await _subscribeThread(threadId);
  }

  void _markThreadDisconnected(
    String threadId,
    int generation, [
    Object? error,
  ]) {
    _streamEpochByThread.remove(threadId);
    final current = state.value;
    if (current == null ||
        generation != _threadCoordinator.generation ||
        current.selectedThreadId != threadId) {
      return;
    }
    if (_workspaceUi(current, threadId).syncState ==
            AgentWorkspaceSyncState.failed &&
        error == null) {
      return;
    }
    state = AsyncData(
      _withWorkspaceUi(
        current,
        threadId,
        (ui) => ui.copyWith(
          syncState: error == null
              ? AgentWorkspaceSyncState.reconnecting
              : AgentWorkspaceSyncState.failed,
          loadError: error?.toString(),
        ),
      ),
    );
    if (error != null) return;
    _threadCoordinator.scheduleResubscribe(
      threadId: threadId,
      generation: generation,
      isCurrent: () => state.value?.selectedThreadId == threadId,
      resubscribe: () => unawaited(_subscribeThread(threadId)),
    );
  }

  Future<void> _adoptProductState(StudioState incoming) async {
    final current = state.value;
    final previousThreadId = current?.selectedThreadId;
    // 选择已由显式 selection intent 解析并随 incoming 携带；这里不再改写。
    var next = current == null
        ? incoming
        : _mergeProductSnapshots(
            current,
            incoming,
          ).copyWith(providerCatalog: current.providerCatalog);
    if (previousThreadId != next.selectedThreadId && previousThreadId != null) {
      // 选择切换（例如目录事件把焦点移到别的 Thread）同样释放上一个会话的历史载荷。
      _releaseThreadSession(previousThreadId);
      next = releaseThreadHistoryPayload(next, previousThreadId);
    }
    if (previousThreadId != next.selectedThreadId) {
      // 非显式选择变化不继承“已打开”：新选中会话保持未打开，等待用户交互（§6.1）。
      next = _markThreadUnopened(next, next.selectedThreadId);
    }
    state = AsyncData(next);
    // 选择变化只更新选择并释放旧载荷：产品快照/目录事件不是“用户打开会话”，
    // 新选中的会话保持未打开，等待显式交互（§6.1）。
  }
}

StudioState _mergeProductSnapshots(StudioState current, StudioState incoming) {
  var next = applyProjectDirectory(current, incoming.projectDirectory);
  // 目录是分页窗口：resync snapshot 的首页整体替换当前窗口；选择采纳
  // incoming 携带的显式解析结果（_resolveSelection 是唯一解析点）。
  next = next.copyWith(
    threadDirectory: incoming.threadDirectory,
    selectedProjectId: incoming.selectedProjectId,
    selectedThreadId: incoming.selectedThreadId,
  );
  next = applyAgentDirectory(next, incoming.agentDirectory);
  next = applySettingsState(next, incoming.settingsState);
  next = applyRecoveryState(next, incoming.recoveryState);
  next = applyMcpState(next, incoming.mcpState);
  next = applyLspState(next, incoming.lspState);
  next = applyThreadModeCatalog(next, incoming.threadModeCatalog);
  next = applyProviderUsageState(next, incoming.providerUsageState);
  next = applyModelPerformanceState(next, incoming.modelPerformance);
  next = applyUpdaterState(next, incoming.updaterState);
  for (final snapshot in incoming.skillsByProject.values) {
    next = applySkillsState(next, snapshot);
  }
  return next;
}

WorkspaceUiState _workspaceUi(StudioState state, String threadId) {
  // 首屏只恢复选择，不打开会话（§6.1）：从未打开的会话没有 UI 条目，语义是
  // `idle`（尚未打开），不能沿用 `WorkspaceUiState` 的默认 `loading`——否则
  // `openThread` 会把“尚未打开”当成“正在打开”而直接返回，用户永远打不开首屏
  // 已选中的会话。与 `StudioState.selectedWorkspaceUi` 的判读保持一致。
  return state.workspaceUiByThread[threadId] ??
      const WorkspaceUiState(syncState: AgentWorkspaceSyncState.idle);
}

StudioState _withWorkspaceUi(
  StudioState state,
  String threadId,
  WorkspaceUiState Function(WorkspaceUiState ui) update,
) {
  return state.copyWith(
    workspaceUiByThread: {
      ...state.workspaceUiByThread,
      threadId: update(_workspaceUi(state, threadId)),
    },
  );
}

StudioState _attachProviderCatalog(
  StudioState state,
  ProviderCatalogView catalog,
) {
  return state.copyWith(providerCatalog: catalog);
}

sealed class _SelectionIntent {
  const _SelectionIntent();
}

final class _BootstrapSelection extends _SelectionIntent {
  const _BootstrapSelection({
    required this.preferredProjectId,
    required this.preferredThreadId,
  });

  final String? preferredProjectId;
  final String? preferredThreadId;
}

final class _PreserveSelection extends _SelectionIntent {
  const _PreserveSelection();
}

final class _ProjectDefaultSelection extends _SelectionIntent {
  const _ProjectDefaultSelection(this.projectId);

  final String? projectId;
}

final class _ExactThreadSelection extends _SelectionIntent {
  const _ExactThreadSelection({
    required this.projectId,
    required this.threadId,
  });

  final String? projectId;
  final String threadId;
}

StudioState _resolveSelection(
  StudioState incoming, {
  required StudioState? previous,
  required _SelectionIntent intent,
}) {
  final requestedProjectId = switch (intent) {
    _BootstrapSelection(:final preferredProjectId) => preferredProjectId,
    _PreserveSelection() => previous?.selectedProjectId,
    _ProjectDefaultSelection(:final projectId) => projectId,
    _ExactThreadSelection(:final projectId) => projectId,
  };
  final projectId =
      incoming.projects.any((project) => project.id == requestedProjectId)
      ? requestedProjectId
      : incoming.projects.firstOrNull?.id;
  final firstRootId = incoming.threads
      .where((thread) => thread.isRoot && thread.projectId == projectId)
      .firstOrNull
      ?.id;
  final threadId = switch (intent) {
    _BootstrapSelection(:final preferredThreadId) =>
      incoming.threads.any(
            (thread) =>
                thread.id == preferredThreadId && thread.projectId == projectId,
          )
          ? preferredThreadId
          : firstRootId,
    _ProjectDefaultSelection() => firstRootId,
    _ExactThreadSelection(:final threadId) => threadId,
    _PreserveSelection() => _preservedThreadSelection(
      incoming,
      previous,
      projectId,
      firstRootId,
    ),
  };
  return incoming.copyWith(
    selectedProjectId: projectId,
    selectedThreadId: threadId,
  );
}

String? _preservedThreadSelection(
  StudioState incoming,
  StudioState? previous,
  String? projectId,
  String? firstRootId,
) {
  if (previous == null || previous.selectedProjectId != projectId) {
    return firstRootId;
  }
  final selectedThreadId = previous.selectedThreadId;
  if (selectedThreadId == null) return null;
  final knownProjectId =
      previous.threads
          .where((thread) => thread.id == selectedThreadId)
          .firstOrNull
          ?.projectId ??
      previous.workspacesByThread[selectedThreadId]?.thread.projectId ??
      incoming.threads
          .where((thread) => thread.id == selectedThreadId)
          .firstOrNull
          ?.projectId;
  return knownProjectId == null || knownProjectId == projectId
      ? selectedThreadId
      : firstRootId;
}

StudioState _applyArchiveResult(
  StudioState current,
  ArchiveThreadResult result,
) {
  final removed = result.removedThreadIds.toSet();
  var next = applyThreadDirectoryDelta(
    current,
    upserted: const [],
    removed: result.removedThreadIds,
  );
  final nextRoot = result.nextRoot;
  if (nextRoot != null &&
      !next.threadDirectory.threads.any((thread) => thread.id == nextRoot.id)) {
    final threads = [...next.threadDirectory.threads, nextRoot]
      ..sort((left, right) {
        final updated = right.updatedAt.compareTo(left.updatedAt);
        return updated != 0 ? updated : right.id.compareTo(left.id);
      });
    next = next.copyWith(
      threadDirectory: next.threadDirectory.copyWith(threads: threads),
    );
  }
  return next.copyWith(
    selectedThreadId:
        current.selectedThreadId != null &&
            removed.contains(current.selectedThreadId)
        ? nextRoot?.id
        : current.selectedThreadId,
  );
}

/// 侧栏目录分页加载的最近一次错误文案；null 表示无未恢复错误。
@Riverpod(keepAlive: true)
class DirectoryLoadError extends _$DirectoryLoadError {
  @override
  String? build() => null;

  void set(String? message) => state = message;
}
