import 'dart:async';

import 'package:flutter/scheduler.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../domain/models/studio_models.dart';
import '../frb/studio_api.dart';

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
  StreamSubscription<Object>? _sessionSubscription;
  final List<StudioBridgeEvent> _pendingPartDeltas = [];
  bool _partDeltaFrameScheduled = false;
  final List<StudioBridgeEvent> _sessionLoadBuffer = [];
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
    final bootstrapped = await _api.bootstrap();
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
    var merged = _mergeSessionState(latest, sessionState);
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

  Future<void> setSessionMode(CompileMode mode) async {
    final current = state.value;
    final sessionId = current?.selectedSessionId;
    if (current == null ||
        sessionId == null ||
        current.isBusy ||
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
    state = AsyncData(_mergeSessionState(latest, next));
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
        effort ?? _defaultEffortForModel(current, providerId, model);
    final optimisticRole = RoleSettingsView(
      key: roleKey,
      providerId: providerId,
      model: model,
      effort: nextEffort,
    );
    state = AsyncData(
      current.copyWith(roles: _replaceRole(current.roles, optimisticRole)),
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
    state = AsyncData(_mergeConfigState(latest, next));
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
    state = AsyncData(_mergeConfigState(latest, next));
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
    state = AsyncData(_mergeConfigState(latest, next));
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
    final followUpPrompt = _planFollowUpPrompt(interaction, resolution);
    await _api.resolveInteraction(interaction.id, resolution);
    final latest = state.value ?? current;
    state = AsyncData(
      latest.copyWith(
        sessions: shouldImplementPlan
            ? [
                for (final session in latest.sessions)
                  session.id == interaction.sessionId
                      ? session.copyWith(mode: CompileMode.auto)
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
    if (event.payload is StalePayload) {
      final sessionId = event.sessionId ?? current.selectedSessionId;
      if (sessionId != null) {
        unawaited(_recoverStaleSession(sessionId));
      }
      return;
    }
    if (!_targetsSelectedSession(current, event) ||
        _isDuplicateDurableEvent(current, event)) {
      return;
    }
    final reduced = _reduceEvent(current, event);
    state = AsyncData(_withEventCursor(reduced, event));
  }

  void _handleSessionEvent(Object event, String sessionId, int generation) {
    if (generation != _sessionGeneration || event is! StudioBridgeEvent) {
      return;
    }
    final eventSessionId = _eventSessionId(event);
    if (eventSessionId != null && eventSessionId != sessionId) {
      return;
    }
    if (_loadingSessionId == sessionId && _loadingGeneration == generation) {
      if (event.payload is StalePayload) {
        _bufferedStaleSession = true;
      } else {
        _sessionLoadBuffer.add(event);
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
  }

  void _clearAnySessionLoadBarrier() {
    _loadingSessionId = null;
    _loadingGeneration = null;
    _bufferedStaleSession = false;
    _sessionLoadBuffer.clear();
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
      final aSequence = a.sequence?.toInt() ?? 0;
      final bSequence = b.sequence?.toInt() ?? 0;
      final sequence = switch ((aSequence > 0, bSequence > 0)) {
        (true, true) => aSequence.compareTo(bSequence),
        (true, false) => -1,
        (false, true) => 1,
        (false, false) => 0,
      };
      if (sequence != 0) {
        return sequence;
      }
      final createdAt = (a.createdAt?.millisecondsSinceEpoch ?? 0).compareTo(
        b.createdAt?.millisecondsSinceEpoch ?? 0,
      );
      if (createdAt != 0) {
        return createdAt;
      }
      return (a.eventId ?? '').compareTo(b.eventId ?? '');
    });
    var latest = current;
    for (final event in _sessionLoadBuffer) {
      if (!_targetsSelectedSession(latest, event)) {
        continue;
      }
      latest = _withEventCursor(_reduceEvent(latest, event), event);
    }
    final shouldRecover = _bufferedStaleSession;
    _clearAnySessionLoadBarrier();
    if (shouldRecover) {
      unawaited(_recoverStaleSession(sessionId));
    }
    return latest;
  }

  void _queuePartDelta(StudioState current, StudioBridgeEvent event) {
    if (!_targetsSelectedSession(current, event)) {
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
      if (!_targetsSelectedSession(latest, event)) {
        continue;
      }
      latest = _reduceEvent(latest, event);
    }
    state = AsyncData(latest);
  }

  Future<void> _reloadSession(String sessionId) async {
    final sessionState = await _api.loadSessionState(sessionId);
    final current = state.value;
    if (current == null || current.selectedSessionId != sessionId) {
      return;
    }
    state = AsyncData(_mergeSessionState(current, sessionState));
  }

  Future<void> _adoptState(StudioState next) async {
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
    var merged = _mergeSessionState(current, withProjection);
    merged = _flushSessionLoadBuffer(merged, sessionId, generation);
    state = AsyncData(merged);
  }

  Future<StudioState> _withSelectedSessionProjection(StudioState next) async {
    final sessionId = next.selectedSessionId;
    if (sessionId == null) {
      return next;
    }
    final sessionState = await _api.loadSessionState(sessionId);
    return _mergeSessionState(next, sessionState);
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
      if (!_targetsSelectedSession(latest, event) ||
          _isDuplicateDurableEvent(latest, event)) {
        continue;
      }
      latest = _withEventCursor(_reduceEvent(latest, event), event);
    }
    state = AsyncData(latest);
    if (events.length >= 500) {
      await _reloadSession(sessionId);
    }
  }

  StudioState _reduceEvent(StudioState current, StudioBridgeEvent event) {
    return switch (event.payload) {
      MessageUpdatedPayload(:final message) => _upsertMessageSnapshot(
        current,
        message,
      ),
      MessageRemovedPayload(:final messageId) => _removeMessage(
        current,
        event.sessionId,
        messageId,
      ),
      MessagePartUpdatedPayload(:final part) => _upsertPartSnapshot(
        current,
        part,
      ),
      MessagePartRemovedPayload(:final messageId, :final partId) => _removePart(
        current,
        event.sessionId,
        messageId,
        partId,
      ),
      MessagePartDeltaPayload(:final delta) => _appendPartDelta(current, delta),
      TurnChangedPayload(:final turn) => current.copyWith(
        turnPhase: _turnPhase(turn),
      ),
      InteractionChangedPayload(:final interaction, :final status) =>
        _upsertInteraction(current, interaction, status),
      SessionRuntimeChangedPayload(:final runtime) => current.copyWith(
        runtime: runtime.copyWith(agentCount: current.runtime.agentCount),
      ),
      SessionListChangedPayload() => _mergeSessionListChanged(current, event),
      AgentChangedPayload(:final agent) => _applyAgentChanged(current, agent),
      AgentTimelineChangedPayload(:final event) => _upsertAgentTimelineEvent(
        current,
        event,
      ),
      SkillActivatedPayload(:final name) => _applySkillActivation(
        current,
        name,
      ),
      McpHealthChangedPayload(:final activeMcpServers, :final servers) =>
        _applyMcpHealth(current, activeMcpServers, servers),
      LspHealthChangedPayload(:final activeLspServers) => _applyLspHealth(
        current,
        activeLspServers,
      ),
      PlanLifecycleChangedPayload(:final state) => _applyPlanLifecycle(
        current,
        state,
      ),
      StalePayload() ||
      IgnoredBridgeEventPayload() ||
      SettingsDraftSavedPayload() => current,
    };
  }

  StudioState _mergeSessionState(
    StudioState current,
    StudioState sessionState,
  ) {
    final sessionId = sessionState.selectedSessionId;
    var merged = current;
    if (sessionId != null) {
      for (final message
          in sessionState.messagesBySession[sessionId] ?? const []) {
        merged = _upsertMessageSnapshot(merged, message);
      }
      final snapshots = sessionState.partSnapshotsBySession[sessionId] ?? {};
      final orderedSnapshots = snapshots.values.toList()
        ..sort((a, b) {
          final order = a.order.compareTo(b.order);
          if (order != 0) {
            return order;
          }
          final sequence = a.sequence.compareTo(b.sequence);
          return sequence != 0 ? sequence : a.id.compareTo(b.id);
        });
      for (final snapshot in orderedSnapshots) {
        merged = _upsertPartSnapshot(merged, snapshot, recoverOnInvalid: false);
      }
      final agentEvents =
          sessionState.agentTimelineEventsBySession[sessionId] ?? const {};
      if (agentEvents.isNotEmpty) {
        merged = _withAgentTimelineEvents(merged, sessionId, {
          ...(merged.agentTimelineEventsBySession[sessionId] ?? const {}),
          ...agentEvents,
        });
      }
    }
    return merged.copyWith(
      sessions: sessionState.sessions.isEmpty
          ? merged.sessions
          : sessionState.sessions,
      selectedProjectId:
          sessionState.selectedProjectId ?? merged.selectedProjectId,
      selectedSessionId: sessionId ?? merged.selectedSessionId,
      runtime: sessionState.runtime,
      pendingInteractions: sessionState.pendingInteractions,
      eventCursorsBySession: _mergeEventCursors(
        merged.eventCursorsBySession,
        sessionState.eventCursorsBySession,
      ),
    );
  }

  StudioState _mergeConfigState(StudioState current, StudioState next) {
    return current.copyWith(
      providers: next.providers.isEmpty ? current.providers : next.providers,
      providerUsages: next.providerUsages.isEmpty
          ? current.providerUsages
          : next.providerUsages,
      roles: next.roles.isEmpty ? current.roles : next.roles,
      mcpServers: next.mcpServers.isEmpty
          ? current.mcpServers
          : next.mcpServers,
      instructions: next.instructions,
      skills: next.skills,
      general: next.general,
      permissionMode: next.permissionMode,
      runtime: next.runtime.model.isEmpty ? current.runtime : next.runtime,
    );
  }

  StudioState _mergeSessionListChanged(
    StudioState current,
    StudioBridgeEvent event,
  ) {
    final payload = event.payload;
    if (payload is! SessionListChangedPayload) {
      return current;
    }
    final incoming = payload.sessions;
    final projectId = payload.projectId;
    if (projectId == null && incoming.isEmpty) {
      return current.copyWith(sessions: const []);
    }
    final Set<String> projectIds = projectId == null
        ? incoming.map((session) => session.projectId).toSet()
        : {projectId};
    final sessions = [
      for (final session in current.sessions)
        if (!projectIds.contains(session.projectId)) session,
      ...incoming,
    ];
    return current.copyWith(sessions: sessions);
  }

  Map<String, int> _mergeEventCursors(
    Map<String, int> current,
    Map<String, int> incoming,
  ) {
    final merged = {...current};
    for (final entry in incoming.entries) {
      final existing = merged[entry.key] ?? 0;
      if (entry.value > existing) {
        merged[entry.key] = entry.value;
      }
    }
    return merged;
  }

  StudioState _upsertMessageSnapshot(
    StudioState current,
    TimelineMessage message,
  ) {
    if (message.id.isEmpty || message.sessionId.isEmpty) {
      return current;
    }
    final messages = _messagesFor(current, message.sessionId);
    final index = messages.indexWhere(
      (candidate) => candidate.id == message.id,
    );
    if (index >= 0) {
      final existing = messages[index];
      if (message.sequence > 0 && message.sequence < existing.sequence) {
        return current;
      }
      messages[index] = existing.copyWith(
        role: message.role,
        sequence: message.sequence > existing.sequence
            ? message.sequence
            : existing.sequence,
      );
    } else {
      messages.add(message);
    }
    messages.sort((a, b) => a.createdAt.compareTo(b.createdAt));
    return _withMessages(current, message.sessionId, messages);
  }

  StudioState _upsertPartSnapshot(
    StudioState current,
    TimelinePartSnapshot snapshot, {
    bool recoverOnInvalid = true,
  }) {
    final sessionId = snapshot.sessionId;
    if (snapshot.id.isEmpty ||
        snapshot.messageId.isEmpty ||
        sessionId.isEmpty) {
      return current;
    }
    final existingSnapshot =
        current.partSnapshotsBySession[sessionId]?[snapshot.id];
    if (existingSnapshot != null &&
        snapshot.sequence > 0 &&
        snapshot.sequence < existingSnapshot.sequence) {
      return current;
    }
    if (existingSnapshot != null &&
        !_canApplyPartSnapshot(existingSnapshot, snapshot)) {
      if (recoverOnInvalid) {
        unawaited(_recoverStaleSession(sessionId));
      }
      return current;
    }
    final snapshots = {
      ...(current.partSnapshotsBySession[sessionId] ?? const {}),
      snapshot.id: snapshot,
    };
    final overlays = {
      ...(current.partOverlaysBySession[sessionId] ?? const {}),
    };
    final currentOverlay = overlays[snapshot.id];
    if (currentOverlay != null &&
        _snapshotCoversOverlay(snapshot, currentOverlay)) {
      overlays.remove(snapshot.id);
    }
    final messages = _messagesFor(current, sessionId);
    final messageIndex = messages.indexWhere(
      (message) => message.id == snapshot.messageId,
    );
    if (messageIndex < 0) {
      return current;
    }
    return _withPartState(current, sessionId, snapshots, overlays);
  }

  StudioState _appendPartDelta(StudioState current, TimelinePartDelta delta) {
    if (delta.sessionId.isEmpty ||
        delta.messageId.isEmpty ||
        delta.partId.isEmpty ||
        delta.delta.isEmpty ||
        !_canAppendDeltaField(delta.field)) {
      return current;
    }
    final snapshot =
        current.partSnapshotsBySession[delta.sessionId]?[delta.partId];
    if (snapshot == null || _isTerminalPartStatus(snapshot.status)) {
      return current;
    }
    final currentOverlay =
        current.partOverlaysBySession[delta.sessionId]?[delta.partId] ??
        const TimelinePartOverlay();
    final lastRevision =
        currentOverlay.lastRevisions[delta.field] ?? snapshot.revision;
    if (delta.revision <= lastRevision) {
      return current;
    }
    if (delta.revision != lastRevision + 1) {
      final overlays = {
        ...(current.partOverlaysBySession[delta.sessionId] ?? const {}),
      }..remove(delta.partId);
      unawaited(_recoverStaleSession(delta.sessionId));
      return _withPartState(
        current,
        delta.sessionId,
        current.partSnapshotsBySession[delta.sessionId] ?? const {},
        overlays,
      );
    }
    if (delta.chunkIndex != null) {
      final previousChunk = currentOverlay.lastChunkIndexes[delta.field] ?? -1;
      if (delta.chunkIndex! <= previousChunk) {
        return current;
      }
    }
    final baseValue =
        currentOverlay.values[delta.field] ??
        _snapshotField(snapshot, delta.field);
    final nextOverlay = currentOverlay.append(
      field: delta.field,
      value: '$baseValue${delta.delta}',
      revision: delta.revision,
      chunkIndex: delta.chunkIndex,
    );
    final overlays = {
      ...(current.partOverlaysBySession[delta.sessionId] ?? const {}),
      delta.partId: nextOverlay,
    };
    return _withPartState(
      current,
      delta.sessionId,
      current.partSnapshotsBySession[delta.sessionId] ?? const {},
      overlays,
    );
  }

  StudioState _removeMessage(
    StudioState current,
    String? sessionId,
    String messageId,
  ) {
    if (sessionId == null || sessionId.isEmpty || messageId.isEmpty) {
      return current;
    }
    final messages = _messagesFor(
      current,
      sessionId,
    ).where((message) => message.id != messageId).toList();
    final snapshots = {
      ...(current.partSnapshotsBySession[sessionId] ?? const {}),
    }..removeWhere((_, part) => part.messageId == messageId);
    final overlays = {...(current.partOverlaysBySession[sessionId] ?? const {})}
      ..removeWhere((partId, _) => !snapshots.containsKey(partId));
    return _withMessageAndPartState(
      current,
      sessionId,
      messages,
      snapshots,
      overlays,
    );
  }

  StudioState _removePart(
    StudioState current,
    String? sessionId,
    String messageId,
    String partId,
  ) {
    if (sessionId == null ||
        sessionId.isEmpty ||
        messageId.isEmpty ||
        partId.isEmpty) {
      return current;
    }
    final snapshots = {
      ...(current.partSnapshotsBySession[sessionId] ?? const {}),
    }..remove(partId);
    final overlays = {...(current.partOverlaysBySession[sessionId] ?? const {})}
      ..remove(partId);
    return _withPartState(current, sessionId, snapshots, overlays);
  }

  StudioState _applyAgentChanged(StudioState current, StudioAgentView agent) {
    if (agent.sessionId.isNotEmpty &&
        agent.sessionId != current.selectedSessionId) {
      return current;
    }
    final nextCount = current.runtime.agentCount == 0
        ? 1
        : current.runtime.agentCount;
    return current.copyWith(
      runtime: current.runtime.copyWith(agentCount: nextCount),
    );
  }

  StudioState _upsertAgentTimelineEvent(
    StudioState current,
    TimelineAgentEvent event,
  ) {
    if (event.sessionId.isEmpty ||
        event.sessionId != current.selectedSessionId ||
        event.eventId.isEmpty) {
      return current;
    }
    return _withAgentTimelineEvent(current, event);
  }

  StudioState _applySkillActivation(StudioState current, String name) {
    if (name.isEmpty || current.runtime.activeSkills.contains(name)) {
      return current;
    }
    return current.copyWith(
      runtime: current.runtime.copyWith(
        activeSkills: [...current.runtime.activeSkills, name]..sort(),
      ),
    );
  }

  StudioState _applyMcpHealth(
    StudioState current,
    List<String> activeMcpServers,
    List<McpServerSettingsView> servers,
  ) {
    return current.copyWith(
      runtime: current.runtime.copyWith(activeMcpServers: activeMcpServers),
      mcpServers: servers.isEmpty ? current.mcpServers : servers,
    );
  }

  StudioState _applyLspHealth(
    StudioState current,
    List<String> activeLspServers,
  ) {
    return current.copyWith(
      runtime: current.runtime.copyWith(activeLspServers: activeLspServers),
    );
  }

  StudioState _applyPlanLifecycle(StudioState current, String planState) {
    return current.copyWith(
      turnPhase: switch (planState) {
        'pendingConfirmation' => TurnPhase.waitingForInteraction,
        'accepted' || 'implementing' => TurnPhase.runningTool,
        'implementationFailed' => TurnPhase.failed,
        'cancelled' => TurnPhase.cancelled,
        'implemented' ||
        'continuedPlanning' ||
        'dismissed' => TurnPhase.completed,
        _ => current.turnPhase,
      },
    );
  }

  StudioState _upsertInteraction(
    StudioState current,
    PendingInteraction interaction,
    String status,
  ) {
    if (interaction.id.isEmpty) {
      return current;
    }
    final interactions = [...current.pendingInteractions];
    final index = interactions.indexWhere(
      (candidate) => candidate.id == interaction.id,
    );
    if (status != 'pending') {
      if (index >= 0) {
        interactions.removeAt(index);
      }
      return current.copyWith(pendingInteractions: interactions);
    }
    if (index >= 0) {
      interactions[index] = interaction;
    } else {
      interactions.add(interaction);
    }
    return current.copyWith(pendingInteractions: interactions);
  }

  List<TimelineMessage> _messagesFor(StudioState state, String sessionId) {
    return [...(state.messagesBySession[sessionId] ?? const [])];
  }

  StudioState _withMessages(
    StudioState state,
    String sessionId,
    List<TimelineMessage> messages,
  ) {
    final bySession = Map<String, List<TimelineMessage>>.from(
      state.messagesBySession,
    );
    bySession[sessionId] = messages;
    return state.copyWith(messagesBySession: bySession);
  }

  StudioState _withPartState(
    StudioState state,
    String sessionId,
    Map<String, TimelinePartSnapshot> snapshots,
    Map<String, TimelinePartOverlay> overlays,
  ) {
    return state.copyWith(
      partSnapshotsBySession: {
        ...state.partSnapshotsBySession,
        sessionId: snapshots,
      },
      partOverlaysBySession: {
        ...state.partOverlaysBySession,
        sessionId: overlays,
      },
    );
  }

  StudioState _withMessageAndPartState(
    StudioState state,
    String sessionId,
    List<TimelineMessage> messages,
    Map<String, TimelinePartSnapshot> snapshots,
    Map<String, TimelinePartOverlay> overlays,
  ) {
    final withMessages = _withMessages(state, sessionId, messages);
    return _withPartState(withMessages, sessionId, snapshots, overlays);
  }

  StudioState _withAgentTimelineEvent(
    StudioState state,
    TimelineAgentEvent event,
  ) {
    final events = {
      ...(state.agentTimelineEventsBySession[event.sessionId] ?? const {}),
      event.eventId: event,
    };
    return _withAgentTimelineEvents(state, event.sessionId, events);
  }

  StudioState _withAgentTimelineEvents(
    StudioState state,
    String sessionId,
    Map<String, TimelineAgentEvent> events,
  ) {
    return state.copyWith(
      agentTimelineEventsBySession: {
        ...state.agentTimelineEventsBySession,
        sessionId: events,
      },
    );
  }

  TurnPhase _turnPhase(StudioTurnView turn) {
    return switch (turn.status) {
      'queued' => TurnPhase.queued,
      'contextLoading' => TurnPhase.contextLoading,
      'waitingForModel' => TurnPhase.waitingForModel,
      'streaming' => TurnPhase.streaming,
      'waitingForInteraction' => TurnPhase.waitingForInteraction,
      'runningTool' => TurnPhase.runningTool,
      'completed' => TurnPhase.completed,
      'failed' => TurnPhase.failed,
      'cancelled' => TurnPhase.cancelled,
      _ => TurnPhase.idle,
    };
  }

  String _planFollowUpPrompt(
    PendingInteraction interaction,
    Map<String, Object?> resolution,
  ) {
    final content = resolution['content']?.toString().trim() ?? '';
    if (content.isNotEmpty) {
      return content;
    }
    final reason = resolution['reason']?.toString().trim() ?? '';
    if (reason.isNotEmpty) {
      return reason;
    }
    return (interaction.payload['content'] ?? interaction.body)
        .toString()
        .trim();
  }

  bool _targetsSelectedSession(StudioState current, StudioBridgeEvent event) {
    final sessionId = _eventSessionId(event);
    return sessionId == null ||
        current.selectedSessionId == null ||
        sessionId == current.selectedSessionId;
  }

  bool _isDuplicateDurableEvent(StudioState current, StudioBridgeEvent event) {
    if (_isLiveOnlyEvent(event)) {
      return false;
    }
    final sessionId = _eventSessionId(event);
    final sequence = event.sequence?.toInt();
    if (sessionId == null || sequence == null || sequence <= 0) {
      return false;
    }
    return sequence <= (current.eventCursorsBySession[sessionId] ?? 0);
  }

  StudioState _withEventCursor(StudioState current, StudioBridgeEvent event) {
    if (_isLiveOnlyEvent(event)) {
      return current;
    }
    final sessionId = _eventSessionId(event);
    final sequence = event.sequence?.toInt();
    if (sessionId == null || sequence == null || sequence <= 0) {
      return current;
    }
    final existing = current.eventCursorsBySession[sessionId] ?? 0;
    if (sequence <= existing) {
      return current;
    }
    return current.copyWith(
      eventCursorsBySession: {
        ...current.eventCursorsBySession,
        sessionId: sequence,
      },
    );
  }

  bool _isLiveOnlyEvent(StudioBridgeEvent event) {
    return event.payload is MessagePartDeltaPayload ||
        event.payload is StalePayload;
  }

  String? _eventSessionId(StudioBridgeEvent event) {
    if (event.sessionId != null && event.sessionId!.isNotEmpty) {
      return event.sessionId;
    }
    return event.payload.sessionId;
  }

  List<RoleSettingsView> _replaceRole(
    List<RoleSettingsView> roles,
    RoleSettingsView replacement,
  ) {
    var replaced = false;
    final next = [
      for (final role in roles)
        if (role.key == replacement.key) ...[replacement] else role,
    ];
    replaced = roles.any((role) => role.key == replacement.key);
    if (!replaced) {
      next.add(replacement);
    }
    return next;
  }

  String _defaultEffortForModel(
    StudioState current,
    String providerId,
    String model,
  ) {
    for (final provider in current.providers) {
      if (provider.id != providerId) {
        continue;
      }
      for (final candidate in provider.models) {
        if (candidate.slug == model && candidate.reasoningEfforts.isNotEmpty) {
          return candidate.reasoningEfforts.first;
        }
      }
    }
    return current.role('planner')?.effort ?? 'high';
  }

  bool _canAppendDeltaField(String field) {
    return switch (field) {
      'text' ||
      'reasoning.summary' ||
      'planContent' ||
      'tool.arguments' ||
      'tool.result' => true,
      _ => false,
    };
  }

  String _snapshotField(TimelinePartSnapshot snapshot, String field) {
    return switch (field) {
      'text' => snapshot.text,
      'reasoning.summary' => snapshot.text,
      'planContent' => snapshot.planContent ?? snapshot.text,
      'tool.arguments' => snapshot.tool?.arguments ?? '',
      'tool.result' => snapshot.tool?.result ?? '',
      _ => '',
    };
  }

  bool _snapshotCoversOverlay(
    TimelinePartSnapshot snapshot,
    TimelinePartOverlay overlay,
  ) {
    if (_isTerminalPartStatus(snapshot.status)) {
      return true;
    }
    return overlay.lastRevisions.values.every(
      (revision) => revision <= snapshot.revision,
    );
  }

  bool _isTerminalPartStatus(String status) {
    return switch (status) {
      'completed' ||
      'failed' ||
      'interrupted' ||
      'cancelled' ||
      'denied' ||
      'budgetLimited' => true,
      _ => false,
    };
  }

  bool _canApplyPartSnapshot(
    TimelinePartSnapshot existing,
    TimelinePartSnapshot incoming,
  ) {
    if (!_samePartIdentity(existing, incoming)) {
      return false;
    }
    if (incoming.revision < existing.revision) {
      return false;
    }
    if (_isTerminalPartStatus(existing.status) &&
        !_samePartSnapshot(existing, incoming)) {
      return false;
    }
    return true;
  }

  bool _samePartIdentity(
    TimelinePartSnapshot existing,
    TimelinePartSnapshot incoming,
  ) {
    return existing.id == incoming.id &&
        existing.messageId == incoming.messageId &&
        existing.sessionId == incoming.sessionId &&
        existing.turnId == incoming.turnId &&
        existing.type == incoming.type &&
        existing.order == incoming.order &&
        existing.createdAt == incoming.createdAt &&
        existing.textChannel == incoming.textChannel;
  }

  bool _samePartSnapshot(
    TimelinePartSnapshot existing,
    TimelinePartSnapshot incoming,
  ) {
    return _samePartIdentity(existing, incoming) &&
        existing.revision == incoming.revision &&
        existing.text == incoming.text &&
        existing.status == incoming.status &&
        existing.updatedAt == incoming.updatedAt &&
        existing.completedAt == incoming.completedAt &&
        existing.error == incoming.error &&
        _sameToolPart(existing.tool, incoming.tool) &&
        _sameAgentPart(existing.agent, incoming.agent) &&
        existing.planContent == incoming.planContent &&
        existing.synthetic == incoming.synthetic &&
        existing.ignored == incoming.ignored;
  }

  bool _sameToolPart(TimelineToolPart? existing, TimelineToolPart? incoming) {
    if (existing == null || incoming == null) {
      return existing == incoming;
    }
    return existing.toolCallId == incoming.toolCallId &&
        existing.name == incoming.name &&
        existing.callId == incoming.callId &&
        existing.providerItemId == incoming.providerItemId &&
        existing.arguments == incoming.arguments &&
        existing.result == incoming.result &&
        existing.exitCode == incoming.exitCode &&
        existing.timedOut == incoming.timedOut &&
        existing.workingDirectory == incoming.workingDirectory &&
        existing.denialReason == incoming.denialReason;
  }

  bool _sameAgentPart(
    TimelineAgentPart? existing,
    TimelineAgentPart? incoming,
  ) {
    if (existing == null || incoming == null) {
      return existing == incoming;
    }
    return existing.id == incoming.id &&
        existing.path == incoming.path &&
        existing.parentPath == incoming.parentPath &&
        existing.role == incoming.role &&
        existing.task == incoming.task &&
        existing.status == incoming.status &&
        existing.summary == incoming.summary &&
        existing.depth == incoming.depth &&
        existing.error == incoming.error &&
        existing.reason == incoming.reason;
  }
}
