import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/foundation.dart'
    show debugPrint, kDebugMode, visibleForTesting;
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../../domain/models/studio_models.dart';
import '../../domain/models/timeline_commands.dart';
import '../../platform/clipboard_image_reader.dart';
import '../frb/studio_api.dart';
import 'studio_api_provider.dart';
import 'studio_product_topics.dart';
import 'studio_state_reducer.dart';
import 'studio_stream_coordinators.dart';
import 'studio_topic_reducer.dart';

part 'studio_controller.g.dart';

Duration? _disableStudioRetry(int retryCount, Object error) => null;

final class _ControllerTimelineSession implements TimelineSession {
  const _ControllerTimelineSession({
    required this.events,
    required this.onDispatch,
    required this.onClose,
  });

  @override
  final Stream<TimelineEvent> events;
  final Future<void> Function(TimelineCommand command) onDispatch;
  final Future<void> Function() onClose;

  @override
  Future<void> dispatch(TimelineCommand command) => onDispatch(command);

  @override
  Future<void> close() => onClose();
}

enum _ChatWindowLoad { older, newer, extendLatest }

@Riverpod(keepAlive: true, retry: _disableStudioRetry)
class StudioController extends _$StudioController {
  static bool _startupProjectActivated = false;

  late ProductTopicRegistry _topics;
  late ThreadStreamCoordinator _threadCoordinator;

  final StreamController<TimelineEvent> _timelineEvents =
      StreamController<TimelineEvent>.broadcast();
  final Map<String, int> _timelineEventVersions = {};

  /// 当前打开会话的唯一事件流。调用方只能订阅指定 threadId，不会收到其他会话事件。
  Stream<TimelineEvent> timelineEvents(String threadId) =>
      _timelineEvents.stream.where((event) => event.threadId == threadId);

  /// 将 TimelineView 的 typed command 路由到当前会话。
  Future<void> dispatchTimelineCommand(
    String threadId,
    TimelineCommand command,
  ) async {
    switch (command) {
      case TimelineLoadOlder():
        await loadOlderHistory(threadId);
      case TimelineLoadNewer():
        await loadNewerHistory(threadId);
      case TimelineExtendLatest():
        await extendLatestHistory(threadId);
      case TimelineJumpToLatest():
        await jumpToLatest(threadId);
      case TimelineExpandBody(:final itemId):
        await loadItemBody(threadId, itemId);
      case TimelineVisibleBodies(:final itemIds):
        await ensureItemBodies(threadId, itemIds);
      case TimelineAnchorChanged(:final anchor):
        updateTimelineAnchor(threadId, anchor);
    }
  }

  TimelineSession timelineSession(String threadId) =>
      _ControllerTimelineSession(
        events: timelineEvents(threadId),
        onDispatch: (command) => dispatchTimelineCommand(threadId, command),
        onClose: () async {
          if (_chatWindowThreadId == threadId) _closeChatWindow();
        },
      );

  /// Shell 常驻产品 topics 租约（导航目录/公共配置/诊断 summary）。
  ProductTopicLeaseBundle? _shellLease;

  /// 选中会话所属 root 的费用租约；随会话切换/离开释放。
  ProductTopicLease? _sessionCostsLease;

  /// 会话阅读面可见性：不可见时不持有 Thread 状态流/ChatView 窗口/费用租约。
  bool _conversationVisible = true;

  /// 会话阅读面可见性的单调生命周期代次：快速离开/重入/再离开时，旧异步收尾
  /// （旧 false continuation）不得清除后来 true 建立的窗口/费用租约。
  int _visibilityGeneration = 0;

  /// 串行化可见性切换与订阅建立的 barrier。
  Future<void> _visibilityBarrier = Future<void>.value();

  /// 当前已建立状态流订阅的会话；可见时等于选中会话，不可见时为 null。
  String? _subscribedThreadId;
  final Set<String> _historyRequests = {};
  final Map<String, int> _windowLoadGeneration = {};
  StudioChatWindow? _chatWindow;
  String? _chatWindowThreadId;
  String? _chatFocusedItemId;
  int _chatWindowOperation = 0;
  Future<void>? _openingChatWindow;

  /// 新会话 model route 保存的串行链尾：快速切换模型/effort 按顺序落库，每次执行时重读最新
  /// revision，避免并发 CAS 互冲；链本身永不失败，单次失败由 [_modeRouteSaveError] 记录。
  Future<void> _modeRouteSaveChain = Future<void>.value();

  /// All settings mutations share one write barrier.  This keeps the in-memory
  /// snapshot and the backend CAS revision ordered across independent Settings
  /// tabs, instead of letting a late save overwrite a newer one.
  Future<void> _settingsWriteBarrier = Future<void>.value();

  /// 最近一次新会话 model route 保存失败；成功后清空。submit 前若仍存在则拒绝用旧模型启动。
  Object? _modeRouteSaveError;

  /// 固定活动条展开详情的在途请求：同一活动身份只保留一个，避免每 token 排队全量读。
  Future<void>? _activityDetailInFlight;
  String? _activityDetailInFlightThread;
  String? _activityDetailInFlightIdentity;
  String? _activityDetailExpandedThread;

  /// 在途详情请求期间活动版本又前进过：完成后再补取一次最新版本。
  ///
  /// 只合并成一个尾随请求，不为一串 token 排队全量读取；末帧 token 落在在途请求
  /// 之后时也一定能被取到。
  bool _activityDetailRefreshQueued = false;
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
    _topics = ProductTopicRegistry(
      _api,
      onFrame: _handleTopicFrame,
      onTopicConnection: _handleTopicConnection,
    );
    _threadCoordinator = ThreadStreamCoordinator(
      _api,
      _handleThreadFrame,
      _markThreadDisconnected,
    );
    ref.onDispose(() {
      _closeChatWindow();
      final shellLease = _shellLease;
      _shellLease = null;
      if (shellLease != null) unawaited(shellLease.release());
      _releaseSessionCostsLease();
      unawaited(_topics.releaseAll());
      unawaited(_threadCoordinator.dispose());
      _windowLoadGeneration.clear();
      _streamEpochByThread.clear();
      _historyRequests.clear();
      _timelineEventVersions.clear();
      unawaited(_timelineEvents.close());
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
    // 先提交 bootstrap 状态再建立 Shell 常驻 topics：Baseline 首帧到达时
    // reducer 已能读到 state（帧不会被 AsyncLoading 阶段丢弃）。
    state = AsyncData(bootstrapped);
    // Shell 常驻 topics：导航目录、公共配置与诊断 summary。其余 topics 只随
    // 可见页面/面板按需租用（设置 tab、持久化面板、会话费用）。
    _shellLease = _topics.acquireBundle(shellProductTopics);
    // 启动只读取全局配置、工作区与会话目录，并恢复“选择”；首个 GUI 帧
    // 不加载会话状态或历史。GUI 在首帧之后通过 openSelectedThread 打开当前会话，
    // 不自动恢复模型或工具执行。
    _activateStartupProject(bootstrapped);
    debugPrint(
      'startup_stage=controller_ready elapsed_ms=${startupWatch.elapsedMilliseconds}',
    );
    // 返回当前 canonical state：Shell 租约建立时写入的局部传输连接状态已并入，
    // 避免 build 完成时的 data(...) 回写覆盖这些条目。
    return state.value ?? bootstrapped;
  }

  /// 重置启动激活 guard，仅用于隔离测试。
  @visibleForTesting
  static void resetStartupProjectActivation() {
    _startupProjectActivated = false;
  }

  /// 单个 topic 的局部传输连接状态；`null` 表示该 topic 的租约已全部释放。
  ///
  /// 连接状态是独立于业务 snapshot 的 transport 事实：Failure/closed 时 canonical
  /// 领域数据保持 last-known value，只在此呈现局部错误/reconnecting。释放时清理
  /// 该 topic 的连接墓碑，并对不再选中/租用的 session costs、skills 作用域条目做
  /// release cleanup，避免无界增长。
  void _handleTopicConnection(
    ProductTopic topic,
    ProductTopicConnectionStateView? view,
  ) {
    if (!ref.mounted) return;
    final current = state.value;
    if (current == null) return;
    if (view != null) {
      if (current.topicConnections[topic] == view) return;
      state = AsyncData(
        current.copyWith(
          topicConnections: {...current.topicConnections, topic: view},
        ),
      );
      return;
    }
    final connections = {...current.topicConnections}..remove(topic);
    var costs = current.sessionCostsByRoot;
    var skills = current.skillsByProject;
    if (topic is SessionCostsTopic) {
      final selectedRoot = current.selectedRootThread?.id;
      if (topic.rootThreadId != selectedRoot &&
          costs.containsKey(topic.rootThreadId)) {
        costs = {...costs}..remove(topic.rootThreadId);
      }
    } else if (topic is SkillsTopic) {
      if (topic.projectId != current.selectedProjectId &&
          skills.containsKey(topic.projectId)) {
        skills = {...skills}..remove(topic.projectId);
      }
    }
    final unchanged =
        connections.length == current.topicConnections.length &&
        identical(costs, current.sessionCostsByRoot) &&
        identical(skills, current.skillsByProject);
    if (unchanged) return;
    state = AsyncData(
      current.copyWith(
        topicConnections: connections,
        sessionCostsByRoot: costs,
        skillsByProject: skills,
      ),
    );
  }

  /// 会话阅读面可见性合同（Shell 进入设置/离开会话页时调用）。
  ///
  /// 使用单调生命周期代次 + 串行 barrier，覆盖 `false→true→false`（即使布尔值重复）：
  /// 被后续调用取代的旧 continuation 不得清除新建立的窗口/费用租约。
  ///
  /// `visible = false`：取消 Thread 状态订阅（barrier 等待旧流真正取消）、关闭
  /// ChatView 窗口并释放所属 root 的 SessionCosts 租约；迟到帧按 generation 拒绝。
  /// 选择、已打开标记、草稿与阅读锚点全部保留。
  /// `visible = true`：已打开的当前选择重新建立状态流，首个权威 snapshot 后按
  /// 保留的锚点恢复阅读窗口。
  Future<void> setConversationVisible(bool visible) {
    _conversationVisible = visible;
    final generation = ++_visibilityGeneration;
    final operation = _visibilityBarrier.then((_) async {
      if (generation != _visibilityGeneration) return;
      if (!visible) {
        _threadCoordinator.switchThread(null);
        await _threadCoordinator.switchBarrier;
        if (generation != _visibilityGeneration) return;
        _closeChatWindow();
        _releaseSessionCostsLease();
        _subscribedThreadId = null;
        return;
      }
      final current = state.value;
      final threadId = current?.selectedThreadId;
      if (current == null || threadId == null) return;
      if (!current.openedThreadIds.contains(threadId)) return;
      if (_subscribedThreadId == threadId) return;
      await _subscribeThread(threadId);
    });
    _visibilityBarrier = operation.then(
      (_) {},
      onError: (Object _, StackTrace _) {},
    );
    return operation;
  }

  /// 按设置页可见 tab 获取 topics 租约；隐藏 tab 应 release，不持有订阅。
  ProductTopicLeaseBundle acquireSettingsScope(SettingsProductScopeKind kind) {
    return _topics.acquireBundle(
      settingsScopeTopics(kind, state.value?.selectedProjectId),
    );
  }

  /// 持久化队列诊断面板的按需租约：面板打开时获取，关闭时释放（替代周期轮询）。
  ProductTopicLease acquirePersistenceQueueScope() {
    return _topics.acquire(const PersistenceQueueTopic());
  }

  /// 显式获取某个 root 会话的费用租约（阅读面自身管理时使用）。
  ProductTopicLease acquireSessionCostsScope(String rootThreadId) {
    return _topics.acquire(SessionCostsTopic(rootThreadId: rootThreadId));
  }

  /// 只读诊断：活动 topic 与引用计数，供人工核对租约释放（不含凭据/内容）。
  Map<String, int> activeTopicRefCountViews() =>
      _topics.activeTopicRefCountViews();

  /// 显式重试某个 topic 的订阅：取消当前句柄、重置退避预算并重新接收基线。
  /// 供可见组件在局部 failure/closed 后由用户触发；该 topic 未租用时为空操作。
  void retryProductTopic(ProductTopic topic) => _topics.retry(topic);

  void _releaseSessionCostsLease() {
    final lease = _sessionCostsLease;
    _sessionCostsLease = null;
    unawaited(lease?.release());
  }

  /// 会话状态租约与当前选择对齐：切换/清空时释放旧 root 费用租约。
  void _syncSessionCostsLease(String? threadId) {
    final current = state.value;
    final rootId = threadId == null
        ? null
        : current?.threads
                  .where((thread) => thread.id == threadId)
                  .firstOrNull
                  ?.effectiveRootThreadId ??
              threadId;
    final existing = _sessionCostsLease;
    if (existing != null) {
      if (rootId != null &&
          existing.topic == SessionCostsTopic(rootThreadId: rootId)) {
        return;
      }
      _releaseSessionCostsLease();
    }
    if (rootId == null || rootId.isEmpty || !_conversationVisible) return;
    final lease = _topics.acquire(SessionCostsTopic(rootThreadId: rootId));
    _sessionCostsLease = lease;
  }

  void _activateStartupProject(StudioState bootstrapped) {
    if (_startupProjectActivated) return;
    final projectId = bootstrapped.selectedProjectId;
    if (projectId == null ||
        bootstrapped.recoveryIssue(
              blockingOnly: true,
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
              blockingOnly: true,
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
              blockingOnly: true,
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
    // 切换 mode 后，上一 mode 的未落库 model 意图不再相关，清除其失败标记以免误挡提交。
    _modeRouteSaveError = null;
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
      ..sort(StudioThread.compareDirectoryOrder);
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
              blockingOnly: true,
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
              blockingOnly: true,
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
          revision: latest.threadDirectory.revision,
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
              blockingOnly: true,
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
              blockingOnly: true,
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
    if (!_conversationVisible) {
      // 阅读面不可见：不建立状态流/费用租约；可见性恢复时按当前选择重开。
      _subscribedThreadId = null;
      _threadCoordinator.switchThread(null);
      _releaseSessionCostsLease();
      return;
    }
    _subscribedThreadId = threadId;
    final generation = _threadCoordinator.switchThread(threadId);
    _syncSessionCostsLease(threadId);
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
    if (_activityDetailExpandedThread == threadId) {
      _activityDetailExpandedThread = null;
      _activityDetailRefreshQueued = false;
    }
  }

  void _closeChatWindow() {
    final closedThreadId = _chatWindowThreadId;
    _chatWindowOperation++;
    _openingChatWindow = null;
    _chatWindowThreadId = null;
    _chatFocusedItemId = null;
    final window = _chatWindow;
    _chatWindow = null;
    if (window != null) unawaited(window.close());
    if (closedThreadId != null) {
      _timelineEvents.add(TimelineSessionClosed(closedThreadId));
      _timelineEventVersions.remove(closedThreadId);
    }
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
      if (anchor != null &&
          anchor.readingIntent == TimelineReadingIntent.browseHistory) {
        initial = await window.focus(anchor.itemId);
      }
      if (!_acceptChatWindow(threadId, operation)) return;
      _chatWindow = window;
      _adoptChatWindow(
        threadId,
        initial,
        historyCompletion: ChatHistoryCompletion.focus,
      );
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

  void _adoptChatWindow(
    String threadId,
    StudioChatSnapshot snapshot, {
    ChatHistoryCompletion? historyCompletion,
  }) {
    final current = state.value;
    if (current == null || current.selectedThreadId != threadId) return;
    _chatFocusedItemId = snapshot.focusedItemId;
    final previousVersion = _timelineEventVersions[threadId];
    _timelineEventVersions[threadId] = snapshot.version;
    if (previousVersion != null && snapshot.version > previousVersion) {
      _timelineEvents.add(
        TimelineWindowPatch(threadId, previousVersion, snapshot.version),
      );
    } else {
      _timelineEvents.add(TimelineWindowReset(threadId, snapshot.version));
    }
    state = AsyncData(
      applyChatWindowSnapshot(
        current,
        threadId,
        snapshot,
        historyCompletion: historyCompletion,
      ),
    );
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

  Future<void> extendLatestHistory(String threadId) async {
    await _ensureThreadOpen(threadId);
    await _loadChatWindow(threadId, _ChatWindowLoad.extendLatest);
  }

  /// 可见条目正文补齐：对窗口内仍是预览的可见身份按 identity 向同一 ChatView 请求展开。
  ///
  /// 这不是用户点击回源：身份进入视口即自动展开，因此智能体正文不需要“加载完整内容”
  /// 入口。展开是一次同 view 的窗口操作（`BridgeChatView.expand`）：原生窗口把完整正文
  /// retain 进窗口并交回权威快照，Dart 当成一次 Reset 应用，不另拼 SQL 页、不覆盖窗口基线。
  ///
  /// 运行中与终态的预览身份走**同一条**路径：超过窗口预览预算的流式正文同样会被截成预览，
  /// 因此按身份补齐与是否终态无关，助手正文（含实时过程）始终完整、不折叠。窗口对进行中身份
  /// 从内存返回完整正文，并在同一 view 内继续跟踪最新 revision（不每 token 重读历史）。
  /// 每个身份最多一个在途请求（`loadingItemIds` 即在途标记），正文完整后预览标记自然消失、
  /// 不会对已完整正文重复请求；窗口或会话切换后旧响应会被丢弃；不在窗口内、或已有错误的
  /// 身份不再自动重试。
  Future<void> ensureItemBodies(
    String threadId,
    Iterable<String> itemIds,
  ) async {
    for (final itemId in itemIds) {
      await _completeItemBody(threadId, itemId);
    }
  }

  /// 显式重试一次按 identity 的正文补齐（只对窗口内仍是预览的身份生效）。
  Future<void> loadItemBody(String threadId, String itemId) async {
    await _completeItemBody(threadId, itemId, retry: true);
  }

  Future<void> _completeItemBody(
    String threadId,
    String itemId, {
    bool retry = false,
  }) async {
    // 先确认“确实有可展开的窗口”再激活会话：可见性上报是帧末诊断，不得成为打开/订阅
    // 会话这类状态变更的副作用（展开本来就只走 ChatView 窗口自己的按身份读取）。
    final window = _chatWindowThreadId == threadId ? _chatWindow : null;
    if (window == null) return;
    await _ensureThreadOpen(threadId);
    final current = state.value;
    if (current == null ||
        current.selectedThreadId != threadId ||
        !current.workspacesByThread.containsKey(threadId)) {
      return;
    }
    final history = _workspaceUi(current, threadId).history;
    if (!history.previewedItemIds.contains(itemId) ||
        history.loadingItemIds.contains(itemId)) {
      return;
    }
    if (!retry && history.itemBodyErrors.containsKey(itemId)) {
      return;
    }
    // 自动补齐不再限定终态：上面的 `previewedItemIds` 已由窗口条目正文状态派生，只要求身份
    // 仍在窗口内、正文仍是预览。补齐完成后预览标记消失，不会对已完整的正文重复请求。
    state = AsyncData(startItemBodyLoad(current, threadId, itemId));
    try {
      final snapshot = await window.expandItem(itemId);
      if (!ref.mounted || _chatWindow != window) return;
      final latest = state.value;
      if (latest == null || latest.selectedThreadId != threadId) return;
      // null 只说明数据源此刻给不出完整正文（例如身份已离开窗口，或历史事务尚未
      // durable）：保留预览与重试入口，不把它当成“身份不存在”，也不本地编造正文。
      state = AsyncData(
        snapshot == null
            ? markItemBodyPending(latest, threadId, itemId)
            : applyChatWindowSnapshot(latest, threadId, snapshot),
      );
      if (snapshot != null) {
        _timelineEventVersions[threadId] = snapshot.version;
        _timelineEvents.add(
          TimelineBodyLoadCompleted(threadId, itemId, snapshot.version),
        );
      }
    } catch (error) {
      if (!ref.mounted || _chatWindow != window) return;
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
            detached:
                anchor.readingIntent == TimelineReadingIntent.browseHistory,
          ),
        ),
      ),
    );
    // 保存位置不等于授权历史聚焦：查看条目仍保留 canonical Latest 窗口。
    unawaited(_ensureThreadOpen(threadId));
    if (anchor.readingIntent == TimelineReadingIntent.browseHistory &&
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
    if (_chatWindowThreadId == threadId && _chatWindow != null) {
      // 原生窗口直接交回 canonical Latest。先清空会让已在 Latest 的幂等
      // focus 提前返回后留下空白，也会在异步聚焦期间丢掉正在显示的正文。
      await _focusChatWindow(threadId, null);
      // `_focusChatWindow` 对已经处于 Latest 的窗口是幂等的，此时仍必须
      // 清除 UI 保存的 browseHistory 锚点。否则下一次滚轮会按旧阅读意图
      // 重新打开历史窗口，表现为一次滚动跳回最早内容。
      final latest = state.value;
      if (latest != null && latest.selectedThreadId == threadId) {
        state = AsyncData(
          _withWorkspaceUi(
            latest,
            threadId,
            (ui) => ui.copyWith(
              history: ui.history.copyWith(
                hasNewer: false,
                detached: false,
                anchor: null,
              ),
            ),
          ),
        );
      }
      return;
    }
    state = AsyncData(jumpTimelineToLatest(current, threadId));
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
    final loaded =
        anchor != null &&
            anchor.readingIntent == TimelineReadingIntent.browseHistory
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
        await _loadChatWindow(threadId, switch (direction) {
          TimelineDirection.older => _ChatWindowLoad.older,
          TimelineDirection.newer => _ChatWindowLoad.newer,
        });
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
            ? history.olderCursor ?? workspace.items.firstOrNull?.id
            : history.newerCursor ?? workspace.items.lastOrNull?.id);
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
        _adoptChatWindow(
          threadId,
          snapshot,
          historyCompletion: ChatHistoryCompletion.focus,
        );
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

  Future<void> _loadChatWindow(String threadId, _ChatWindowLoad request) async {
    final window = _chatWindow;
    if (window == null || _chatWindowThreadId != threadId) return;
    final direction = request == _ChatWindowLoad.newer
        ? TimelineDirection.newer
        : TimelineDirection.older;
    if (!_historyRequests.add(threadId)) return;
    _timelineEvents.add(TimelinePagingStarted(threadId, direction));
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
      final snapshot = await switch (request) {
        _ChatWindowLoad.extendLatest => window.extendLatest(),
        _ChatWindowLoad.older ||
        _ChatWindowLoad.newer => window.load(direction),
      };
      if (_chatWindow == window && _acceptChatWindow(threadId, operation)) {
        _adoptChatWindow(
          threadId,
          snapshot,
          historyCompletion: switch (direction) {
            TimelineDirection.older => ChatHistoryCompletion.older,
            TimelineDirection.newer => ChatHistoryCompletion.newer,
          },
        );
        _timelineEvents.add(
          TimelinePagingCompleted(threadId, direction, snapshot.version),
        );
      }
    } catch (error) {
      _timelineEvents.add(TimelinePagingFailed(threadId, direction, error));
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

  /// 展开固定活动条详情：按当前活动身份**按需**读取完整内容。
  ///
  /// 调用发生在用户显式展开时；同一活动身份只保留一个在途请求（重复触发被合并），
  /// 活动版本前进时刷新，身份变化时丢弃旧结果。读取独立于消息窗口，不查 SQL 历史，
  /// 也不依赖该活动是否可见。
  void expandActivityDetail(String threadId) {
    _activityDetailExpandedThread = threadId;
    unawaited(_loadActivityDetail(threadId));
  }

  void collapseActivityDetail(String threadId) {
    if (_activityDetailExpandedThread == threadId) {
      _activityDetailExpandedThread = null;
      _activityDetailRefreshQueued = false;
    }
  }

  Future<void> _loadActivityDetail(String threadId) async {
    final current = state.value;
    if (!ref.mounted ||
        current == null ||
        current.selectedThreadId != threadId ||
        _activityDetailExpandedThread != threadId) {
      return;
    }
    final activity = current.workspacesByThread[threadId]?.activity;
    if (activity == null) return;
    if (_workspaceUi(
      current,
      threadId,
    ).activityDetail.covers(activity.identity, activity.revision)) {
      return;
    }
    // 合并同一身份的在途请求：不为一串 token 排队多次全量读取。合并掉的那次仍然
    // 记下“完成后再取一次”，否则末帧 token 可能永远取不到。
    if (_activityDetailInFlight != null &&
        _activityDetailInFlightThread == threadId &&
        _activityDetailInFlightIdentity == activity.identity) {
      _activityDetailRefreshQueued = true;
      return;
    }
    _activityDetailInFlightThread = threadId;
    _activityDetailInFlightIdentity = activity.identity;
    _setActivityDetailState(
      threadId,
      (state) => state.copyWith(
        identity: activity.identity,
        revision: activity.revision,
        loading: true,
        error: null,
      ),
    );
    // 可见性代次：阅读面切换后，旧在途详情的迟到返回不得写进新窗口/新活动状态。
    final visibility = _visibilityGeneration;
    final request = _performActivityDetailLoad(threadId, activity, visibility);
    _activityDetailInFlight = request;
    await request;
    if (identical(_activityDetailInFlight, request)) {
      _activityDetailInFlight = null;
      _activityDetailInFlightThread = null;
      _activityDetailInFlightIdentity = null;
    }
    // 在途期间活动又前进过（末帧 token 落在请求之后）：补取一次最新版本。
    // 版本没变时 [_loadActivityDetail] 会被 [ThreadActivityDetailState.covers] 拦下，
    // 因此这里最多只多出一次读取，不会为每个 token 排队。
    if (_activityDetailRefreshQueued &&
        ref.mounted &&
        _activityDetailExpandedThread == threadId) {
      _activityDetailRefreshQueued = false;
      unawaited(_loadActivityDetail(threadId));
    }
  }

  Future<void> _performActivityDetailLoad(
    String threadId,
    ThreadActivityView activity,
    int visibility,
  ) async {
    try {
      final detail = await _api.readThreadActivityDetail(
        threadId,
        activity.identity,
      );
      if (!ref.mounted || visibility != _visibilityGeneration) return;
      final latest = state.value;
      if (latest == null || latest.selectedThreadId != threadId) return;
      // 迟到的旧身份结果不得覆盖新身份（身份变化即丢弃）。
      final currentActivity = latest.workspacesByThread[threadId]?.activity;
      if (currentActivity != null &&
          currentActivity.identity != activity.identity) {
        return;
      }
      _setActivityDetailState(
        threadId,
        (state) => state.copyWith(
          identity: activity.identity,
          revision: activity.revision,
          loading: false,
          detail: detail,
          error: null,
        ),
      );
    } catch (error) {
      if (!ref.mounted || visibility != _visibilityGeneration) return;
      final latest = state.value;
      if (latest == null || latest.selectedThreadId != threadId) return;
      // 与成功路径同规：迟到的旧身份失败同样不得写进 `activityDetail`，否则会把新身份的
      // 在途/已成功状态覆盖成旧身份的错误。
      final currentActivity = latest.workspacesByThread[threadId]?.activity;
      if (currentActivity != null &&
          currentActivity.identity != activity.identity) {
        return;
      }
      _setActivityDetailState(
        threadId,
        (state) => state.copyWith(
          identity: activity.identity,
          revision: activity.revision,
          loading: false,
          error: error.toString(),
        ),
      );
    }
  }

  void _setActivityDetailState(
    String threadId,
    ThreadActivityDetailState Function(ThreadActivityDetailState) update,
  ) {
    final current = state.value;
    if (current == null || current.selectedThreadId != threadId) return;
    state = AsyncData(
      _withWorkspaceUi(
        current,
        threadId,
        (ui) => ui.copyWith(activityDetail: update(ui.activityDetail)),
      ),
    );
  }

  /// 侧栏触底加载下一页会话目录；内存未命中时由 bridge 从数据库分页取回。
  /// 测试入口：显式触发一次全量 canonical 重同步（显式命令路径，非轮询）。
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
              blockingOnly: true,
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

    // 先等所选 mode 的在途 model 路由保存落库，runtime 才会读到 canonical 并按所选模型创建
    // root；不得用本地乐观模型抢先创建。保存失败则展示错误并终止本次提交（保留草稿）。
    await _awaitModeRouteSaves();
    if (!ref.mounted) return;
    final routeError = _modeRouteSaveError;
    if (routeError != null) {
      final latest = state.value;
      if (latest == null) return;
      final active =
          latest.newThreadComposerByProject[projectId] ??
          const ComposerThreadState.idle();
      final failed = active.fail(
        routeError,
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
    final submitMode = state.value?.newThreadMode ?? current.newThreadMode;
    final submitWorkspaceMode =
        state.value?.newThreadWorkspaceMode.id ??
        current.newThreadWorkspaceMode.id;

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
        submitMode,
        workspaceMode: submitWorkspaceMode,
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
      revision: latest.threadDirectory.revision,
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
    await _saveConfigSettings(
      (revision) => _api.setModelRole(
        expectedSettingsRevision: revision,
        roleKey: roleKey,
        providerId: providerId,
        model: model,
        effort: effort ?? target?.reasoningEfforts.firstOrNull,
      ),
    );
  }

  /// 排队保存新会话 mode 的路由（provider/model/effort）。
  ///
  /// 返回的 future 完成即代表该次保存已结束：成功应用到 canonical，或失败已记录并展示。
  /// 多次快速切换通过 [_modeRouteSaveChain] 串行执行，每次执行时重读最新 settings revision，
  /// 因此不会并发 CAS 互冲；新会话提交进行中不再接受新的路由变更，保证 submit 等待的队列有界。
  Future<void> setModeModelRoute({
    required ThreadModeId mode,
    required String providerId,
    required String model,
    String? effort,
  }) {
    final current = state.value;
    if (current == null ||
        current.newThreadMode != mode ||
        current.newThreadComposer.isSubmissionPending) {
      return Future<void>.value();
    }
    final task = _modeRouteSaveChain.then(
      (_) => _runSetModeModelRoute(
        mode: mode,
        providerId: providerId,
        model: model,
        effort: effort,
      ),
    );
    _modeRouteSaveChain = task.then((_) {}, onError: (_) {});
    return task;
  }

  Future<void> _runSetModeModelRoute({
    required ThreadModeId mode,
    required String providerId,
    required String model,
    String? effort,
  }) async {
    if (!ref.mounted) return;
    await _awaitSettingsWrites();
    if (!ref.mounted) return;
    final current = state.value;
    if (current == null || current.newThreadMode != mode) return;
    final target = _findModel(current, providerId, model);
    if (target == null ||
        !_acceptsAttachments(current.newThreadComposer, target)) {
      // 当前 mode 的这一选择无法落库：记录并经既有 composer 错误通道展示，submit 据此拒绝用
      // 旧模型启动 root（不猜 provider 可用性，只报告所选路由不可用）。
      final error = StateError(
        'Selected model route is unavailable: $providerId / $model',
      );
      _modeRouteSaveError = error;
      reportComposerFailure(error);
      return;
    }
    final route = current.modeModelRoutes
        .where((candidate) => candidate.modeId == mode)
        .firstOrNull;
    if (route != null &&
        route.providerId == providerId &&
        route.model == model &&
        (effort == null || route.effort == effort)) {
      // 目标与 canonical 一致：没有待保存意图，清除仍属于当前 mode 的历史失败。
      if (state.value?.newThreadMode == mode) _modeRouteSaveError = null;
      return;
    }
    try {
      final next = await _api.setModeModelRoute(
        expectedSettingsRevision: current.settingsRevision,
        mode: mode,
        providerId: providerId,
        model: model,
        effort: effort ?? target.reasoningEfforts.firstOrNull,
      );
      // await 期间 controller 可能已被回收：不得再读取/写入 state。
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest != null) state = AsyncData(applySettingsState(latest, next));
      // 只清除仍属于当前 mode 的失败标记，避免吞掉其它 mode 的实际失败。
      if (state.value?.newThreadMode == mode) _modeRouteSaveError = null;
    } catch (error) {
      // 回收后不得读取/写入 state，也不把回收后的失败写进 UI。
      if (!ref.mounted) return;
      // 期间换 mode 时，旧 mode 的失败不应覆盖新 mode 的标记或误挡 submit。
      if (state.value?.newThreadMode == mode) {
        _modeRouteSaveError = error;
        reportComposerFailure(error);
      }
    }
  }

  /// 等待所有在途新会话 mode 路由保存结束。
  ///
  /// 提交进行中不再接受新的路由变更，因此这里最多再多等一轮即稳定，不会无限等待。
  Future<void> _awaitModeRouteSaves() async {
    while (true) {
      final tail = _modeRouteSaveChain;
      await tail;
      if (identical(tail, _modeRouteSaveChain)) return;
    }
  }

  Future<void> setThreadModelRoute({
    required String providerId,
    required String model,
    String? effort,
  }) async {
    // Thread route updates validate against the same settings revision as
    // configuration writes. Serialize them with the settings repository so a
    // provider/model edit cannot race this CAS operation.
    await _awaitSettingsWrites();
    if (!ref.mounted) return;
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
    final modelRoute = workspace.runtime.modelRoute;
    if (modelRoute == null) {
      state = AsyncData(
        _withWorkspaceUi(
          current,
          thread.id,
          (ui) => ui.copyWith(
            composer: ui.composer.reportFailure(
              StateError('Current Thread model route is unavailable.'),
            ),
          ),
        ),
      );
      return;
    }
    final ThreadModelRouteUpdateResult response;
    try {
      response = await _api.setThreadModelRoute(
        threadId: thread.id,
        expectedModelRouteRevision: modelRoute.revision,
        expectedSettingsRevision: current.settingsRevision,
        providerId: providerId,
        model: model,
        effort: effort ?? target.reasoningEfforts.firstOrNull,
      );
    } catch (error) {
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest == null ||
          !latest.threads.any((candidate) => candidate.id == thread.id)) {
        return;
      }
      state = AsyncData(
        _withWorkspaceUi(
          latest,
          thread.id,
          (ui) => ui.copyWith(composer: ui.composer.reportFailure(error)),
        ),
      );
      return;
    }
    if (!ref.mounted) return;
    final latest = state.value;
    if (latest == null) return;
    var next = applySettingsState(latest, response.settings);
    final latestWorkspace = next.workspacesByThread[thread.id];
    if (latestWorkspace != null) {
      final latestRouteRevision = latestWorkspace.runtime.modelRoute?.revision;
      final responseRoute = response.runtime.modelRoute;
      if (responseRoute != null &&
          (latestRouteRevision == null ||
              responseRoute.revision >= latestRouteRevision)) {
        next = next.copyWith(
          workspacesByThread: {
            ...next.workspacesByThread,
            thread.id: latestWorkspace.copyWith(
              runtime: latestWorkspace.runtime.copyWith(
                modelRoute: responseRoute,
              ),
            ),
          },
        );
      }
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

  Future<SettingsStateSnapshot> saveProviderSettings(
    ProviderSettingsCommand command,
  ) {
    return _saveConfigSettings(
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

  /// 保存网页搜索设置，并返回应用后的 canonical settings 快照（必非空）。
  ///
  /// 调用方用返回值把已发布的 canonical 值同步回草稿；状态不可读或保存失败都会抛出
  /// 类型化 [StudioFailure]，不返回 null，避免把未保存当成成功。其他 `save*` 消费者
  /// 忽略返回值，保持既有行为。
  Future<SettingsStateSnapshot> saveWebSearchSettings(
    WebSearchSettingsCommand command,
  ) {
    return _saveConfigSettings(
      (revision) => _api.saveWebSearchSettings(revision, command),
    );
  }

  /// 保存 DeepSeek 原生网页搜索开关，并返回应用后的 canonical settings 快照（必非空）。
  Future<SettingsStateSnapshot> saveDeepSeekWebSearchSettings(
    DeepSeekWebSearchSettingsCommand command,
  ) {
    return _saveConfigSettings(
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

  /// 统一保存 helper：返回已应用的 canonical settings 快照（必非空）。
  ///
  /// 状态不可读时抛出 [StudioFailureCode.notInitialized]，绝不返回 null 假冒成功。
  /// revision/CAS 冲突（[StudioFailureCode.staleRevision]/[StudioFailureCode.conflict]）
  /// 时先重新读取 backend canonical settings 并应用，再原样抛出冲突错误，让调用方基于
  /// 最新事实源显式重提交；刷新失败同样保留原冲突错误事实。
  Future<SettingsStateSnapshot> _saveConfigSettings(
    Future<SettingsStateSnapshot> Function(int revision) request,
  ) async {
    final operation = _settingsWriteBarrier.then((_) async {
      final current = state.value;
      if (current == null) throw _settingsNotReady();
      final next = await _requestSettings(request, current.settingsRevision);
      final latest = state.value;
      if (latest != null) {
        final updated = applySettingsState(latest, next);
        state = AsyncData(updated);
        return updated.settingsState;
      }
      return next;
    });
    _settingsWriteBarrier = operation.then<void>((_) {}, onError: (_) {});
    return operation;
  }

  Future<void> _awaitSettingsWrites() => _settingsWriteBarrier;

  /// 执行保存请求；revision/CAS 冲突时先刷新 canonical 再抛出原错误。
  Future<SettingsStateSnapshot> _requestSettings(
    Future<SettingsStateSnapshot> Function(int revision) request,
    int revision,
  ) async {
    try {
      return await request(revision);
    } on StudioFailure catch (error, stackTrace) {
      if (error.code == StudioFailureCode.staleRevision ||
          error.code == StudioFailureCode.conflict) {
        await _refreshCanonicalSettings();
      }
      Error.throwWithStackTrace(error, stackTrace);
    }
  }

  /// 从 backend 重新读取 settings 并应用到 controller 状态。
  ///
  /// 仅刷新事实源，不重放保存；刷新失败时保留原错误事实，因此这里吞掉刷新异常。
  Future<void> _refreshCanonicalSettings() async {
    try {
      final snapshot = await _api.readSettingsState();
      final latest = state.value;
      if (latest != null) {
        state = AsyncData(applySettingsState(latest, snapshot));
      }
    } catch (_) {
      // 刷新失败：原冲突错误仍是用户可见的事实，不在此覆盖。
    }
  }

  StudioFailure _settingsNotReady() => const StudioFailure(
    code: StudioFailureCode.notInitialized,
    message: 'Settings are not available yet',
    retryable: false,
    correlationId: 'client-settings-not-ready',
  );

  Future<void> refreshModelCatalog(String providerId) async {
    final next = await _api.refreshModelCatalog(providerId);
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

  void retryInitialization() {
    FrbStudioApi.retryInitialization();
    ref.invalidateSelf();
  }

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

  /// 显式继续执行：与重试保存是**两个动作**。
  ///
  /// 必须先按同一代数重试保存成功，后端 typed 存储状态解除硬故障闩（`resumeRequired`
  /// 变 false）后此命令才会让下一轮模型/工具准入恢复。代数不匹配或仍有未上交批次时后端
  /// 会拒绝；不自动恢复、不从错误文本推断结果，调用方按返回值/typed 状态重试。
  Future<PersistenceQueueSnapshot> resumeThreadHistory(
    String threadId,
    int faultGeneration,
  ) => _api.resumeThreadHistory(threadId, faultGeneration);

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

  /// typed topic 帧入口：Baseline/Event 走同一领域 apply（revision 拒旧）；
  /// Lagged/Failure 保留旧数据，单域恢复由订阅注册表完成。
  void _handleTopicFrame(ProductTopicFrame frame) {
    final current = state.value;
    if (current == null) return;
    if (frame is ProductTopicLaggedFrame || frame is ProductTopicFailureFrame) {
      return;
    }
    final previousThreadId = current.selectedThreadId;
    var next = applyProductTopicFrame(current, frame);
    if (identical(next, current)) return;
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
      // 目录事件把选择移到别的 Thread：按“旧选中取消、不自动打开新 Thread”语义
      // 收束旧 Thread 状态 stream 与 root SessionCosts 租约。先提交新选择再释放，
      // 使连接状态 release cleanup 以新 selectedRoot 判定。迟到的旧帧按 generation
      // 拒绝；不建立新订阅。
      _subscribedThreadId = null;
      _threadCoordinator.switchThread(null);
      _releaseSessionCostsLease();
      return;
    }
    state = AsyncData(next);
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
        );
        if (reduced.resyncThreadId != null) {
          unawaited(_resyncThread(threadId, generation));
          return;
        }
        state = AsyncData(reduced.state);
        // 展开中的固定活动条随 typed 活动版本前进刷新（同一身份只保留一个在途请求）。
        if (update is ThreadActivityUpdate &&
            _activityDetailExpandedThread == threadId) {
          unawaited(_loadActivityDetail(threadId));
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
  // 目录是分页窗口：resync snapshot 的首页按基线语义合并（保留已加载更远页）；
  // 选择采纳 incoming 携带的显式解析结果（_resolveSelection 是唯一解析点）。
  next = next.copyWith(
    threadDirectory: next.threadDirectory.applyBaselinePage(
      ThreadDirectoryPage(
        threads: incoming.threadDirectory.threads,
        nextCursor: incoming.threadDirectory.nextCursor,
        revision: incoming.threadDirectory.revision,
      ),
      revision: incoming.threadDirectory.revision,
    ),
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
  next = applySessionCostsMerge(next, incoming);
  final incomingQueue = incoming.persistenceQueueState;
  if (incomingQueue != null) {
    next = applyPersistenceQueueState(next, incomingQueue);
  }
  final incomingProfiles = incoming.agentProfilesState;
  if (incomingProfiles != null) {
    next = applyAgentProfilesState(next, incomingProfiles);
  }
  for (final snapshot in incoming.skillsByProject.values) {
    next = applySkillsState(next, snapshot);
  }
  return next;
}

/// 全量快照携带的每个 root 费用状态逐条按 revision 合并（含显式清除）。
StudioState applySessionCostsMerge(StudioState current, StudioState incoming) {
  var next = current;
  for (final state in incoming.sessionCostsByRoot.values) {
    next = applySessionCostsState(next, state);
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
  // Product snapshots describe backend-owned state only; they do not carry
  // the local new-session draft. Keep drafts for projects that still exist in
  // the canonical directory while resolving any snapshot (topic baseline,
  // explicit reload, or project activation). Otherwise a settings round-trip
  // can silently replace a user's selected mode, workspace, or composer text
  // with defaults.
  final canonicalProjectIds = incoming.projects
      .map((project) => project.id)
      .toSet();
  final modeDrafts = {
    for (final entry in incoming.newThreadModeByProject.entries)
      if (canonicalProjectIds.contains(entry.key)) entry.key: entry.value,
  };
  final workspaceDrafts = {
    for (final entry in incoming.newThreadWorkspaceModeByProject.entries)
      if (canonicalProjectIds.contains(entry.key)) entry.key: entry.value,
  };
  final composerDrafts = {
    for (final entry in incoming.newThreadComposerByProject.entries)
      if (canonicalProjectIds.contains(entry.key)) entry.key: entry.value,
  };
  if (previous != null) {
    for (final entry in previous.newThreadModeByProject.entries) {
      if (canonicalProjectIds.contains(entry.key)) {
        modeDrafts[entry.key] = entry.value;
      }
    }
    for (final entry in previous.newThreadWorkspaceModeByProject.entries) {
      if (canonicalProjectIds.contains(entry.key)) {
        workspaceDrafts[entry.key] = entry.value;
      }
    }
    for (final entry in previous.newThreadComposerByProject.entries) {
      if (canonicalProjectIds.contains(entry.key)) {
        composerDrafts[entry.key] = entry.value;
      }
    }
  }
  return incoming.copyWith(
    selectedProjectId: projectId,
    selectedThreadId: threadId,
    newThreadModeByProject: modeDrafts,
    newThreadWorkspaceModeByProject: workspaceDrafts,
    newThreadComposerByProject: composerDrafts,
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
    revision: current.threadDirectory.revision,
    upserted: const [],
    removed: result.removedThreadIds,
  );
  final nextRoot = result.nextRoot;
  if (nextRoot != null &&
      !next.threadDirectory.threads.any((thread) => thread.id == nextRoot.id)) {
    final threads = [...next.threadDirectory.threads, nextRoot]
      ..sort(StudioThread.compareDirectoryOrder);
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
