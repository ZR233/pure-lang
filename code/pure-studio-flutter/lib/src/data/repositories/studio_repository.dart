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

class _BufferedSessionEvent {
  const _BufferedSessionEvent(this.event, this.arrivalIndex);

  final StudioBridgeEvent event;
  final int arrivalIndex;
}

class StudioController extends AsyncNotifier<StudioState> {
  StreamSubscription<Object>? _globalSubscription;
  StreamSubscription<Object>? _sessionSubscription;
  final List<StudioBridgeEvent> _pendingPartDeltas = [];
  bool _partDeltaFrameScheduled = false;
  final List<_BufferedSessionEvent> _sessionLoadBuffer = [];
  int _sessionLoadBufferNextIndex = 0;
  int _sessionGeneration = 0;
  String? _loadingSessionId;
  int? _loadingGeneration;
  bool _bufferedStaleSession = false;

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
      _clearAnySessionLoadBarrier();
    });
    final catalog = await _api.loadProviderCatalog();
    final bootstrapped = _attachProviderCatalog(
      await _api.bootstrap(),
      catalog,
    );
    final sessionId = bootstrapped.selectedSessionId;
    _subscribe(sessionId);
    if (sessionId == null) {
      return bootstrapped;
    }
    final generation = _sessionGeneration;
    _startSessionLoadBarrier(sessionId, generation);
    final projected = await _withSelectedSessionProjection(bootstrapped);
    return _flushSessionLoadBuffer(projected, sessionId, generation);
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

  void _subscribe(String? sessionId) {
    _flushPartDeltaBatch();
    _sessionGeneration += 1;
    final generation = _sessionGeneration;
    _globalSubscription ??= _api.subscribeGlobalEvents().listen(_handleEvent);
    final oldSession = _sessionSubscription;
    if (oldSession != null) {
      unawaited(oldSession.cancel());
    }
    _sessionSubscription = sessionId == null
        ? null
        : _api
              .subscribeSessionEvents(sessionId)
              .listen(
                (event) => _handleSessionEvent(event, sessionId, generation),
              );
  }

  Future<void> selectSession(String sessionId) async {
    final current = state.value;
    if (current == null || current.selectedSessionId == sessionId) {
      return;
    }
    state = AsyncData(current.copyWith(selectedSessionId: sessionId));
    _subscribe(sessionId);
    final generation = _sessionGeneration;
    _startSessionLoadBarrier(sessionId, generation);
    final sessionState = await _api.loadSessionState(sessionId);
    final latest = state.value;
    if (latest == null ||
        latest.selectedSessionId != sessionId ||
        generation != _sessionGeneration) {
      _clearSessionLoadBarrier(sessionId, generation);
      return;
    }
    var merged = mergeStudioSessionState(latest, sessionState);
    merged = _flushSessionLoadBuffer(merged, sessionId, generation);
    state = AsyncData(merged);
  }

  void updateComposer(String value) {
    final current = state.value;
    if (current == null) {
      return;
    }
    state = AsyncData(current.copyWith(composerText: value));
  }

  Future<void> submitComposer() async {
    final current = state.value;
    final sessionId = current?.selectedSessionId;
    final prompt = current?.composerText.trim() ?? '';
    if (current == null || sessionId == null || prompt.isEmpty) {
      return;
    }
    state = AsyncData(
      current.copyWith(composerText: '', turnPhase: TurnPhase.waitingForModel),
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
    final next = await _api.setSessionMode(sessionId, mode);
    final latest = state.value;
    if (latest == null) {
      return;
    }
    state = AsyncData(mergeStudioSessionState(latest, next));
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
        unawaited(_recoverStaleSession(sessionId));
      }
      return;
    }
    if (!targetsSelectedSession(latest, event) ||
        isDuplicateDurableEvent(latest, event)) {
      return;
    }
    state = AsyncData(_reduceEventWithCursor(latest, event));
  }

  void _handleSessionEvent(Object event, String sessionId, int generation) {
    if (generation != _sessionGeneration || event is! StudioBridgeEvent) {
      return;
    }
    final eventSessionId = studioEventSessionId(event);
    if (eventSessionId != null && eventSessionId != sessionId) {
      return;
    }
    if (_loadingSessionId == sessionId && _loadingGeneration == generation) {
      if (event.payload is StalePayload) {
        _bufferedStaleSession = true;
      } else {
        _sessionLoadBuffer.add(
          _BufferedSessionEvent(event, _sessionLoadBufferNextIndex++),
        );
      }
      return;
    }
    _handleEvent(event);
  }

  void _startSessionLoadBarrier(String sessionId, int generation) {
    _flushPartDeltaBatch();
    _loadingSessionId = sessionId;
    _loadingGeneration = generation;
    _bufferedStaleSession = false;
    _sessionLoadBuffer.clear();
    _sessionLoadBufferNextIndex = 0;
  }

  void _clearAnySessionLoadBarrier() {
    _loadingSessionId = null;
    _loadingGeneration = null;
    _bufferedStaleSession = false;
    _sessionLoadBuffer.clear();
    _sessionLoadBufferNextIndex = 0;
  }

  void _clearSessionLoadBarrier(String sessionId, int generation) {
    if (_loadingSessionId == sessionId && _loadingGeneration == generation) {
      _clearAnySessionLoadBarrier();
    }
  }

  StudioState _flushSessionLoadBuffer(
    StudioState current,
    String sessionId,
    int generation,
  ) {
    if (_loadingSessionId != sessionId || _loadingGeneration != generation) {
      return current;
    }
    _sessionLoadBuffer.sort((a, b) {
      final aEvent = a.event;
      final bEvent = b.event;
      if (isLiveOnlyStudioEvent(aEvent) && isLiveOnlyStudioEvent(bEvent)) {
        return a.arrivalIndex.compareTo(b.arrivalIndex);
      }
      final aSequence = aEvent.sequence?.toInt() ?? 0;
      final bSequence = bEvent.sequence?.toInt() ?? 0;
      final sequence = switch ((aSequence > 0, bSequence > 0)) {
        (true, true) => aSequence.compareTo(bSequence),
        (true, false) => -1,
        (false, true) => 1,
        (false, false) => 0,
      };
      if (sequence != 0) {
        return sequence;
      }
      final createdAt = (aEvent.createdAt?.millisecondsSinceEpoch ?? 0)
          .compareTo(bEvent.createdAt?.millisecondsSinceEpoch ?? 0);
      if (createdAt != 0) {
        return createdAt;
      }
      return (aEvent.eventId ?? '').compareTo(bEvent.eventId ?? '');
    });
    var latest = current;
    for (final buffered in _sessionLoadBuffer) {
      final event = buffered.event;
      if (!targetsSelectedSession(latest, event) ||
          isDuplicateDurableEvent(latest, event)) {
        continue;
      }
      latest = _reduceEventWithCursor(latest, event);
    }
    final shouldRecover = _bufferedStaleSession;
    _clearAnySessionLoadBarrier();
    if (shouldRecover) {
      unawaited(_recoverStaleSession(sessionId));
    }
    return latest;
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

  Future<void> _reloadSession(String sessionId) async {
    final sessionState = await _api.loadSessionState(sessionId);
    final current = state.value;
    if (current == null || current.selectedSessionId != sessionId) {
      return;
    }
    state = AsyncData(mergeStudioSessionState(current, sessionState));
  }

  Future<void> _adoptState(StudioState next) async {
    final catalog = state.value?.providerCatalog;
    if (catalog != null) {
      next = _attachProviderCatalog(next, catalog);
    }
    _subscribe(next.selectedSessionId);
    state = AsyncData(next);
    final sessionId = next.selectedSessionId;
    if (sessionId == null) {
      return;
    }
    final generation = _sessionGeneration;
    _startSessionLoadBarrier(sessionId, generation);
    final withProjection = await _withSelectedSessionProjection(next);
    final current = state.value;
    if (current == null ||
        current.selectedSessionId != sessionId ||
        generation != _sessionGeneration) {
      _clearSessionLoadBarrier(sessionId, generation);
      return;
    }
    var merged = mergeStudioSessionState(current, withProjection);
    merged = _flushSessionLoadBuffer(merged, sessionId, generation);
    state = AsyncData(merged);
  }

  Future<StudioState> _withSelectedSessionProjection(StudioState next) async {
    final sessionId = next.selectedSessionId;
    if (sessionId == null) {
      return next;
    }
    final sessionState = await _api.loadSessionState(sessionId);
    return mergeStudioSessionState(next, sessionState);
  }

  Future<void> _recoverStaleSession(String sessionId) async {
    final current = state.value;
    final afterSequence = current?.eventCursorsBySession[sessionId];
    if (current == null || afterSequence == null || afterSequence <= 0) {
      await _reloadSession(sessionId);
      return;
    }
    final events = await _api.loadStudioEvents(
      sessionId,
      afterSequence: afterSequence,
      limit: 500,
    );
    final liveState = state.value;
    if (liveState == null || liveState.selectedSessionId != sessionId) {
      return;
    }
    var latest = liveState;
    if (events.isEmpty) {
      return;
    }
    for (final event in events) {
      if (!targetsSelectedSession(latest, event) ||
          isDuplicateDurableEvent(latest, event)) {
        continue;
      }
      latest = _reduceEventWithCursor(latest, event);
    }
    state = AsyncData(latest);
    if (events.length >= 500) {
      await _reloadSession(sessionId);
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
      unawaited(_recoverStaleSession(staleSessionId));
    }
    return reduced.state;
  }
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
