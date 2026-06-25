import 'dart:async';

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
  Timer? _partDeltaFlushTimer;
  final List<StudioBridgeEvent> _pendingPartDeltas = [];

  StudioApi get _api => ref.read(studioApiProvider);

  @override
  Future<StudioState> build() async {
    ref.onDispose(() {
      _partDeltaFlushTimer?.cancel();
      _pendingPartDeltas.clear();
      final global = _globalSubscription;
      final session = _sessionSubscription;
      if (global != null) {
        unawaited(global.cancel());
      }
      if (session != null) {
        unawaited(session.cancel());
      }
    });
    final bootstrapped = await _api.bootstrap();
    _subscribe(bootstrapped.selectedSessionId);
    return _withSelectedSessionProjection(bootstrapped);
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
    _globalSubscription ??= _api.subscribeGlobalEvents().listen(_handleEvent);
    final oldSession = _sessionSubscription;
    if (oldSession != null) {
      unawaited(oldSession.cancel());
    }
    _sessionSubscription = sessionId == null
        ? null
        : _api.subscribeSessionEvents(sessionId).listen(_handleEvent);
  }

  Future<void> selectSession(String sessionId) async {
    final current = state.value;
    if (current == null || current.selectedSessionId == sessionId) {
      return;
    }
    state = AsyncData(current.copyWith(selectedSessionId: sessionId));
    _subscribe(sessionId);
    final sessionState = await _api.loadSessionState(sessionId);
    final latest = state.value;
    if (latest == null || latest.selectedSessionId != sessionId) {
      return;
    }
    state = AsyncData(_mergeSessionState(latest, sessionState));
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
    if (event.kindType == 'messagePartDelta') {
      _queuePartDelta(current, event);
      return;
    }
    _flushPartDeltaBatch();
    if (event.kindType == 'stale') {
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

  void _queuePartDelta(StudioState current, StudioBridgeEvent event) {
    if (!_targetsSelectedSession(current, event)) {
      return;
    }
    _pendingPartDeltas.add(event);
    _partDeltaFlushTimer ??= Timer(Duration.zero, _flushPartDeltaBatch);
  }

  void _flushPartDeltaBatch() {
    if (_pendingPartDeltas.isEmpty) {
      _partDeltaFlushTimer?.cancel();
      _partDeltaFlushTimer = null;
      return;
    }
    final events = List<StudioBridgeEvent>.of(_pendingPartDeltas);
    _pendingPartDeltas.clear();
    _partDeltaFlushTimer?.cancel();
    _partDeltaFlushTimer = null;
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
    final withProjection = await _withSelectedSessionProjection(next);
    final current = state.value;
    if (current == null || current.selectedSessionId != sessionId) {
      return;
    }
    state = AsyncData(_mergeSessionState(current, withProjection));
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
    return switch (event.kindType) {
      'messageUpdated' => _upsertMessage(current, event.payload['message']),
      'messageRemoved' => _removeMessage(
        current,
        event.sessionId,
        event.payload['messageId'],
      ),
      'messagePartUpdated' => _upsertPart(current, event.payload['part']),
      'messagePartRemoved' => _removePart(
        current,
        event.sessionId,
        event.payload['messageId'],
        event.payload['partId'],
      ),
      'messagePartDelta' => _appendPartDelta(current, event.payload['delta']),
      'turnChanged' => current.copyWith(
        turnPhase: _turnPhase(event.payload['turn']),
      ),
      'interactionChanged' => _upsertInteraction(
        current,
        event.payload['event'],
      ),
      'sessionRuntimeChanged' => current.copyWith(
        runtime: sessionRuntimeFromJson(
          event.payload['runtime'],
        ).copyWith(agentCount: current.runtime.agentCount),
      ),
      'sessionListChanged' => _mergeSessionListChanged(current, event),
      'agentChanged' => _applyAgentChanged(current, event.payload['agent']),
      'agentTimelineChanged' => _upsertAgentTimelineEvent(
        current,
        event.payload['event'],
      ),
      'skillActivated' => _applySkillActivation(
        current,
        event.payload['activation'],
      ),
      'mcpHealthChanged' => _applyMcpHealth(current, event.payload['health']),
      'lspHealthChanged' => _applyLspHealth(current, event.payload['health']),
      'planLifecycleChanged' => _applyPlanLifecycle(
        current,
        event.payload['event'],
      ),
      _ => current,
    };
  }

  StudioState _mergeSessionState(
    StudioState current,
    StudioState sessionState,
  ) {
    final messagesBySession = Map<String, List<TimelineMessage>>.from(
      current.messagesBySession,
    );
    final snapshotsBySession =
        Map<String, Map<String, TimelinePartSnapshot>>.from(
          current.partSnapshotsBySession,
        );
    final sessionId = sessionState.selectedSessionId;
    if (sessionId != null) {
      messagesBySession[sessionId] =
          sessionState.messagesBySession[sessionId] ?? const [];
      snapshotsBySession[sessionId] =
          sessionState.partSnapshotsBySession[sessionId] ?? const {};
    }
    return current.copyWith(
      sessions: sessionState.sessions.isEmpty
          ? current.sessions
          : sessionState.sessions,
      messagesBySession: messagesBySession,
      partSnapshotsBySession: snapshotsBySession,
      partOverlaysBySession: {
        ...current.partOverlaysBySession,
        ?sessionId: const {},
      },
      selectedProjectId:
          sessionState.selectedProjectId ?? current.selectedProjectId,
      selectedSessionId: sessionId ?? current.selectedSessionId,
      runtime: sessionState.runtime,
      pendingInteractions: sessionState.pendingInteractions,
      eventCursorsBySession: {
        ...current.eventCursorsBySession,
        ...sessionState.eventCursorsBySession,
      },
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
    final incoming = studioSessionsFromJson(event.payload['sessions']);
    final projectId = _emptyToNull(
      event.payload['projectId']?.toString() ?? '',
    );
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

  StudioState _upsertMessage(StudioState current, Object? value) {
    final message = timelineMessageFromJson(value);
    if (message.id.isEmpty || message.sessionId.isEmpty) {
      return current;
    }
    final messages = _messagesFor(current, message.sessionId);
    final index = messages.indexWhere(
      (candidate) => candidate.id == message.id,
    );
    if (index >= 0) {
      final existing = messages[index];
      messages[index] = TimelineMessage(
        id: message.id,
        sessionId: message.sessionId,
        role: message.role,
        createdAt: message.createdAt,
        parts: existing.parts,
      );
    } else {
      messages.add(message);
    }
    messages.sort((a, b) => a.createdAt.compareTo(b.createdAt));
    return _withMessages(current, message.sessionId, messages);
  }

  StudioState _upsertPart(StudioState current, Object? value) {
    final snapshot = timelinePartSnapshotFromJson(value);
    final sessionId = snapshot.sessionId;
    if (snapshot.id.isEmpty ||
        snapshot.messageId.isEmpty ||
        sessionId.isEmpty) {
      return current;
    }
    final snapshots = {
      ...(current.partSnapshotsBySession[sessionId] ?? const {}),
      snapshot.id: snapshot,
    };
    final overlays = {...(current.partOverlaysBySession[sessionId] ?? const {})}
      ..remove(snapshot.id);
    final messages = _messagesFor(current, sessionId);
    final messageIndex = messages.indexWhere(
      (message) => message.id == snapshot.messageId,
    );
    final projected = timelinePartFromSnapshot(snapshot);
    if (messageIndex < 0) {
      messages.add(
        TimelineMessage(
          id: snapshot.messageId,
          sessionId: sessionId,
          role: 'assistant',
          createdAt: snapshot.createdAt,
          parts: [projected],
        ),
      );
      return _withTimelineState(
        current,
        sessionId,
        messages,
        snapshots,
        overlays,
      );
    }
    final message = messages[messageIndex];
    final parts = [...message.parts];
    final partIndex = parts.indexWhere(
      (candidate) => candidate.id == snapshot.id,
    );
    if (partIndex >= 0) {
      parts[partIndex] = projected;
    } else {
      parts.add(projected);
    }
    parts.sort((a, b) {
      final order = a.order.compareTo(b.order);
      return order != 0 ? order : a.id.compareTo(b.id);
    });
    messages[messageIndex] = message.copyWith(parts: parts);
    return _withTimelineState(
      current,
      sessionId,
      messages,
      snapshots,
      overlays,
    );
  }

  StudioState _appendPartDelta(StudioState current, Object? value) {
    final delta = timelinePartDeltaFromJson(value);
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
    if (delta.revision > 0 && delta.revision <= lastRevision) {
      return current;
    }
    if (delta.chunkIndex != null) {
      final previousChunk = currentOverlay.lastChunkIndexes[delta.field] ?? -1;
      if (delta.chunkIndex! < previousChunk) {
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
    final messages = _messagesFor(current, delta.sessionId);
    final messageIndex = messages.indexWhere(
      (message) => message.id == delta.messageId,
    );
    if (messageIndex < 0) {
      return current;
    }
    final message = messages[messageIndex];
    final parts = [...message.parts];
    final partIndex = parts.indexWhere((part) => part.id == delta.partId);
    if (partIndex < 0) {
      return current;
    }
    parts[partIndex] = timelinePartFromSnapshot(snapshot, overlay: nextOverlay);
    messages[messageIndex] = message.copyWith(parts: parts);
    return _withTimelineState(
      current,
      delta.sessionId,
      messages,
      current.partSnapshotsBySession[delta.sessionId] ?? const {},
      overlays,
    );
  }

  StudioState _removeMessage(
    StudioState current,
    String? sessionId,
    Object? messageIdValue,
  ) {
    final messageId = messageIdValue?.toString() ?? '';
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
    return _withTimelineState(
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
    Object? messageIdValue,
    Object? partIdValue,
  ) {
    final messageId = messageIdValue?.toString() ?? '';
    final partId = partIdValue?.toString() ?? '';
    if (sessionId == null ||
        sessionId.isEmpty ||
        messageId.isEmpty ||
        partId.isEmpty) {
      return current;
    }
    final messages = _messagesFor(current, sessionId);
    final messageIndex = messages.indexWhere(
      (message) => message.id == messageId,
    );
    if (messageIndex < 0) {
      return current;
    }
    final message = messages[messageIndex];
    messages[messageIndex] = message.copyWith(
      parts: message.parts.where((part) => part.id != partId).toList(),
    );
    final snapshots = {
      ...(current.partSnapshotsBySession[sessionId] ?? const {}),
    }..remove(partId);
    final overlays = {...(current.partOverlaysBySession[sessionId] ?? const {})}
      ..remove(partId);
    return _withTimelineState(
      current,
      sessionId,
      messages,
      snapshots,
      overlays,
    );
  }

  StudioState _applyAgentChanged(StudioState current, Object? value) {
    final sessionId = _stringFromMap(value, 'sessionId');
    if (sessionId.isNotEmpty && sessionId != current.selectedSessionId) {
      return current;
    }
    final nextCount = current.runtime.agentCount == 0
        ? 1
        : current.runtime.agentCount;
    return current.copyWith(
      runtime: current.runtime.copyWith(agentCount: nextCount),
    );
  }

  StudioState _upsertAgentTimelineEvent(StudioState current, Object? value) {
    final sessionId = _stringFromMap(value, 'sessionId');
    final eventId = _stringFromMap(value, 'eventId');
    if (sessionId.isEmpty ||
        sessionId != current.selectedSessionId ||
        eventId.isEmpty) {
      return current;
    }
    final kind = _nested(value, 'kind');
    final kindType = _stringFromMap(kind, 'type');
    final callId = _stringFromMap(kind, 'callId');
    final createdAt = _intFromMap(value, 'createdAt');
    final messageId = 'agent-timeline:${callId.isEmpty ? eventId : callId}';
    final part = TimelinePart(
      id: '$messageId:part',
      messageId: messageId,
      type: TimelinePartType.agent,
      order: createdAt,
      revision: 0,
      title: _agentActivityTitle(kindType),
      text: _agentActivityText(kind),
      status: _stringFromMap(kind, 'status', fallback: 'completed'),
    );
    final messages = _messagesFor(current, sessionId);
    final index = messages.indexWhere((message) => message.id == messageId);
    final message = TimelineMessage(
      id: messageId,
      sessionId: sessionId,
      role: 'assistant',
      createdAt: DateTime.fromMillisecondsSinceEpoch(createdAt * 1000),
      parts: [part],
    );
    if (index >= 0) {
      messages[index] = message;
    } else {
      messages.add(message);
    }
    messages.sort((a, b) => a.createdAt.compareTo(b.createdAt));
    return _withMessages(current, sessionId, messages);
  }

  StudioState _applySkillActivation(StudioState current, Object? value) {
    final name = _stringFromMap(value, 'name');
    if (name.isEmpty || current.runtime.activeSkills.contains(name)) {
      return current;
    }
    return current.copyWith(
      runtime: current.runtime.copyWith(
        activeSkills: [...current.runtime.activeSkills, name]..sort(),
      ),
    );
  }

  StudioState _applyMcpHealth(StudioState current, Object? value) {
    final health = _map(value);
    final active = _stringList(
      health['activeMcpServers'] ?? health['active_mcp_servers'],
    );
    final servers = _list(health['mcpServers'] ?? health['mcp_servers'])
        .map(_mcpServerFromHealth)
        .where((server) => server.id.isNotEmpty)
        .toList();
    return current.copyWith(
      runtime: current.runtime.copyWith(activeMcpServers: active),
      mcpServers: servers.isEmpty ? current.mcpServers : servers,
    );
  }

  StudioState _applyLspHealth(StudioState current, Object? value) {
    final health = _map(value);
    final active = _stringList(
      health['activeLspServers'] ?? health['active_lsp_servers'],
    );
    return current.copyWith(
      runtime: current.runtime.copyWith(activeLspServers: active),
    );
  }

  StudioState _applyPlanLifecycle(StudioState current, Object? value) {
    return current.copyWith(
      turnPhase: switch (_stringFromMap(value, 'state')) {
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

  StudioState _upsertInteraction(StudioState current, Object? value) {
    final interaction = pendingInteractionFromJson(
      _nested(value, 'interaction'),
    );
    if (interaction.id.isEmpty) {
      return current;
    }
    final interactions = [...current.pendingInteractions];
    final index = interactions.indexWhere(
      (candidate) => candidate.id == interaction.id,
    );
    if (_stringFromMap(_nested(value, 'interaction'), 'status') != 'pending') {
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

  String _agentActivityTitle(String kindType) {
    return switch (kindType) {
      'spawnBegin' || 'spawnEnd' => 'agentTimeline.spawn',
      'interactionBegin' || 'interactionEnd' => 'agentTimeline.message',
      'waitingBegin' || 'waitingEnd' => 'agentTimeline.waiting',
      'closeBegin' || 'closeEnd' => 'agentTimeline.close',
      _ => 'agentTimeline.agent',
    };
  }

  String _agentActivityText(Object? value) {
    final kind = _map(value);
    final receiver = _stringFromMap(kind, 'receiverPath');
    final path = _stringFromMap(kind, 'path', fallback: receiver);
    final task = _stringFromMap(kind, 'taskName');
    final prompt = _stringFromMap(kind, 'prompt');
    final error = _stringFromMap(kind, 'error');
    return [
      path,
      task,
      prompt,
      error,
    ].where((part) => part.trim().isNotEmpty).join('\n');
  }

  StudioState _withTimelineState(
    StudioState state,
    String sessionId,
    List<TimelineMessage> messages,
    Map<String, TimelinePartSnapshot> snapshots,
    Map<String, TimelinePartOverlay> overlays,
  ) {
    final bySession = Map<String, List<TimelineMessage>>.from(
      state.messagesBySession,
    );
    bySession[sessionId] = messages;
    return state.copyWith(
      messagesBySession: bySession,
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

  TurnPhase _turnPhase(Object? value) {
    return switch (_stringFromMap(value, 'status')) {
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
    if (event.kindType == 'messagePartDelta') {
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
    if (event.kindType == 'messagePartDelta') {
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

  String? _eventSessionId(StudioBridgeEvent event) {
    if (event.sessionId != null && event.sessionId!.isNotEmpty) {
      return event.sessionId;
    }
    return switch (event.kindType) {
      'messageUpdated' => _emptyToNull(
        _stringFromMap(event.payload['message'], 'sessionId'),
      ),
      'messagePartUpdated' => _emptyToNull(
        _stringFromMap(event.payload['part'], 'sessionId'),
      ),
      'messagePartDelta' => _emptyToNull(
        _stringFromMap(event.payload['delta'], 'sessionId'),
      ),
      'turnChanged' => _emptyToNull(
        _stringFromMap(event.payload['turn'], 'sessionId'),
      ),
      'interactionChanged' => _emptyToNull(
        _stringFromMap(
          _nested(event.payload['event'], 'interaction'),
          'sessionId',
        ),
      ),
      'agentChanged' => _emptyToNull(
        _stringFromMap(event.payload['agent'], 'sessionId'),
      ),
      'agentTimelineChanged' => _emptyToNull(
        _stringFromMap(event.payload['event'], 'sessionId'),
      ),
      'sessionRuntimeChanged' => _emptyToNull(
        _stringFromMap(event.payload['runtime'], 'sessionId'),
      ),
      _ => null,
    };
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
      'text' || 'planContent' || 'tool.arguments' || 'tool.result' => true,
      _ => false,
    };
  }

  String _snapshotField(TimelinePartSnapshot snapshot, String field) {
    return switch (field) {
      'text' => snapshot.text,
      'planContent' => snapshot.planContent ?? snapshot.text,
      'tool.arguments' => snapshot.tool?.arguments ?? '',
      'tool.result' => snapshot.tool?.result ?? '',
      _ => '',
    };
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

  McpServerSettingsView _mcpServerFromHealth(Object? value) {
    final json = _map(value);
    final command = _stringFromMap(json, 'command');
    final url = _stringFromMap(json, 'url');
    final endpoint = _stringFromMap(json, 'endpoint');
    return McpServerSettingsView(
      id: _stringFromMap(json, 'id'),
      transport: _stringFromMap(json, 'transport'),
      endpoint: endpoint.isNotEmpty ? endpoint : (url.isEmpty ? command : url),
      enabled: _boolFromMap(json, 'enabled'),
      status: _stringFromMap(json, 'statusKind').isEmpty
          ? _stringFromMap(json, 'availabilityKind')
          : _stringFromMap(json, 'statusKind'),
    );
  }

  Map<String, Object?> _map(Object? value) {
    if (value is Map<String, Object?>) {
      return value;
    }
    if (value is Map) {
      return value.map((key, value) => MapEntry(key.toString(), value));
    }
    return const {};
  }

  List<Object?> _list(Object? value) {
    if (value is List) {
      return value.cast<Object?>();
    }
    return const [];
  }

  List<String> _stringList(Object? value) {
    return _list(value)
        .map((item) => item?.toString() ?? '')
        .where((item) => item.isNotEmpty)
        .toList();
  }

  bool _boolFromMap(Object? value, String key) {
    final nested = _nested(value, key);
    if (nested is bool) {
      return nested;
    }
    return nested?.toString() == 'true';
  }

  int _intFromMap(Object? value, String key) {
    final nested = _nested(value, key);
    if (nested is int) {
      return nested;
    }
    return int.tryParse(nested?.toString() ?? '') ?? 0;
  }

  String? _emptyToNull(String value) {
    return value.isEmpty ? null : value;
  }

  Object? _nested(Object? value, String key) {
    if (value is Map<String, Object?>) {
      return value[key];
    }
    if (value is Map) {
      return value[key];
    }
    return null;
  }

  String _stringFromMap(Object? value, String key, {String fallback = ''}) {
    final nested = _nested(value, key);
    if (nested == null) {
      return fallback;
    }
    return nested is String ? nested : nested.toString();
  }
}
