import 'dart:async';

import 'package:flutter/foundation.dart' show visibleForTesting;
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../../domain/models/studio_models.dart';
import '../../shared/studio_driver_state.dart';
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
  final Set<String> _archivingThreadIds = {};

  StudioApi get _api => ref.read(studioApiProvider);

  @override
  Future<StudioState> build() async {
    _productCoordinator = ProductStreamCoordinator(_api, _handleProductEvent);
    _threadCoordinator = ThreadStreamCoordinator(
      _api,
      _handleThreadFrame,
      _markThreadDisconnected,
    );
    ref.onDispose(() {
      unawaited(_productCoordinator.dispose());
      unawaited(_threadCoordinator.dispose());
    });
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
    unawaited(
      Future<void>.microtask(
        () => _subscribeThread(bootstrapped.selectedThreadId),
      ),
    );
    _activateStartupProject(bootstrapped);
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
    final project = await _api.openProject(path);
    await _api.activateProject(project.id);
    await _reloadProductState(selection: _ProjectDefaultSelection(project.id));
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

  Future<void> archiveThread(String threadId) async {
    final current = state.value;
    final thread = current?.threads
        .where((candidate) => candidate.id == threadId && candidate.isRoot)
        .firstOrNull;
    if (current == null ||
        thread == null ||
        current.recoveryIssue(
              scope: RecoveryIssueScope.thread,
              threadId: threadId,
            ) !=
            null ||
        (current.isBusy && current.selectedRootThread?.id == threadId)) {
      return;
    }
    if (!_archivingThreadIds.add(threadId)) return;
    try {
      final result = await _api.archiveThread(threadId);
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest == null) return;
      final previousThreadId = latest.selectedThreadId;
      final next = _applyArchiveResult(latest, result);
      state = AsyncData(next);
      if (previousThreadId != next.selectedThreadId) {
        await _subscribeThread(next.selectedThreadId);
      }
    } finally {
      _archivingThreadIds.remove(threadId);
    }
  }

  Future<void> archiveProject(String projectId) async {
    final current = state.value;
    if (current == null ||
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

  Future<RecoveryCleanupPreview> previewProjectCleanup(String projectId) {
    return _api.previewProjectCleanup(projectId);
  }

  Future<void> cleanupProject(String projectId, String expectedRevision) async {
    final current = state.value;
    await _api.cleanupProject(projectId, expectedRevision);
    await _reloadProductState(
      selection: current?.selectedProjectId == projectId
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
        current.selectedThreadId == threadId ||
        !current.threads.any((thread) => thread.id == threadId) ||
        current.recoveryIssue(
              scope: RecoveryIssueScope.thread,
              threadId: threadId,
            ) !=
            null) {
      return;
    }
    state = AsyncData(
      _withWorkspaceUi(
        current.copyWith(selectedThreadId: threadId),
        threadId,
        (ui) => ui.copyWith(syncState: AgentWorkspaceSyncState.loading),
      ),
    );
    await _subscribeThread(threadId);
  }

  Future<void> _subscribeThread(String? threadId) async {
    final generation = _threadCoordinator.switchThread(threadId);
    if (!ref.mounted || threadId == null) return;
    final current = state.value;
    if (current == null || current.selectedThreadId != threadId) return;
    state = AsyncData(
      _withWorkspaceUi(
        current,
        threadId,
        (ui) => ui.copyWith(subscriptionGeneration: generation),
      ),
    );
  }

  Future<void> loadOlderHistory(String threadId) async {
    final current = state.value;
    if (current == null) return;
    final workspace = current.workspacesByThread[threadId];
    final history = _workspaceUi(current, threadId).history;
    // 回源锚点从窗口首条内容派生（服务器 cursor 即 Turn id 的 before 语义）；
    // 窗口为空时不存在可回源的更旧历史。
    final anchor = workspace?.items.firstOrNull?.turnId;
    if (workspace == null || anchor == null) return;
    if (!history.hasOlder || history.isLoading) return;
    final epoch = history.epoch;
    state = AsyncData(
      _withWorkspaceUi(
        current,
        threadId,
        (ui) => ui.copyWith(
          history: ThreadHistoryWindow(
            hasOlder: ui.history.hasOlder,
            isLoading: true,
            epoch: epoch,
            errorMessage: null,
          ),
        ),
      ),
    );
    try {
      final page = await _api.listThreadTurns(threadId, cursor: anchor);
      if (!ref.mounted) return;
      final latest = state.value;
      // 窗口代际已变（快照重建/线程切换重订）：本次响应属于旧窗口，整体丢弃。
      if (latest == null ||
          _workspaceUi(latest, threadId).history.epoch != epoch) {
        return;
      }
      state = AsyncData(applyThreadHistoryPage(latest, threadId, page));
    } catch (error) {
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest == null ||
          _workspaceUi(latest, threadId).history.epoch != epoch) {
        return;
      }
      state = AsyncData(
        _withWorkspaceUi(
          latest,
          threadId,
          (ui) => ui.copyWith(
            history: ThreadHistoryWindow(
              hasOlder: ui.history.hasOlder,
              isLoading: false,
              epoch: epoch,
              errorMessage: error.toString(),
            ),
          ),
        ),
      );
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
      ref.read(directoryLoadErrorProvider.notifier).state = error.toString();
    }
  }

  void updateComposer(String threadId, String value) {
    final current = state.value;
    if (current == null || current.selectedThreadId != threadId) return;
    state = AsyncData(
      _withWorkspaceUi(
        current,
        threadId,
        (ui) => ui.copyWith(composer: ui.composer.updateDraft(value)),
      ),
    );
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

  Future<void> submitNewThreadComposer() async {
    final current = state.value;
    final projectId = current?.selectedProjectId;
    final composer =
        current?.newThreadComposer ?? const ComposerThreadState.idle();
    final prompt = composer.draft.trim();
    if (current == null ||
        projectId == null ||
        current.selectedThreadId != null ||
        current.recoveryIssue(
              scope: RecoveryIssueScope.project,
              projectId: projectId,
            ) !=
            null ||
        prompt.isEmpty ||
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
      result = await _api.startNewThread(projectId, prompt, const []);
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
        active.phase == ComposerSubmissionPhase.submitting &&
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
    final current = state.value;
    final composer = current == null
        ? const ComposerThreadState.idle()
        : _workspaceUi(current, threadId).composer;
    final prompt = composer.draft.trim();
    if (current == null ||
        current.selectedThreadId != threadId ||
        prompt.isEmpty ||
        composer.isSubmissionPending) {
      return;
    }
    final workspace = current.workspacesByThread[threadId];
    final submit = workspace?.activeTurn?.state.isBusy == true
        ? () => _api.steerTurn(threadId, prompt, const [])
        : () => _api.startTurn(threadId, prompt, const []);
    await _submitThreadInput(
      current,
      threadId,
      composer.beginSubmission(),
      submit,
    );
  }

  Future<TaskRecoveryPreview> previewTaskRecovery(String rootThreadId) {
    return _api.previewTaskRecovery(rootThreadId);
  }

  Future<TaskRecoveryResult> applyTaskRecovery(
    TaskRecoveryRequest request,
  ) async {
    final result = await _api.applyTaskRecovery(request);
    await _refreshRecoveredHistory(result.targetThreadId);
    return result;
  }

  Future<void> _refreshRecoveredHistory(String threadId) async {
    try {
      final page = await _api.listThreadTurns(threadId);
      if (!ref.mounted) return;
      final current = state.value;
      if (current == null || current.workspacesByThread[threadId] == null) {
        return;
      }
      state = AsyncData(applyRecoveredDispositions(current, threadId, page));
    } on Object {
      // Recovery is already durable; a later history load will project labels.
    }
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
    state = AsyncData(_reconcileComposer(next, threadId));
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

  Future<void> setThreadMode(StudioMode mode) async {
    final current = state.value;
    final thread = current?.selectedThread;
    if (current == null ||
        thread == null ||
        !thread.isRoot ||
        thread.mode == mode ||
        thread.status != 'idle' ||
        current.runtime.hasActiveTask) {
      return;
    }
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
      effort: effort ?? defaultEffortForModel(current, providerId, model),
    );
    final latest = state.value;
    if (latest != null) state = AsyncData(applySettingsState(latest, next));
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
    await _saveConfigSettings(
      (revision) => _api.saveGeneralSettings(revision, command),
    );
  }

  Future<void> saveWebSearchSettings(WebSearchSettingsCommand command) async {
    await _saveConfigSettings(
      (revision) => _api.saveWebSearchSettings(revision, command),
    );
  }

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

  Future<RecoveryCleanupPreview> previewRecoveryIssueCleanup(String issueId) {
    return _api.previewRecoveryIssueCleanup(issueId);
  }

  Future<void> cleanupRecoveryIssue(
    String issueId,
    String expectedRevision,
  ) async {
    await _api.cleanupRecoveryIssue(issueId, expectedRevision);
    await _reloadProductState(selection: const _PreserveSelection());
  }

  Future<void> retryRecoveryIssue(String issueId) async {
    final current = state.value;
    if (current == null) return;
    final issue = current.recoveryIssue(id: issueId);
    if (issue == null || !issue.canRetry) return;
    await _api.retryRecoveryIssue(issueId);
    await _reloadProductState(
      selection: issue.threadId == null
          ? _ProjectDefaultSelection(
              issue.projectId ?? current.selectedProjectId,
            )
          : _ExactThreadSelection(
              projectId: issue.projectId ?? current.selectedProjectId,
              threadId: issue.threadId!,
            ),
    );
  }

  void retryInitialization() => ref.invalidateSelf();

  Future<void> resolveActiveInteraction(
    String threadId,
    InteractionResolutionCommand resolution,
  ) async {
    final current = state.value;
    final workspace = current?.workspacesByThread[threadId];
    final interaction = current?.activeInteraction;
    if (current == null ||
        current.selectedThreadId != threadId ||
        workspace == null ||
        interaction == null) {
      return;
    }
    await _api.respondInteraction(interaction.id, resolution);
    if (!ref.mounted) return;
    final latest = state.value;
    final active = latest?.workspacesByThread[threadId];
    if (latest == null || active == null) return;
    state = AsyncData(
      latest.copyWith(
        workspacesByThread: {
          ...latest.workspacesByThread,
          threadId: active.copyWith(
            interactions: active.interactions
                .where((candidate) => candidate.id != interaction.id)
                .toList(),
          ),
        },
      ),
    );
  }

  void _handleProductEvent(Object event) {
    final current = state.value;
    if (current == null || event is! StudioBridgeEvent) return;
    if (event.payload is StalePayload) {
      unawaited(_reloadProductState());
      return;
    }
    final previousThreadId = current.selectedThreadId;
    final next = reduceStudioEvent(current, event).state;
    state = AsyncData(next);
    if (previousThreadId != next.selectedThreadId) {
      unawaited(_subscribeThread(next.selectedThreadId));
    }
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
    final current = state.value;
    if (current == null ||
        generation != _threadCoordinator.generation ||
        current.selectedThreadId != threadId ||
        _workspaceUi(current, threadId).subscriptionGeneration != generation) {
      return;
    }
    switch (frame) {
      case ThreadSnapshotFrame(:final workspace, :final historyCursor):
        final next = _reconcileComposer(
          applyThreadSnapshot(current, workspace, historyCursor: historyCursor),
          threadId,
        );
        state = AsyncData(next);
      case ThreadNotificationFrame(:final revision, :final update):
        if (const bool.fromEnvironment('PURE_STUDIO_DRIVER')) {
          if (update case ThreadTurnUpdate(:final turn)) {
            StudioDriverState.publishTurn(turn);
          }
        }
        final reduced = applyThreadUpdate(
          current,
          threadId: threadId,
          revision: revision,
          update: update,
        );
        if (reduced.resyncThreadId != null) {
          unawaited(_resyncThread(threadId, generation));
          return;
        }
        final observedTurn = switch (update) {
          ThreadTurnUpdate(:final turn) => turn,
          _ => null,
        };
        state = AsyncData(
          _reconcileComposer(
            reduced.state,
            threadId,
            observedTurn: observedTurn,
          ),
        );
      case ThreadResyncRequiredFrame():
        unawaited(_resyncThread(threadId, generation));
    }
  }

  Future<void> _resyncThread(String threadId, int generation) async {
    _markThreadDisconnected(threadId, generation);
    try {
      final snapshot = await _api.readThreadSnapshot(threadId);
      final current = state.value;
      if (current == null ||
          generation != _threadCoordinator.generation ||
          current.selectedThreadId != threadId) {
        return;
      }
      state = AsyncData(
        _reconcileComposer(
          applyThreadSnapshot(
            current,
            snapshot.workspace,
            historyCursor: snapshot.historyCursor,
          ),
          threadId,
        ),
      );
    } on Object {
      // The scheduled subscription retry remains the recovery path.
    }
  }

  void _markThreadDisconnected(String threadId, int generation) {
    final current = state.value;
    if (current == null ||
        generation != _threadCoordinator.generation ||
        current.selectedThreadId != threadId) {
      return;
    }
    state = AsyncData(
      _withWorkspaceUi(
        current,
        threadId,
        (ui) => ui.copyWith(syncState: AgentWorkspaceSyncState.reconnecting),
      ),
    );
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
    final next = current == null
        ? incoming
        : _mergeProductSnapshots(
            current,
            incoming,
          ).copyWith(providerCatalog: current.providerCatalog);
    state = AsyncData(next);
    if (previousThreadId != next.selectedThreadId) {
      await _subscribeThread(next.selectedThreadId);
    }
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
  next = applyTaskDirectory(next, incoming.taskDirectory);
  next = applyAgentDirectory(next, incoming.agentDirectory);
  next = applySettingsState(next, incoming.settingsState);
  next = applyRecoveryState(next, incoming.recoveryState);
  next = applyMcpState(next, incoming.mcpState);
  next = applyLspState(next, incoming.lspState);
  next = applyProviderUsageState(next, incoming.providerUsageState);
  next = applyUpdaterState(next, incoming.updaterState);
  for (final snapshot in incoming.skillsByProject.values) {
    next = applySkillsState(next, snapshot);
  }
  return next;
}

StudioState _reconcileComposer(
  StudioState state,
  String threadId, {
  StudioTurnView? observedTurn,
}) {
  final workspace = state.workspacesByThread[threadId];
  if (workspace == null) return state;
  return _withWorkspaceUi(
    state,
    threadId,
    (ui) => ui.copyWith(
      composer: ui.composer.observeTurn(observedTurn ?? workspace.activeTurn),
    ),
  );
}

WorkspaceUiState _workspaceUi(StudioState state, String threadId) {
  return state.workspaceUiByThread[threadId] ?? const WorkspaceUiState();
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
