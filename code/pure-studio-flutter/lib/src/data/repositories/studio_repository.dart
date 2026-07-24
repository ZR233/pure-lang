import 'dart:async';

import 'package:flutter/scheduler.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../domain/models/studio_models.dart';
import '../frb/studio_api.dart';
import 'studio_state_reducer.dart';

final studioApiProvider = Provider<StudioApi>((ref) {
  if (const bool.fromEnvironment('PURE_STUDIO_DEMO')) {
    return DemoStudioApi();
  }
  return FrbStudioApi();
});

final studioControllerProvider =
    AsyncNotifierProvider<StudioController, StudioState>(StudioController.new);

class StudioController extends AsyncNotifier<StudioState> {
  StreamSubscription<Object>? _globalSubscription;
  StreamSubscription<SessionStreamFrame>? _sessionSubscription;
  final List<StudioBridgeEvent> _pendingPartDeltas = [];
  bool _partDeltaFrameScheduled = false;
  int _sessionGeneration = 0;

  StudioApi get _api => ref.read(studioApiProvider);

  @override
  Future<StudioState> build() async {
    ref.onDispose(() {
      _pendingPartDeltas.clear();
      _partDeltaFrameScheduled = false;
      final global = _globalSubscription;
      final session = _sessionSubscription;
      if (global != null) {
        unawaited(global.cancel());
      }
      if (session != null) {
        unawaited(session.cancel());
      }
    });
    final catalog = await _api.loadProviderCatalog();
    final bootstrapped = _attachProviderCatalog(
      await _api.bootstrap(),
      catalog,
    );
    final sessionId = bootstrapped.selectedSessionId;
    _subscribe(sessionId);
    return bootstrapped;
  }

  Future<void> openProject(String path) async {
    final next = await _api.openProject(path);
    await _adoptState(next);
  }

  Future<void> selectProject(String projectId) async {
    final current = state.value;
    if (current == null || current.selectedProjectId == projectId) {
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
    if (current == null || current.isBusy) {
      return;
    }
    final next = await _api.archiveSession(
      sessionId,
      selectedSessionId: current.selectedSessionId,
    );
    await _adoptState(next);
  }

  void _subscribe(String? sessionId, {bool forceSnapshot = false}) {
    _flushPartDeltaBatch();
    _sessionGeneration += 1;
    final generation = _sessionGeneration;
    _globalSubscription ??= _api.subscribeProductEvents().listen(_handleEvent);
    final oldSession = _sessionSubscription;
    if (oldSession != null) {
      unawaited(oldSession.cancel());
    }
    final afterSequence = forceSnapshot || sessionId == null
        ? null
        : state.value?.eventCursorsBySession[sessionId];
    _sessionSubscription = sessionId == null
        ? null
        : _api
              .subscribeSessionEvents(sessionId, afterSequence: afterSequence)
              .listen(
                (frame) => _handleSessionFrame(frame, sessionId, generation),
                onError: (_) => _resubscribeSnapshot(sessionId, generation),
              );
  }

  Future<void> selectSession(String sessionId) async {
    final current = state.value;
    if (current == null || current.selectedSessionId == sessionId) {
      return;
    }
    final session = current.sessions
        .where((session) => session.id == sessionId)
        .firstOrNull;
    final composerTexts = _composerTextsAfterLeaving(current);
    state = AsyncData(
      current.copyWith(
        selectedRootSessionId: session?.effectiveRootSessionId ?? sessionId,
        selectedSessionId: sessionId,
        composerText: composerTexts[sessionId] ?? '',
        composerTextsBySession: composerTexts,
      ),
    );
    _subscribe(sessionId);
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
    final selectedRoot = current.selectedRootSession;
    if (selectedRoot != null &&
        target.effectiveRootSessionId != selectedRoot.id) {
      return;
    }
    final composerTexts = _composerTextsAfterLeaving(current);
    state = AsyncData(
      current.copyWith(
        selectedRootSessionId: target.effectiveRootSessionId,
        selectedSessionId: target.id,
        composerText: composerTexts[target.id] ?? '',
        composerTextsBySession: composerTexts,
      ),
    );
    _subscribe(target.id);
  }

  void updateComposer(String value) {
    final current = state.value;
    if (current == null) {
      return;
    }
    final sessionId = current.selectedAgentSessionId;
    final composerTexts = {...current.composerTextsBySession};
    if (sessionId != null) {
      composerTexts[sessionId] = value;
    }
    state = AsyncData(
      current.copyWith(
        composerText: value,
        composerTextsBySession: composerTexts,
      ),
    );
  }

  Future<void> submitComposer() async {
    final current = state.value;
    final sessionId = current?.selectedSessionId;
    final prompt = current?.composerText.trim() ?? '';
    if (current == null || sessionId == null || prompt.isEmpty) {
      return;
    }
    final composerTexts = {...current.composerTextsBySession}..[sessionId] = '';
    state = AsyncData(
      current.copyWith(
        composerText: '',
        composerTextsBySession: composerTexts,
        turnPhase: TurnPhase.waitingForModel,
      ),
    );
    await _api.submitPrompt(sessionId, prompt, const []);
  }

  Future<void> stop() async {
    final current = state.value;
    final sessionId = current?.selectedSessionId;
    if (current == null || sessionId == null) {
      return;
    }
    await _api.stopPrompt(sessionId);
    state = AsyncData(current.copyWith(turnPhase: TurnPhase.cancelled));
  }

  Future<void> setPermissionMode(PermissionMode mode) async {
    final current = state.value;
    if (current == null) {
      return;
    }
    state = AsyncData(current.copyWith(permissionMode: mode));
    await _api.saveRuntimePermissionMode(mode);
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
    state = AsyncData(
      current.copyWith(
        sessions: [
          for (final session in current.sessions)
            session.id == sessionId ? session.copyWith(mode: mode) : session,
        ],
      ),
    );
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
    final optimisticRole = RoleSettingsView(
      key: roleKey,
      providerId: providerId,
      model: model,
      effort: nextEffort,
    );
    state = AsyncData(
      current.copyWith(roles: replaceStudioRole(current.roles, optimisticRole)),
    );
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

  Future<void> saveProviderSettings(Map<String, Object?> settings) async {
    final current = state.value;
    if (current == null) {
      return;
    }
    final next = await _api.saveProviderSettings(settings);
    final latest = state.value;
    if (latest == null) {
      return;
    }
    state = AsyncData(mergeStudioConfigState(latest, next));
  }

  Future<void> saveInstructionsSettings(Map<String, Object?> settings) async {
    await _saveConfigSettings(() => _api.saveInstructionsSettings(settings));
  }

  Future<void> saveSkillsSettings(Map<String, Object?> settings) async {
    await _saveConfigSettings(() => _api.saveSkillsSettings(settings));
  }

  Future<void> saveMcpSettings(Map<String, Object?> settings) async {
    await _saveConfigSettings(() => _api.saveMcpSettings(settings));
  }

  Future<void> saveGeneralSettings(Map<String, Object?> settings) async {
    await _saveConfigSettings(() => _api.saveGeneralSettings(settings));
  }

  Future<void> saveWebSearchSettings(WebSearchSettingsView settings) async {
    await _saveConfigSettings(() => _api.saveWebSearchSettings(settings));
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

  Future<void> saveSettingsDraft(
    String section,
    Map<String, Object?> draft,
  ) async {
    await _api.saveStudioSettingsDraft(section, draft);
  }

  Future<void> resolveActiveInteraction(Map<String, Object?> resolution) async {
    final current = state.value;
    final interaction = current?.activeInteraction;
    if (current == null || interaction == null) {
      return;
    }
    final shouldContinuePlanning =
        resolution['type'] == 'planConfirmation' &&
        resolution['decision'] == 'continuePlanning';
    final shouldImplementPlan =
        resolution['type'] == 'planConfirmation' &&
        resolution['decision'] == 'implementFreshContext';
    final followUpPrompt = planFollowUpPrompt(interaction, resolution);
    await _api.resolveInteraction(interaction.id, resolution);
    final latest = state.value ?? current;
    state = AsyncData(
      latest.copyWith(
        sessions: shouldImplementPlan
            ? [
                for (final session in latest.sessions)
                  session.id == interaction.sessionId
                      ? session.copyWith(mode: StudioMode.task)
                      : session,
              ]
            : latest.sessions,
        pendingInteractions: latest.pendingInteractions
            .where((candidate) => candidate.id != interaction.id)
            .toList(),
        turnPhase: shouldContinuePlanning && followUpPrompt.isNotEmpty
            ? TurnPhase.waitingForModel
            : latest.turnPhase,
      ),
    );
    if (shouldContinuePlanning &&
        followUpPrompt.isNotEmpty &&
        interaction.sessionId.isNotEmpty) {
      await _api.submitPrompt(interaction.sessionId, followUpPrompt, const []);
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
    _flushPartDeltaBatch();
    final latest = state.value;
    if (latest == null) {
      return;
    }
    if (event.payload is StalePayload) {
      final sessionId = event.sessionId ?? latest.selectedSessionId;
      if (sessionId != null) {
        _resubscribeSnapshot(sessionId, _sessionGeneration);
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
    if (generation != _sessionGeneration ||
        state.value?.selectedSessionId != sessionId) {
      return;
    }
    switch (frame) {
      case SessionSnapshotFrame(:final snapshot):
        _flushPartDeltaBatch();
        final current = state.value;
        if (current != null) {
          state = AsyncData(applyCanonicalSessionSnapshot(current, snapshot));
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
    if (generation != _sessionGeneration ||
        state.value?.selectedSessionId != sessionId) {
      return;
    }
    _subscribe(sessionId, forceSnapshot: true);
  }

  void _queuePartDelta(StudioState current, StudioBridgeEvent event) {
    if (!targetsSelectedSession(current, event)) {
      return;
    }
    _pendingPartDeltas.add(event);
    if (_partDeltaFrameScheduled) {
      return;
    }
    _partDeltaFrameScheduled = true;
    SchedulerBinding.instance.scheduleFrameCallback((_) {
      _partDeltaFrameScheduled = false;
      _flushPartDeltaBatch();
    });
  }

  void _flushPartDeltaBatch() {
    if (_pendingPartDeltas.isEmpty) {
      _partDeltaFrameScheduled = false;
      return;
    }
    final events = List<StudioBridgeEvent>.of(_pendingPartDeltas);
    _pendingPartDeltas.clear();
    _partDeltaFrameScheduled = false;
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
    final catalog = current?.providerCatalog;
    if (catalog != null) {
      next = _attachProviderCatalog(next, catalog);
    }
    if (current != null) {
      final composerTexts = _composerTextsAfterLeaving(current);
      final turnPhases = {
        ...current.turnPhasesBySession,
        ...next.turnPhasesBySession,
      };
      final runtimes = {
        ...current.runtimesBySession,
        ...next.runtimesBySession,
      };
      final currentSessionId = current.selectedAgentSessionId;
      if (currentSessionId != null) {
        turnPhases[currentSessionId] = current.turnPhase;
        runtimes[currentSessionId] = current.runtime;
      }
      next = next.copyWith(
        composerText:
            composerTexts[next.selectedAgentSessionId] ?? next.composerText,
        composerTextsBySession: composerTexts,
        turnPhasesBySession: turnPhases,
        runtimesBySession: runtimes,
      );
    }
    final selected = next.selectedAgentSession;
    next = next.copyWith(
      selectedRootSessionId:
          selected?.effectiveRootSessionId ?? next.rootSessions.firstOrNull?.id,
    );
    state = AsyncData(next);
    _subscribe(next.selectedSessionId);
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
      _resubscribeSnapshot(staleSessionId, _sessionGeneration);
    }
    return reduced.state;
  }
}

Map<String, String> _composerTextsAfterLeaving(StudioState state) {
  final composerTexts = {...state.composerTextsBySession};
  final sessionId = state.selectedAgentSessionId;
  if (sessionId != null) {
    composerTexts[sessionId] = state.composerText;
  }
  return composerTexts;
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
