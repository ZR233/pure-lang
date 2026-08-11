import 'dart:async';

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
  late ProductStreamCoordinator _productCoordinator;
  late ThreadStreamCoordinator _threadCoordinator;

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
    final bootstrapped = _attachProviderCatalog(
      await _api.bootstrap(),
      catalog,
    );
    _productCoordinator.start();
    unawaited(
      Future<void>.microtask(
        () => _subscribeThread(bootstrapped.selectedThreadId),
      ),
    );
    return bootstrapped;
  }

  Future<void> openProject(String path) async {
    await _adoptProductState(await _api.openProject(path));
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
    await _adoptProductState(await _api.selectProject(projectId));
  }

  Future<void> createThread() async {
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
    await _adoptProductState(await _api.createThread(projectId));
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
    await _adoptProductState(
      await _api.archiveThread(
        threadId,
        selectedThreadId: current.selectedThreadId,
      ),
    );
  }

  Future<void> archiveProject(String projectId) async {
    final current = state.value;
    if (current == null ||
        (current.isBusy && current.selectedProjectId == projectId)) {
      return;
    }
    await _adoptProductState(
      await _api.archiveProject(
        projectId,
        selectedProjectId: current.selectedProjectId,
      ),
    );
  }

  Future<RecoveryCleanupPreview> previewProjectCleanup(String projectId) {
    return _api.previewProjectCleanup(projectId);
  }

  Future<void> cleanupProject(String projectId, String expectedRevision) async {
    final current = state.value;
    if (current == null) return;
    await _adoptProductState(
      await _api.cleanupProject(
        projectId,
        expectedRevision,
        selectedProjectId: current.selectedProjectId,
      ),
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
    if (current == null || current.workspacesByThread[threadId] == null) return;
    final paging = _workspaceUi(current, threadId).history;
    if (paging.isLoading || (paging.isLoaded && !paging.hasMore)) return;
    state = AsyncData(
      _withWorkspaceUi(
        current,
        threadId,
        (ui) => ui.copyWith(
          history: ThreadHistoryPagingState(
            nextCursor: paging.nextCursor,
            hasMore: paging.hasMore,
            isLoading: true,
            isLoaded: paging.isLoaded,
          ),
        ),
      ),
    );
    try {
      final page = await _api.listThreadTurns(
        threadId,
        cursor: paging.isLoaded ? paging.nextCursor : null,
      );
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest == null) return;
      state = AsyncData(
        _withWorkspaceUi(
          mergeThreadHistoryPage(latest, threadId, page),
          threadId,
          (ui) => ui.copyWith(
            history: ThreadHistoryPagingState(
              nextCursor: page.nextCursor,
              hasMore: page.nextCursor != null,
              isLoading: false,
              isLoaded: true,
            ),
          ),
        ),
      );
    } catch (error) {
      if (!ref.mounted) return;
      final latest = state.value;
      if (latest == null) return;
      final active = _workspaceUi(latest, threadId).history;
      state = AsyncData(
        _withWorkspaceUi(
          latest,
          threadId,
          (ui) => ui.copyWith(
            history: ThreadHistoryPagingState(
              nextCursor: active.nextCursor,
              hasMore: active.hasMore,
              isLoading: false,
              isLoaded: active.isLoaded,
              errorMessage: error.toString(),
            ),
          ),
        ),
      );
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
      state = AsyncData(mergeThreadHistoryPage(current, threadId, page));
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
    state = AsyncData(
      _withWorkspaceUi(
        latest,
        threadId,
        (ui) => ui.copyWith(composer: accepted),
      ),
    );
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
    await _saveConfigSettings(() => _api.saveRuntimePermissionMode(mode));
  }

  Future<void> setThreadMode(StudioMode mode) async {
    final current = state.value;
    final thread = current?.selectedThread;
    if (current == null ||
        thread == null ||
        !thread.isRoot ||
        thread.mode == mode ||
        current.runtime.hasActiveTask) {
      return;
    }
    final next = await _api.setThreadMode(threadId: thread.id, mode: mode);
    if (!ref.mounted) return;
    final latest = state.value;
    if (latest == null) return;
    state = AsyncData(mergeStudioThreadState(latest, next, thread.id));
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
      roleKey: roleKey,
      providerId: providerId,
      model: model,
      effort: effort ?? defaultEffortForModel(current, providerId, model),
      selectedThreadId: current.selectedThreadId,
    );
    final latest = state.value;
    if (latest != null) state = AsyncData(mergeStudioConfigState(latest, next));
  }

  Future<void> saveProviderSettings(ProviderSettingsCommand command) async {
    await _saveConfigSettings(() => _api.saveProviderSettings(command));
  }

  Future<void> saveInstructionsSettings(
    InstructionsSettingsCommand command,
  ) async {
    await _saveConfigSettings(() => _api.saveInstructionsSettings(command));
  }

  Future<void> saveSkillsSettings(SkillsSettingsCommand command) async {
    await _saveConfigSettings(() => _api.saveSkillsSettings(command));
  }

  Future<void> saveMcpSettings(McpSettingsCommand command) async {
    await _saveConfigSettings(() => _api.saveMcpSettings(command));
  }

  Future<void> saveGeneralSettings(GeneralSettingsCommand command) async {
    await _saveConfigSettings(() => _api.saveGeneralSettings(command));
  }

  Future<void> saveWebSearchSettings(WebSearchSettingsCommand command) async {
    await _saveConfigSettings(() => _api.saveWebSearchSettings(command));
  }

  Future<void> _saveConfigSettings(
    Future<StudioState> Function() request,
  ) async {
    if (state.value == null) return;
    final next = await request();
    final latest = state.value;
    if (latest != null) state = AsyncData(mergeStudioConfigState(latest, next));
  }

  Future<void> refreshProviderUsages() async {
    final current = state.value;
    if (current == null) return;
    final usages = await _api.loadProviderUsages();
    final latest = state.value;
    if (latest != null) {
      state = AsyncData(latest.copyWith(providerUsages: usages));
    }
  }

  Future<List<String>> listDiscoveredSkills() async {
    final projectId = state.value?.selectedProjectId;
    return projectId == null ? const [] : _api.listDiscoveredSkills(projectId);
  }

  Future<RecoveryCleanupPreview> previewRecoveryIssueCleanup(String issueId) {
    return _api.previewRecoveryIssueCleanup(issueId);
  }

  Future<void> cleanupRecoveryIssue(
    String issueId,
    String expectedRevision,
  ) async {
    final current = state.value;
    if (current == null) return;
    await _adoptProductState(
      await _api.cleanupRecoveryIssue(
        issueId,
        expectedRevision,
        selectedProjectId: current.selectedProjectId,
        selectedThreadId: current.selectedThreadId,
      ),
    );
  }

  Future<void> retryRecoveryIssue(String issueId) async {
    final current = state.value;
    if (current == null) return;
    final issue = current.recoveryIssue(id: issueId);
    if (issue == null || !issue.canRetry) return;
    await _adoptProductState(
      await _api.retryRecoveryIssue(
        issueId,
        selectedProjectId: issue.projectId ?? current.selectedProjectId,
        selectedThreadId: issue.threadId ?? current.selectedThreadId,
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

  Future<void> _reloadProductState() async {
    try {
      await _adoptProductState(await _api.bootstrap());
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
      case ThreadSnapshotFrame(:final workspace):
        final next = _reconcileComposer(
          applyThreadSnapshot(current, workspace),
          threadId,
        );
        state = AsyncData(next);
        if (!_workspaceUi(next, threadId).history.isLoaded) {
          unawaited(loadOlderHistory(threadId));
        }
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
          _markThreadDisconnected(threadId, generation);
          return;
        }
        state = AsyncData(_reconcileComposer(reduced.state, threadId));
      case ThreadResyncRequiredFrame():
        _markThreadDisconnected(threadId, generation);
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
    var next = incoming;
    if (current != null) {
      next = _attachProviderCatalog(next, current.providerCatalog);
      final knownIds = next.threads.map((thread) => thread.id).toSet();
      final threadsById = {
        for (final thread in next.threads) thread.id: thread,
      };
      final tasks = Map<String, TaskRuntimeView>.from(
        current.tasksByRootThread,
      );
      final selectedRootThreadId = next.selectedRootThread?.id;
      if (selectedRootThreadId != null) {
        tasks.remove(selectedRootThreadId);
      }
      tasks.addAll(next.tasksByRootThread);
      next = next.copyWith(
        workspacesByThread: {
          for (final entry in current.workspacesByThread.entries)
            if (knownIds.contains(entry.key))
              entry.key: entry.value.copyWith(thread: threadsById[entry.key]),
        },
        workspaceUiByThread: {
          for (final entry in current.workspaceUiByThread.entries)
            if (knownIds.contains(entry.key)) entry.key: entry.value,
        },
        tasksByRootThread: tasks,
      );
    }
    state = AsyncData(next);
    if (previousThreadId != next.selectedThreadId) {
      await _subscribeThread(next.selectedThreadId);
    }
  }
}

StudioState _reconcileComposer(StudioState state, String threadId) {
  final workspace = state.workspacesByThread[threadId];
  if (workspace == null) return state;
  return _withWorkspaceUi(
    state,
    threadId,
    (ui) =>
        ui.copyWith(composer: ui.composer.observeTurn(workspace.activeTurn)),
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
  return state.copyWith(
    providerCatalog: catalog,
    providers: [
      for (final provider in state.providers)
        providerWithCatalogMetadata(provider, catalog),
    ],
  );
}
