import 'dart:async';

import '../../../models.dart';
import '../../../studio_ports.dart';
import '../../../anywork_client.dart'
    show
        ReadModel,
        OperationHandle,
        OperationState,
        Observation,
        ResourceKind,
        ResourcePermit;
import '../anywork_client.dart';
import '../stores/domain_sync.dart';
import '../thread_session_state.dart';
import 'command_lane.dart';

class ThreadCommands extends ReadModel<ThreadCommandBoard> {
  ThreadCommands(this._client) : super(const ThreadCommandBoard());
  final AnyworkClient _client;
  final CommandLane _lane = CommandLane();
  final CommandLane _stopLane = CommandLane();
  final Set<Observation> _operationObservations = {};
  bool _closed = false;
  Future<void>? _closing;

  Future<void> close() {
    final existing = _closing;
    if (existing != null) return existing;
    _closed = true;
    _lane.seal();
    _stopLane.seal();
    return _closing = Future.wait<void>([_lane.join(), _stopLane.join()])
        .then((_) {
          for (final observation in List.of(_operationObservations)) {
            observation.cancel();
          }
          _operationObservations.clear();
        });
  }

  void _ensureOpen() {
    if (_closed || _client.isClosed)
      throw StateError('Thread commands are closed');
  }

  Future<T> _track<T>(Future<T> Function() action) {
    _ensureOpen();
    return _lane.run(action);
  }

  Future<void> setThreadModelRoute({
    String? threadId,
    required String providerId,
    required String model,
    String? effort,
  }) {
    _ensureOpen();
    final targetThreadId = threadId ?? _client.selection.state.selectedThreadId;
    if (targetThreadId == null) return Future<void>.value();
    final operation = _client.mutations.begin(
      target: 'thread-route:$targetThreadId',
    );
    _client.settings.beginCommand(
      'thread-route:$targetThreadId',
      CommandTarget(providerId: providerId, model: model, effort: effort),
    );
    _watchObservation(
      operation,
      () => _client.settings.clearCommand('thread-route:$targetThreadId'),
    );
    return _track(
      () => _client.settingsCommands.enqueueSettingsWrite(
        () => _applyThreadModelRoute(
          operation: operation,
          threadId: targetThreadId,
          providerId: providerId,
          model: model,
          effort: effort,
        ),
      ),
    );
  }

  Future<void> setThreadMode(ThreadModeId mode) {
    _ensureOpen();
    // 点击时冻结目标：排队期间切换会话不改变命令目标。
    final threadId = _client.selection.state.selectedThreadId;
    if (threadId == null) return Future<void>.value();
    final key = 'mode:$threadId';
    final operation = _client.mutations.begin(target: key);
    _begin(key, label: mode.id);
    _watchObservation(operation, () => _clear(key));
    return _lane.enqueue(() async {
      final thread = _directoryThread(threadId);
      final runtime = _client.session(threadId).state.runtime;
      if (thread == null ||
          !thread.isRoot ||
          thread.status != ThreadStatusView.idle ||
          (runtime?.hasActiveWorkflow ?? false) ||
          _blocksThread(threadId)) {
        operation.reject(StateError('Thread mode change is unavailable'));
        if (_isLatest(operation)) _clear(key);
        return;
      }
      try {
        final response = await _api.setThreadMode(
          operationId: operation.id,
          threadId: threadId,
          mode: mode,
        );
        operation.accept(response.receipt.toCommitReceipt());
      } catch (error) {
        _failOperation(operation, error);
        if (_isLatest(operation)) _fail(key, threadId, error);
        rethrow;
      }
      // 命令后不显式重读：mode 由目录增量交付、mode 默认 route 由 Thread 有序流
      // 交付（design/25 §25.5）。补偿式重读会让同一事实多出第二条合并路径，并让
      // 「帧没到」这种缺陷在验收里不可见。
    });
  }

  /// 草稿仍属于后续 route 层。这里只接受已经组装好的输入，不写 composer canonical。
  Future<SubmitPromptReceipt> submitComposer(
    String threadId,
    StudioPromptInput input, {
    required int expectedModelRouteRevision,
  }) {
    _ensureOpen();
    return _lane.enqueue(() async {
      if (input.text.trim().isEmpty && input.attachmentDraftIds.isEmpty) {
        throw ArgumentError('Prompt cannot be empty');
      }
      if (_blocksThread(threadId))
        throw StateError('Thread access is blocked by recovery');
      final key = 'submit:$threadId';
      _begin(key, label: threadId);
      try {
        final receipt = await _api.submitPrompt(
          threadId,
          input,
          expectedModelRouteRevision: expectedModelRouteRevision,
        );
        _clear(key);
        return receipt;
      } catch (error) {
        _fail(key, threadId, error);
        rethrow;
      }
    });
  }

  /// 解析当前会话的 pending interaction。
  ///
  /// 响应只确认命令；canonical interaction 帧仅由 Thread 有序流写入，
  /// 不在本地删除或改写 interaction（design/25 §25.5）。
  Future<void> resolveActiveInteraction(
    String threadId,
    String interactionId,
    InteractionResolutionCommand resolution,
  ) {
    _ensureOpen();
    final key = 'interaction:$threadId';
    final operation = _client.mutations.begin(target: key);
    final displayed = _client
        .session(threadId)
        .state
        .interactions
        .where((item) => item.id == interactionId)
        .firstOrNull;
    final turnId = displayed?.turnId;
    _begin(key, label: interactionId);
    try {
      _watchIdentity(
        operation,
        threadId,
        (state) =>
            state.hasSnapshot &&
            !state.interactions.any((item) => item.id == interactionId),
        () => _clear(key),
      );
    } catch (error) {
      operation.reject(error);
      if (_isLatest(operation)) _fail(key, interactionId, error);
      rethrow;
    }
    return _lane.enqueue(() async {
      final active = [..._client.session(threadId).state.interactions]
        ..sort(
          (left, right) =>
              interactionPriority(left.kind)
                  .compareTo(interactionPriority(right.kind)),
        );
      if (turnId == null ||
          active.firstOrNull?.id != interactionId ||
          active.firstOrNull?.turnId != turnId) {
        final conflict = const StudioFailure(
          code: StudioFailureCode.conflict,
          message: 'The displayed interaction is no longer current',
          retryable: false,
          correlationId: 'client-interaction-conflict',
        );
        operation.reject(conflict);
        if (_isLatest(operation)) _fail(key, interactionId, conflict);
        throw conflict;
      }
      try {
        final response = await _api.respondInteraction(
          interactionId,
          resolution,
          operationId: operation.id,
        );
        if (response.interaction.id != interactionId ||
            response.interaction.threadId != threadId ||
            response.interaction.turnId != turnId ||
            !response.receipt.requiresIdentityObservation) {
          throw StateError(
            'Interaction receipt did not match the stable target',
          );
        }
        operation.accept(response.receipt.toCommitReceipt());
      } catch (error) {
        _failOperation(operation, error);
        if (_isLatest(operation)) _fail(key, interactionId, error);
        rethrow;
      }
    });
  }

  Future<StartNewThreadResult?> submitNewThread({
    required String projectId,
    required StudioPromptInput input,
    required ThreadModeId mode,
    required ModeModelRouteView route,
    String? workspaceMode,
  }) {
    _ensureOpen();
    return _lane.enqueue(() async {
      if (input.text.trim().isEmpty && input.attachmentDraftIds.isEmpty) {
        return null;
      }
      if (_blocksProject(projectId)) return null;
      final key = 'new-thread:$projectId';
      _begin(key, label: projectId);
      try {
        final result = await _api.startNewThread(
          projectId,
          input,
          mode,
          route: route,
          workspaceMode: workspaceMode,
        );
        if (!_client.isClosed) {
          // 目录条目由 owner 的目录增量交付（commit_directory 已在命令内发布）；
          // 命令成功后不重读聚合状态（design/25 §25.5）。
          _client.selection.selectThread(
            projectId: projectId,
            threadId: result.thread.id,
          );
        }
        _clear(key);
        return result;
      } catch (error) {
        _fail(key, projectId, error);
        rethrow;
      }
    });
  }

  /// 停止是独立的可取消控制入口：不排在归档/创建/长查询后面，也不按「当前是否选中」
  /// 过滤目标——调用时冻结的 threadId + turnId 就是命令目标。
  Future<void> stop(String threadId) {
    _ensureOpen();
    final turn = _client.session(threadId).state.activeTurn;
    if (turn == null || !turn.state.isBusy) return Future<void>.value();
    final turnId = turn.turnId;
    final key = 'stop:$threadId';
    final operation = _client.mutations.begin(target: key);
    _begin(key, label: turnId);
    try {
      _watchIdentity(operation, threadId, (state) {
        if (!state.hasSnapshot) return false;
        final active = state.activeTurn;
        if (active != null && active.turnId == turnId)
          return !active.state.isBusy;
        // The watched canonical owner previously held this busy Turn. A later
        // owner frame with no such active identity confirms that it ended.
        return true;
      }, () => _clear(key));
    } catch (error) {
      operation.reject(error);
      if (_isLatest(operation)) _fail(key, turnId, error);
      rethrow;
    }
    return _stopLane.enqueue(() async {
      final latest = _client.session(threadId).state.activeTurn;
      if (latest == null || latest.turnId != turnId || !latest.state.isBusy) {
        operation.reject(StateError('The targeted Turn has already ended'));
        if (_isLatest(operation)) _clear(key);
        return;
      }
      try {
        final response = await _api.interruptTurn(
          threadId,
          turnId,
          operationId: operation.id,
        );
        if (response.threadId != threadId ||
            response.turnId != turnId ||
            !response.receipt.requiresIdentityObservation) {
          throw StateError('Interrupt receipt did not match the stable target');
        }
        operation.accept(response.receipt.toCommitReceipt());
      } catch (error) {
        _failOperation(operation, error);
        if (_isLatest(operation)) _fail(key, turnId, error);
        rethrow;
      }
    });
  }

  Future<StudioThread?> renameThread(String threadId, String title) {
    _ensureOpen();
    final key = 'rename:$threadId';
    final operation = _client.mutations.begin(target: key);
    _begin(key, label: title);
    _watchObservation(operation, () => _clear(key));
    return _lane.enqueue(() async {
      final thread = _directoryThread(threadId);
      if (thread == null || !thread.isRoot || _blocksThread(threadId)) {
        operation.reject(StateError('Thread rename is unavailable'));
        if (_isLatest(operation)) _clear(key);
        return null;
      }
      try {
        final response = await _api.renameThread(
          threadId,
          title,
          operationId: operation.id,
        );
        operation.accept(response.receipt.toCommitReceipt());
        // 目录增量已在命令内发布（commit_directory），不重读聚合状态。
        return response.thread;
      } catch (error) {
        _failOperation(operation, error);
        if (_isLatest(operation)) _fail(key, threadId, error);
        rethrow;
      }
    });
  }

  Future<void> archiveThread(String threadId) {
    _ensureOpen();
    final key = 'archive:$threadId';
    final operation = _client.mutations.begin(target: key);
    _begin(key, label: threadId);
    _watchObservation(operation, () => _clear(key));
    return _lane.enqueue(() async {
      final thread = _directoryThread(threadId);
      if (thread == null || !thread.isRoot || _blocksThread(threadId)) {
        operation.reject(StateError('Thread archive is unavailable'));
        if (_isLatest(operation)) _clear(key);
        return;
      }
      try {
        final response = await _api.archiveThread(
          threadId,
          operationId: operation.id,
        );
        operation.accept(response.receipt.toCommitReceipt());
        final result = response.result;
        if (!_client.isClosed) {
          for (final id in result.removedThreadIds) {
            _client.releaseSession(id);
          }
          final selected = _client.selection.state.selectedThreadId;
          if (selected != null && result.removedThreadIds.contains(selected)) {
            _client.selection.replaceRemovedThread(result.nextRoot?.id);
          }
        }
      } catch (error) {
        _failOperation(operation, error);
        if (_isLatest(operation)) _fail(key, threadId, error);
        rethrow;
      }
    });
  }

  Future<void> loadOlderHistory(String threadId) =>
      _track(() => _loadWindow(threadId, TimelineDirection.older));

  Future<void> loadNewerHistory(String threadId) =>
      _track(() => _loadWindow(threadId, TimelineDirection.newer));

  /// 跳回实时底部：清锚点后聚焦权威窗口的 Latest；Reset/Patch 仍由内容泵交付。
  Future<void> jumpToLatest(String threadId) =>
      _track(() => _jumpToLatest(threadId));

  Future<void> _jumpToLatest(String threadId) async {
    final store = _client.session(threadId);
    store.jumpToLatest();
    final request = store.requestViewFocus(null, force: true)!;
    try {
      final window = await store.chatView();
      if (window == null || !store.isViewFocusCurrent(request)) {
        store.settleViewFocus(request, succeeded: false);
        return;
      }
      await window.focus(null);
      store.settleViewFocus(request, succeeded: true);
    } catch (error) {
      if (store.isViewFocusCurrent(request)) {
        store.settleViewFocus(request, succeeded: false);
        store.setHistoryError(TimelineDirection.newer, error.toString());
      }
    }
  }

  /// 写入阅读锚点；只有带定位动作的锚点才允许改变后端窗口焦点。
  ///
  /// 普通窗口内滚动只更新锚点：后端焦点不变，窗口也不移动。此前用「上一次锚点」去重，
  /// 于是每换一个顶行都会 focus 一次，而后端 focus 变化会让内容流发 Reset——滚动被放大成
  /// 整窗重载。去重因此只依据已确认焦点与同一目标的在途请求；失败后同一目标仍可重试。
  Future<void> updateTimelineAnchor(
    String threadId,
    TimelineAnchor anchor, {
    TimelineViewAction? viewAction,
  }) => _track(
    () => _updateTimelineAnchor(threadId, anchor, viewAction: viewAction),
  );

  Future<void> _updateTimelineAnchor(
    String threadId,
    TimelineAnchor anchor, {
    TimelineViewAction? viewAction,
  }) async {
    final store = _client.session(threadId);
    store.setTimelineAnchor(anchor);
    if (viewAction == null ||
        anchor.readingIntent != TimelineReadingIntent.browseHistory) {
      return;
    }
    // 已确认焦点就是同一个条目时，这一次定位动作不产生新的后端请求。
    final request = store.requestViewFocus(anchor.itemId);
    if (request == null) return;
    try {
      final window = await store.chatView();
      if (window == null || !store.isViewFocusCurrent(request)) {
        store.settleViewFocus(request, succeeded: false);
        return;
      }
      await window.focus(anchor.itemId);
      store.settleViewFocus(request, succeeded: true);
    } catch (_) {
      // 聚焦失败保留当前窗口与锚点；下一次同目标定位动作会重试。
      store.settleViewFocus(request, succeeded: false);
    }
  }

  /// 可见条目正文补齐：对窗口内仍是预览的身份按 identity 展开。
  Future<void> ensureItemBodies(String threadId, Iterable<String> itemIds) {
    final frozenIds = List<String>.of(itemIds);
    return _track(() => _ensureItemBodies(threadId, frozenIds));
  }

  Future<void> _ensureItemBodies(
    String threadId,
    Iterable<String> itemIds,
  ) async {
    for (final itemId in itemIds) {
      await _completeItemBody(threadId, itemId);
    }
  }

  /// 显式重试一次按身份的正文补齐（只对窗口内仍是预览的身份生效）。
  Future<void> loadItemBody(String threadId, String itemId) =>
      _track(() => _completeItemBody(threadId, itemId, retry: true));

  Future<void> _completeItemBody(
    String threadId,
    String itemId, {
    bool retry = false,
  }) async {
    final session = _client.session(threadId).state;
    final history = session.history;
    if (!history.previewedItemIds.contains(itemId) ||
        history.loadingItemIds.contains(itemId)) {
      return;
    }
    if (!retry && history.itemBodyErrors.containsKey(itemId)) return;
    final store = _client.session(threadId);
    final request = store.beginItemBodyLoad(itemId);
    if (request == null) return;
    try {
      final window = await store.chatView();
      if (!store.isItemBodyRequestCurrent(request)) return;
      if (window == null) {
        store.failItemBodyLoad(request, 'Chat window is unavailable');
        return;
      }
      final receipt = await window.expandItem(itemId);
      if (!store.isItemBodyRequestCurrent(request)) return;
      if (!receipt.retained) store.markItemBodyPending(request);
      // The same content pump delivers the full body. Its reducer clears loading;
      // a receipt never installs another body baseline.
    } catch (error) {
      store.failItemBodyLoad(request, error.toString());
    }
  }

  /// 窗口内按方向翻一页：控制操作取回执并等待内容泵交付对应帧后完成。
  Future<void> _loadWindow(String threadId, TimelineDirection direction) async {
    final session = _client.session(threadId).state;
    final history = session.history;
    final older = direction == TimelineDirection.older;
    final canPage = older ? history.hasOlder : history.hasNewer;
    if (!session.hasSnapshot || history.isLoading || !canPage) return;
    final store = _client.session(threadId);
    final token = store.beginHistoryOp(direction);
    if (token == null) return;
    try {
      final window = await store.chatView();
      if (window == null || !!_client.isClosed) {
        store.failHistoryOp(token, 'Chat window is unavailable');
        return;
      }
      final latest = _client.session(threadId).state.history;
      final stillCanPage = older ? latest.hasOlder : latest.hasNewer;
      if (!stillCanPage) {
        store.observeHistoryOp(
          token,
          StudioChatReceipt(
            operationId: token,
            version: _client.session(threadId).state.chatVersion ?? 0,
            retained: true,
            disposition: StudioChatDisposition.noChange,
          ),
        );
        return;
      }
      final receipt = await window.load(direction);
      if (!_client.isClosed) {
        store.observeHistoryOp(token, receipt);
        await store.waitForHistoryOp(token);
      }
    } catch (error) {
      if (!_client.isClosed) store.failHistoryOp(token, error.toString());
    }
  }

  Future<void> _applyThreadModelRoute({
    required OperationHandle operation,
    required String threadId,
    required String providerId,
    required String model,
    String? effort,
  }) async {
    final key = 'thread-route:$threadId';
    final target = CommandTarget(
      providerId: providerId,
      model: model,
      effort: effort,
    );
    final settings = _client.settings;
    try {
      if (!_client.settings.state.configLoaded) {
        final refreshed = await _client.settingsCommands.refreshCanonical();
        if (refreshed == null || !_client.settings.state.configLoaded) {
          throw StateError('Settings are not available yet.');
        }
      }
      final response = await retryRevisionConflict(
        () => _requestThreadModelRoute(
          operationId: operation.id,
          threadId: threadId,
          providerId: providerId,
          model: model,
          effort: effort,
        ),
        refresh: () => _refreshRouteOwners(threadId),
      );
      operation.accept(response.receipt.toCommitReceipt());
      if (!!_client.isClosed) return;
      settings.merge(response.settings);
      if (_isLatest(operation)) {
        _rememberWarning(threadId, response.warning);
      }
    } catch (error) {
      _failOperation(operation, error);
      if (!_client.isClosed && _isLatest(operation)) {
        settings.failCommand(
          key,
          typedCommandFailure(error, correlationId: 'client-thread-route'),
          target: target,
        );
      }
      rethrow;
    }
  }

  /// 两次 revision 都在尝试前从各自 owner 读取。响应 runtime 不进入 store。
  Future<ThreadModelRouteCommandReply> _requestThreadModelRoute({
    required OperationId operationId,
    required String threadId,
    required String providerId,
    required String model,
    String? effort,
  }) async {
    final session = _client.session(threadId).state;
    final route = session.runtime?.modelRoute;
    final target = _findModel(providerId, model);
    if (!session.hasSnapshot || route == null || target == null) {
      throw StateError('Selected thread model route is unavailable.');
    }
    return _api.setThreadModelRoute(
      operationId: operationId,
      threadId: threadId,
      expectedModelRouteRevision: route.revision,
      expectedSettingsRevision: _client.settings.state.settingsRevision,
      providerId: providerId,
      model: model,
      effort: effort ?? target.reasoningEfforts.firstOrNull,
    );
  }

  Future<bool> _refreshRouteOwners(String threadId) async {
    final settings = await _client.settingsCommands.refreshCanonical();
    if (settings == null || !!_client.isClosed) return false;
    try {
      await _client.session(threadId).resyncRuntime();
    } catch (_) {
      // Only a fresh owner baseline permits another CAS attempt. Observation
      // failure or retired demand preserves the original known conflict.
      return false;
    }
    return !_client.isClosed;
  }

  ProviderModelView? _findModel(String providerId, String model) {
    final providers = _client.settings.state.canonical?.providers ?? const [];
    for (final provider in providers) {
      if (provider.id != providerId) continue;
      for (final candidate in provider.models) {
        if (candidate.slug == model) return candidate;
      }
    }
    return null;
  }

  StudioThread? _directoryThread(String threadId) {
    final directory = _client.threadDirectory.state;
    for (final thread in directory.window.threads) {
      if (thread.id == threadId) return thread;
    }
    for (final thread in directory.overlayThreads) {
      if (thread.id == threadId) return thread;
    }
    return null;
  }

  bool _blocksThread(String threadId) =>
      _blocks(scope: RecoveryIssueScope.thread, threadId: threadId);

  bool _blocksProject(String projectId) =>
      _blocks(scope: RecoveryIssueScope.project, projectId: projectId);

  bool _blocks({
    required RecoveryIssueScope scope,
    String? projectId,
    String? threadId,
  }) {
    final issues = _client.recovery.state.value?.values ?? const [];
    return issues.any(
      (issue) =>
          issue.blocksAccess &&
          issue.scope == scope &&
          (projectId == null || issue.projectId == projectId) &&
          (threadId == null || issue.threadId == threadId),
    );
  }

  void _rememberWarning(String threadId, String? warning) {
    final warnings = {...state.warnings};
    if (warning == null) {
      warnings.remove(threadId);
    } else {
      warnings[threadId] = warning;
    }
    if (_stringMapsEqual(warnings, state.warnings)) return;
    state = state.copyWith(warnings: warnings);
  }

  void _begin(String key, {required String label}) {
    _put(
      CommandStatus(
        key: key,
        phase: CommandPhase.pending,
        target: CommandTarget(label: label),
      ),
    );
  }

  void _clear(String key) {
    if (!state.commands.containsKey(key)) return;
    state = state.copyWith(commands: {...state.commands}..remove(key));
  }

  void _fail(String key, String id, Object error) {
    final existing = state.commands[key];
    _put(
      CommandStatus(
        key: key,
        phase: CommandPhase.failed,
        target: existing?.target ?? CommandTarget(label: id),
        error: typedCommandFailure(error, correlationId: 'client-thread'),
      ),
    );
  }

  void _put(CommandStatus status) {
    final commands = {...state.commands, status.key: status};
    if (commandMapsEqual(commands, state.commands)) return;
    state = state.copyWith(commands: commands);
  }

  StudioTransport get _api => _client.transport;

  bool _isLatest(OperationHandle operation) =>
      _client.mutations.latest(operation.state.value.target)?.id ==
      operation.id;

  void _watchObservation(
    OperationHandle operation,
    void Function() onObserved,
  ) {
    late final Observation observation;
    void merge(OperationState value) {
      if (value.observed && !_client.isClosed && _isLatest(operation))
        onObserved();
      if (value.observed || value.failurePhase != null) {
        observation.cancel();
        _operationObservations.remove(observation);
      }
    }

    final watched = operation.state.watchLatest(merge);
    observation = watched.observation;
    _operationObservations.add(observation);
    merge(watched.value);
  }

  void _watchIdentity(
    OperationHandle operation,
    String threadId,
    bool Function(ThreadSessionState) predicate,
    void Function() onObserved,
  ) {
    final permit = _client.budget.acquire(
      ResourceKind.observer,
      owner: 'command:${operation.id.value}',
    );
    final binding = _IdentityOperationObservation(permit);
    var confirmed = false;
    void completeIfAccepted() {
      final current = operation.state.value;
      if (binding.isCancelled ||
          !confirmed ||
          current.observed ||
          current.receipt?.requiresIdentityObservation != true)
        return;
      if (!current.effective) operation.markEffective();
      operation.markObserved();
    }

    void mergeOwner(ThreadSessionState next) {
      if (binding.isCancelled) return;
      if (predicate(next)) confirmed = true;
      completeIfAccepted();
    }

    void mergeOperation(OperationState next) {
      if (next.observed && !_client.isClosed && _isLatest(operation))
        onObserved();
      if (next.observed || next.failurePhase != null) {
        binding.cancel();
        _operationObservations.remove(binding);
      } else {
        completeIfAccepted();
      }
    }

    try {
      // Registration and the current value are one synchronous transaction.
      // A matching frame before the receipt is remembered by this binding;
      // the receipt still supplies its own authoritative disposition.
      final owner = _client.session(threadId).source.watchLatest(mergeOwner);
      binding.owner = owner.observation;
      final receipt = operation.state.watchLatest(mergeOperation);
      binding.operation = receipt.observation;
      _operationObservations.add(binding);
      mergeOwner(owner.value);
      mergeOperation(receipt.value);
    } catch (_) {
      binding.cancel();
      _operationObservations.remove(binding);
      rethrow;
    }
  }

  void _failOperation(OperationHandle operation, Object error) {
    if (operation.state.value.receipt != null) {
      operation.syncFailed(error);
    } else if (error is StudioFailure &&
        (switch (error.code) {
          StudioFailureCode.invalidArgument ||
          StudioFailureCode.notFound ||
          StudioFailureCode.busy ||
          StudioFailureCode.conflict ||
          StudioFailureCode.staleRevision ||
          StudioFailureCode.permissionDenied ||
          StudioFailureCode.overloaded => true,
          _ => false,
        })) {
      operation.reject(error);
    } else {
      operation.unknownOutcome(error);
    }
  }
}

class ThreadCommandBoard {
  const ThreadCommandBoard({
    this.commands = const {},
    this.warnings = const {},
  });

  final Map<String, CommandStatus> commands;
  final Map<String, String> warnings;

  ThreadCommandBoard copyWith({
    Map<String, CommandStatus>? commands,
    Map<String, String>? warnings,
  }) {
    return ThreadCommandBoard(
      commands: commands ?? this.commands,
      warnings: warnings ?? this.warnings,
    );
  }

  @override
  bool operator ==(Object other) =>
      identical(other, this) ||
      other is ThreadCommandBoard &&
          commandMapsEqual(commands, other.commands) &&
          _stringMapsEqual(warnings, other.warnings);

  @override
  int get hashCode =>
      Object.hash(commandMapHash(commands), _stringMapHash(warnings));
}

bool _stringMapsEqual(Map<String, String> left, Map<String, String> right) {
  if (identical(left, right)) return true;
  if (left.length != right.length) return false;
  for (final entry in left.entries) {
    if (right[entry.key] != entry.value) return false;
  }
  return true;
}

int _stringMapHash(Map<String, String> values) => Object.hashAll(
  values.entries.map((entry) => Object.hash(entry.key, entry.value)),
);

/// An identity predicate and its operation retention form one bounded observer.
final class _IdentityOperationObservation implements Observation {
  _IdentityOperationObservation(this.permit);
  final ResourcePermit permit;
  Observation? owner;
  Observation? operation;
  @override
  bool isCancelled = false;
  @override
  bool get isPaused => owner?.isPaused ?? false;
  @override
  void pause() {
    owner?.pause();
  }

  @override
  void resume() {
    owner?.resume();
  }

  @override
  void cancel() {
    if (isCancelled) return;
    isCancelled = true;
    owner?.cancel();
    operation?.cancel();
    permit.release();
  }
}
