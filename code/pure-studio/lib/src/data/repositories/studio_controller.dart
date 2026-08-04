import 'dart:async';

import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../../domain/models/studio_models.dart';
import '../frb/studio_api.dart';
import 'studio_api_provider.dart';
import 'studio_state_reducer.dart';
import 'studio_stream_coordinators.dart';

part 'studio_controller.g.dart';

Duration? _disableStudioRetry(int retryCount, Object error) => null;

@Riverpod(keepAlive: true, retry: _disableStudioRetry)
class StudioController extends _$StudioController {
  late ProductStreamCoordinator _productCoordinator;
  late SessionStreamCoordinator _sessionCoordinator;
  late PartDeltaBatcher _partDeltaBatcher;

  StudioApi get _api => ref.read(studioApiProvider);

  @override
  Future<StudioState> build() async {
    _productCoordinator = ProductStreamCoordinator(_api, _handleEvent);
    _sessionCoordinator = SessionStreamCoordinator(
      _api,
      _handleSessionFrame,
      _resubscribeSnapshot,
    );
    _partDeltaBatcher = PartDeltaBatcher(_applyPartDeltaBatch);
    ref.onDispose(() {
      _partDeltaBatcher.dispose();
      unawaited(_productCoordinator.dispose());
      unawaited(_sessionCoordinator.dispose());
    });
    final catalog = await _api.loadProviderCatalog();
    var bootstrapped = _attachProviderCatalog(await _api.bootstrap(), catalog);
    final sessionId = bootstrapped.selectedSessionId;
    if (sessionId != null) {
      final page = await _api.loadSessionHistoryPage(sessionId);
      bootstrapped = _withHistoryPaging(
        mergeSessionHistoryPage(bootstrapped, sessionId, page),
        sessionId,
        SessionHistoryPagingState(
          nextBeforeTurnSequence: page.nextBeforeTurnSequence,
          hasMore: page.hasMore,
          isLoading: false,
          isLoaded: true,
        ),
      );
    }
    await _subscribe(sessionId, forceSnapshot: true);
    return bootstrapped;
  }

  Future<void> openProject(String path) async {
    final next = await _api.openProject(path);
    await _adoptState(next);
  }

  Future<void> selectProject(String projectId) async {
    final current = state.value;
    if (current == null ||
        current.selectedProjectId == projectId ||
        current.recoveryIssueForProject(projectId) != null) {
      return;
    }
    final next = await _api.selectProject(projectId);
    await _adoptState(next);
  }

  Future<void> archiveProject(String projectId) async {
    final current = state.value;
    if (current == null) {
      return;
    }
    if (current.isBusy && current.selectedProjectId == projectId) {
      return;
    }
    final next = await _api.archiveProject(
      projectId,
      selectedProjectId: current.selectedProjectId,
    );
    await _adoptState(next);
  }

  Future<RecoveryCleanupPreview> previewProjectCleanup(String projectId) {
    return _api.previewProjectCleanup(projectId);
  }

  Future<void> cleanupProject(String projectId, String expectedRevision) async {
    final current = state.value;
    if (current == null) {
      return;
    }
    final next = await _api.cleanupProject(
      projectId,
      expectedRevision,
      selectedProjectId: current.selectedProjectId,
    );
    await _adoptState(next);
  }

  Future<void> createSession() async {
    final current = state.value;
    final projectId = current?.selectedProjectId;
    if (projectId == null) {
      return;
    }
    final next = await _api.createSession(projectId);
    await _adoptState(next);
  }

  Future<void> archiveSession(String sessionId) async {
    final current = state.value;
    if (current == null ||
        (current.isBusy && current.selectedRootSession?.id == sessionId)) {
      return;
    }
    final next = await _api.archiveSession(
      sessionId,
      selectedSessionId: current.selectedSessionId,
    );
    await _adoptState(next);
  }

  Future<void> _subscribe(
    String? sessionId, {
    bool forceSnapshot = false,
  }) async {
    _partDeltaBatcher.flush();
    _productCoordinator.start();
    final afterSequence = forceSnapshot || sessionId == null
        ? null
        : state.value?.eventCursorsBySession[sessionId];
    await _sessionCoordinator.switchSession(
      sessionId,
      afterSequence: afterSequence,
    );
  }

  Future<void> selectSession(String sessionId) async {
    final current = state.value;
    if (current == null ||
        current.selectedSessionId == sessionId ||
        current.recoveryIssueForSession(sessionId) != null) {
      return;
    }
    final session = current.sessions
        .where((session) => session.id == sessionId)
        .firstOrNull;
    final syncStates = _workspaceSyncAfterSelecting(current, sessionId);
    state = AsyncData(
      current.copyWith(
        selectedRootSessionId: session?.effectiveRootSessionId ?? sessionId,
        selectedSessionId: sessionId,
        workspaceSyncBySession: syncStates,
      ),
    );
    await loadOlderHistory(sessionId);
    await _subscribe(sessionId, forceSnapshot: true);
  }

  Future<void> selectAgentSession(String sessionId) async {
    final current = state.value;
    if (current == null || current.selectedSessionId == sessionId) {
      return;
    }
    final target = current.sessions
        .where((session) => session.id == sessionId)
        .firstOrNull;
    if (target == null) {
      return;
    }
    if (current.recoveryIssueForSession(sessionId) != null) {
      return;
    }
    final selectedRoot = current.selectedRootSession;
    if (selectedRoot != null &&
        target.effectiveRootSessionId != selectedRoot.id) {
      return;
    }
    final syncStates = _workspaceSyncAfterSelecting(current, target.id);
    state = AsyncData(
      current.copyWith(
        selectedRootSessionId: target.effectiveRootSessionId,
        selectedSessionId: target.id,
        workspaceSyncBySession: syncStates,
      ),
    );
    await loadOlderHistory(target.id);
    await _subscribe(target.id, forceSnapshot: true);
  }

  Future<void> loadOlderHistory(String sessionId) async {
    final current = state.value;
    if (current == null ||
        !current.sessions.any((session) => session.id == sessionId)) {
      return;
    }
    final paging =
        current.historyPagingBySession[sessionId] ??
        const SessionHistoryPagingState.initial();
    if (paging.isLoading || (paging.isLoaded && !paging.hasMore)) {
      return;
    }
    final requestCursor = paging.isLoaded
        ? paging.nextBeforeTurnSequence
        : null;
    state = AsyncData(
      _withHistoryPaging(
        current,
        sessionId,
        SessionHistoryPagingState(
          nextBeforeTurnSequence: paging.nextBeforeTurnSequence,
          hasMore: paging.hasMore,
          isLoading: true,
          isLoaded: paging.isLoaded,
        ),
      ),
    );
    try {
      final page = await _api.loadSessionHistoryPage(
        sessionId,
        beforeTurnSequence: requestCursor,
      );
      if (!ref.mounted) {
        return;
      }
      final latest = state.value;
      if (latest == null) {
        return;
      }
      state = AsyncData(
        _withHistoryPaging(
          mergeSessionHistoryPage(latest, sessionId, page),
          sessionId,
          SessionHistoryPagingState(
            nextBeforeTurnSequence: page.nextBeforeTurnSequence,
            hasMore: page.hasMore,
            isLoading: false,
            isLoaded: true,
          ),
        ),
      );
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      final latest = state.value;
      if (latest == null) {
        return;
      }
      final active =
          latest.historyPagingBySession[sessionId] ??
          const SessionHistoryPagingState.initial();
      state = AsyncData(
        _withHistoryPaging(
          latest,
          sessionId,
          SessionHistoryPagingState(
            nextBeforeTurnSequence: active.nextBeforeTurnSequence,
            hasMore: active.hasMore,
            isLoading: false,
            isLoaded: active.isLoaded,
            errorMessage: error.toString(),
          ),
        ),
      );
    }
  }

  void updateComposer(String sessionId, String value) {
    final current = state.value;
    if (current == null || current.selectedAgentSessionId != sessionId) {
      return;
    }
    final composer =
        current.composersBySession[sessionId] ??
        const ComposerSessionState.idle();
    state = AsyncData(
      _withComposer(current, sessionId, composer.updateDraft(value)),
    );
  }

  Future<void> submitComposer(String sessionId) async {
    final current = state.value;
    final composer =
        current?.composersBySession[sessionId] ??
        const ComposerSessionState.idle();
    final prompt = composer.draft.trim();
    if (current == null ||
        current.selectedAgentSessionId != sessionId ||
        prompt.isEmpty ||
        composer.isSubmissionPending) {
      return;
    }
    final submitting = composer.beginSubmission();
    await _submitSessionInput(
      current,
      sessionId,
      submitting,
      () => _api.submitPrompt(sessionId, prompt, const []),
    );
  }

  Future<void> resumeTask(String sessionId) async {
    final current = state.value;
    final composer =
        current?.composersBySession[sessionId] ??
        const ComposerSessionState.idle();
    if (current == null ||
        current.selectedAgentSessionId != sessionId ||
        current.selectedAgentWorkspace?.isTaskPaused != true ||
        composer.isSubmissionPending) {
      return;
    }
    final submitting = composer.beginCommandSubmission();
    await _submitSessionInput(
      current,
      sessionId,
      submitting,
      () => _api.resumeTask(sessionId),
    );
  }

  Future<void> _submitSessionInput(
    StudioState current,
    String sessionId,
    ComposerSessionState submitting,
    Future<SubmitPromptReceipt> Function() submit,
  ) async {
    final submissionRevision = submitting.submissionRevision;
    state = AsyncData(_withComposer(current, sessionId, submitting));
    final SubmitPromptReceipt receipt;
    try {
      receipt = await submit();
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      final latest = state.value;
      final active = latest?.composersBySession[sessionId];
      if (latest == null || active == null) {
        return;
      }
      final failed = active.fail(error, submissionRevision: submissionRevision);
      if (!identical(failed, active)) {
        state = AsyncData(_withComposer(latest, sessionId, failed));
      }
      return;
    }
    if (!ref.mounted) {
      return;
    }
    final latest = state.value;
    final active = latest?.composersBySession[sessionId];
    if (latest == null || active == null) {
      return;
    }
    if (receipt.sessionId != sessionId) {
      final failed = active.fail(
        StateError(
          'submit receipt session ${receipt.sessionId} does not match $sessionId',
        ),
        submissionRevision: submissionRevision,
      );
      if (!identical(failed, active)) {
        state = AsyncData(_withComposer(latest, sessionId, failed));
      }
      return;
    }
    final accepted = active
        .accept(receipt, submissionRevision: submissionRevision)
        .observeTurn(latest.turnsBySession[sessionId]);
    if (identical(accepted, active)) {
      return;
    }
    state = AsyncData(_withComposer(latest, sessionId, accepted));
    if (state.value?.selectedSessionId == sessionId) {
      await _subscribe(sessionId, forceSnapshot: true);
    }
  }

  Future<void> stop(String sessionId) async {
    final current = state.value;
    if (current == null || current.selectedAgentSessionId != sessionId) {
      return;
    }
    await _api.stopPrompt(sessionId);
  }

  Future<void> setPermissionMode(PermissionMode mode) async {
    final current = state.value;
    if (current == null) {
      return;
    }
    final next = await _api.saveRuntimePermissionMode(mode);
    final latest = state.value;
    if (latest != null) {
      state = AsyncData(mergeStudioConfigState(latest, next));
    }
  }

  Future<void> setSessionMode(StudioMode mode) async {
    final current = state.value;
    final sessionId = current?.selectedSessionId;
    if (current == null ||
        sessionId == null ||
        current.isBusy ||
        current.runtime.hasActiveTask ||
        current.sessions
                .where((session) => session.id == sessionId)
                .firstOrNull
                ?.mode ==
            mode) {
      return;
    }
    final updated = await _api.setSessionMode(sessionId, mode);
    final latest = state.value;
    if (latest == null) {
      return;
    }
    state = AsyncData(
      latest.copyWith(
        sessions: [
          for (final session in latest.sessions)
            session.id == updated.id ? updated : session,
        ],
      ),
    );
  }

  Future<void> setModelRole({
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
  }) async {
    final current = state.value;
    if (current == null) {
      return;
    }
    final role = current.role(roleKey);
    if (role != null &&
        role.providerId == providerId &&
        role.model == model &&
        (effort == null || role.effort == effort)) {
      return;
    }
    final nextEffort =
        effort ?? defaultEffortForModel(current, providerId, model);
    final next = await _api.setModelRole(
      roleKey: roleKey,
      providerId: providerId,
      model: model,
      effort: nextEffort,
      selectedSessionId: current.selectedSessionId,
    );
    final latest = state.value;
    if (latest == null) {
      return;
    }
    state = AsyncData(mergeStudioConfigState(latest, next));
  }

  Future<void> saveProviderSettings(ProviderSettingsCommand command) async {
    final current = state.value;
    if (current == null) {
      return;
    }
    final next = await _api.saveProviderSettings(command);
    final latest = state.value;
    if (latest == null) {
      return;
    }
    state = AsyncData(mergeStudioConfigState(latest, next));
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
    final current = state.value;
    if (current == null) {
      return;
    }
    final next = await request();
    final latest = state.value;
    if (latest == null) {
      return;
    }
    state = AsyncData(mergeStudioConfigState(latest, next));
  }

  Future<void> refreshProviderUsages() async {
    final current = state.value;
    if (current == null) {
      return;
    }
    final usages = await _api.loadProviderUsages();
    final latest = state.value;
    if (latest == null) {
      return;
    }
    state = AsyncData(latest.copyWith(providerUsages: usages));
  }

  Future<List<String>> listDiscoveredSkills() async {
    final projectId = state.value?.selectedProjectId;
    if (projectId == null) {
      return const [];
    }
    return _api.listDiscoveredSkills(projectId);
  }

  Future<RecoveryCleanupPreview> previewRecoveryIssueCleanup(String issueId) {
    return _api.previewRecoveryIssueCleanup(issueId);
  }

  Future<void> cleanupRecoveryIssue(
    String issueId,
    String expectedRevision,
  ) async {
    final current = state.value;
    if (current == null) {
      return;
    }
    final next = await _api.cleanupRecoveryIssue(
      issueId,
      expectedRevision,
      selectedProjectId: current.selectedProjectId,
      selectedSessionId: current.selectedSessionId,
    );
    await _adoptState(next);
  }

  void retryInitialization() {
    ref.invalidateSelf();
  }

  Future<void> resolveActiveInteraction(
    String sessionId,
    InteractionResolutionCommand resolution,
  ) async {
    final current = state.value;
    final interaction = current?.activeInteraction;
    if (current == null ||
        current.selectedAgentSessionId != sessionId ||
        interaction == null) {
      return;
    }
    final shouldContinuePlanning =
        resolution is PlanConfirmationResolutionCommand &&
        resolution.decision == PlanConfirmationDecision.continuePlanning;
    final followUpPrompt = planFollowUpPrompt(interaction, resolution);
    final result = await _api.resolveInteraction(interaction.id, resolution);
    final latest = state.value ?? current;
    state = AsyncData(
      latest.copyWith(
        sessions: result.sessions.isEmpty ? latest.sessions : result.sessions,
        pendingInteractions: result.isResolved
            ? latest.pendingInteractions
                  .where((candidate) => candidate.id != result.interactionId)
                  .toList()
            : latest.pendingInteractions,
      ),
    );
    if (shouldContinuePlanning &&
        followUpPrompt.isNotEmpty &&
        interaction.sessionId.isNotEmpty) {
      await _api.submitPrompt(interaction.sessionId, followUpPrompt, const []);
    }
    if (result.isResolved &&
        interaction.sessionId.isNotEmpty &&
        state.value?.selectedAgentSessionId == interaction.sessionId) {
      await _subscribe(interaction.sessionId, forceSnapshot: true);
    }
  }

  void _handleEvent(Object event) {
    final current = state.value;
    if (current == null || event is! StudioBridgeEvent) {
      return;
    }
    if (event.payload is MessagePartDeltaPayload) {
      _queuePartDelta(current, event);
      return;
    }
    _partDeltaBatcher.flush();
    final latest = state.value;
    if (latest == null) {
      return;
    }
    if (event.payload is StalePayload) {
      final sessionId = event.sessionId ?? latest.selectedSessionId;
      if (sessionId != null) {
        _resubscribeSnapshot(sessionId, _sessionCoordinator.generation);
      }
      return;
    }
    if (!targetsSelectedSession(latest, event) ||
        isDuplicateDurableEvent(latest, event)) {
      return;
    }
    state = AsyncData(_reduceEventWithCursor(latest, event));
  }

  void _handleSessionFrame(
    SessionStreamFrame frame,
    String sessionId,
    int generation,
  ) {
    if (generation != _sessionCoordinator.generation ||
        state.value?.selectedSessionId != sessionId) {
      return;
    }
    switch (frame) {
      case SessionSnapshotFrame(:final snapshot):
        _partDeltaBatcher.flush();
        final current = state.value;
        if (current != null) {
          state = AsyncData(
            _reconcileComposerTurns(
              applyCanonicalSessionSnapshot(current, snapshot),
            ),
          );
        }
      case SessionEventFrame(:final event):
        final eventSessionId = studioEventSessionId(event);
        if (eventSessionId == null || eventSessionId == sessionId) {
          _handleEvent(event);
        }
      case SessionResyncRequiredFrame():
        _resubscribeSnapshot(sessionId, generation);
    }
  }

  void _resubscribeSnapshot(String sessionId, int generation) {
    if (generation != _sessionCoordinator.generation ||
        state.value?.selectedSessionId != sessionId) {
      return;
    }
    final current = state.value;
    if (current != null) {
      state = AsyncData(
        current.copyWith(
          workspaceSyncBySession: {
            ...current.workspaceSyncBySession,
            sessionId: AgentWorkspaceSyncState.reconnecting,
          },
        ),
      );
    }
    _sessionCoordinator.scheduleResync(
      sessionId: sessionId,
      generation: generation,
      isCurrent: () => state.value?.selectedSessionId == sessionId,
    );
  }

  void _queuePartDelta(StudioState current, StudioBridgeEvent event) {
    if (!targetsSelectedSession(current, event)) {
      return;
    }
    _partDeltaBatcher.add(event);
  }

  void _applyPartDeltaBatch(List<StudioBridgeEvent> events) {
    final current = state.value;
    if (current == null) {
      return;
    }
    var latest = current;
    for (final event in events) {
      if (!targetsSelectedSession(latest, event)) {
        continue;
      }
      latest = _reduceEventState(latest, event);
    }
    state = AsyncData(latest);
  }

  Future<void> _adoptState(StudioState next) async {
    final current = state.value;
    final previousSessionId = current?.selectedSessionId;
    final catalog = current?.providerCatalog;
    if (catalog != null) {
      next = _attachProviderCatalog(next, catalog);
    }
    if (current != null) {
      final turns = {...current.turnsBySession, ...next.turnsBySession};
      final runtimes = {
        ...current.runtimesBySession,
        ...next.runtimesBySession,
      };
      next = next.copyWith(
        composersBySession: current.composersBySession,
        turnsBySession: turns,
        runtimesBySession: runtimes,
        workspaceSyncBySession: {
          ...current.workspaceSyncBySession,
          ...next.workspaceSyncBySession,
        },
      );
    }
    final selected = next.selectedAgentSession;
    next = next.copyWith(
      selectedRootSessionId:
          selected?.effectiveRootSessionId ?? next.rootSessions.firstOrNull?.id,
    );
    next = _reconcileComposerTurns(next);
    state = AsyncData(next);
    if (previousSessionId != next.selectedSessionId) {
      final sessionId = next.selectedSessionId;
      if (sessionId != null) {
        await loadOlderHistory(sessionId);
      }
      await _subscribe(next.selectedSessionId, forceSnapshot: true);
    }
  }

  StudioState _reduceEventWithCursor(
    StudioState current,
    StudioBridgeEvent event,
  ) {
    return withStudioEventCursor(_reduceEventState(current, event), event);
  }

  StudioState _reduceEventState(StudioState current, StudioBridgeEvent event) {
    final reduced = reduceStudioEvent(current, event);
    final staleSessionId = reduced.staleSessionId;
    if (staleSessionId != null) {
      _resubscribeSnapshot(staleSessionId, _sessionCoordinator.generation);
    }
    return _reconcileComposerTurns(reduced.state);
  }
}

StudioState _reconcileComposerTurns(StudioState state) {
  final updates = <String, ComposerSessionState>{};
  for (final entry in state.composersBySession.entries) {
    final reconciled = entry.value.observeTurn(state.turnsBySession[entry.key]);
    if (!identical(reconciled, entry.value)) {
      updates[entry.key] = reconciled;
    }
  }
  if (updates.isEmpty) {
    return state;
  }
  return state.copyWith(
    composersBySession: {...state.composersBySession, ...updates},
  );
}

StudioState _withComposer(
  StudioState state,
  String sessionId,
  ComposerSessionState composer,
) {
  return state.copyWith(
    composersBySession: {...state.composersBySession, sessionId: composer},
  );
}

StudioState _withHistoryPaging(
  StudioState state,
  String sessionId,
  SessionHistoryPagingState paging,
) {
  return state.copyWith(
    historyPagingBySession: {
      ...state.historyPagingBySession,
      sessionId: paging,
    },
  );
}

Map<String, AgentWorkspaceSyncState> _workspaceSyncAfterSelecting(
  StudioState state,
  String sessionId,
) {
  final existing = state.workspaceSyncBySession[sessionId];
  return {
    ...state.workspaceSyncBySession,
    sessionId: existing ?? AgentWorkspaceSyncState.loading,
  };
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
