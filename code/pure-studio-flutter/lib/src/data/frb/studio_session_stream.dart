part of 'studio_api.dart';

sealed class SessionStreamFrame {
  const SessionStreamFrame();

  factory SessionStreamFrame.fromFrb(frb.BridgeSessionStreamFrame frame) {
    final json = _decodeJson(frame.payloadJson);
    return switch (_string(json['type'])) {
      'snapshot' => SessionSnapshotFrame(snapshot: _map(json['snapshot'])),
      'event' => SessionEventFrame(
        event: StudioBridgeEvent.fromCanonicalJson(_map(json['event'])),
      ),
      'resyncRequired' => SessionResyncRequiredFrame(
        reason: _map(json['reason']),
      ),
      final type => throw FormatException(
        'Unknown session stream frame: $type',
      ),
    };
  }
}

final class SessionSnapshotFrame extends SessionStreamFrame {
  const SessionSnapshotFrame({required this.snapshot});

  final Map<String, Object?> snapshot;
}

final class SessionEventFrame extends SessionStreamFrame {
  const SessionEventFrame({required this.event});

  final StudioBridgeEvent event;
}

final class SessionResyncRequiredFrame extends SessionStreamFrame {
  const SessionResyncRequiredFrame({required this.reason});

  final Map<String, Object?> reason;
}

StudioBridgeEventPayload _canonicalEventPayload(
  Map<String, Object?> kind, {
  required int sequence,
  required String sessionId,
}) {
  return switch (_string(kind['type'])) {
    'turnChanged' => switch (_map(kind['turn'])) {
      final turn => TurnChangedPayload(
        turn: StudioTurnView(
          sessionId: _string(turn['sessionId'], fallback: sessionId),
          status: _string(turn['status']),
        ),
      ),
    },
    'messageChanged' => MessageUpdatedPayload(
      message: timelineMessageFromJson(kind['message'], sequence: sequence),
    ),
    'messageRemoved' => MessageRemovedPayload(
      messageId: _string(kind['messageId']),
    ),
    'partChanged' => switch (_canonicalPartJson(kind['part'])) {
      final part when _isIgnoredTimelinePartType(part['type']) =>
        const IgnoredBridgeEventPayload(),
      final part => MessagePartUpdatedPayload(
        part: timelinePartSnapshotFromJson(part, sequence: sequence),
      ),
    },
    'partRemoved' => MessagePartRemovedPayload(
      messageId: _string(kind['messageId']),
      partId: _string(kind['partId']),
    ),
    'partDelta' => MessagePartDeltaPayload(
      delta: timelinePartDeltaFromJson(kind['delta']),
    ),
    'interactionChanged' => _canonicalInteractionPayload(kind['event']),
    'agentChanged' => AgentChangedPayload(
      agent: _canonicalAgentView(kind['agent']),
    ),
    'timelineEventAppended' => AgentTimelineChangedPayload(
      event: timelineAgentEventFromPayload(kind['event']),
    ),
    'runtimeChanged' => SessionRuntimeChangedPayload(
      runtime: sessionRuntimeFromJson(kind['runtime']),
      sessionId: sessionId,
      agentCount: _int(_map(kind['runtime'])['agentCount']),
    ),
    'skillActivated' => SkillActivatedPayload(
      name: _string(_map(kind['activation'])['name']),
    ),
    'planChanged' => PlanLifecycleChangedPayload(
      state: _string(_map(kind['event'])['state']),
    ),
    'contextCompacted' || 'errorOccurred' => const IgnoredBridgeEventPayload(),
    final type => throw FormatException('Unknown session event kind: $type'),
  };
}

Map<String, Object?> _canonicalPartJson(Object? value) {
  final part = _map(value);
  final content = _map(part['content']);
  final type = _string(content['type']);
  return {
    ...part,
    'type': type,
    'text': switch (type) {
      'text' || 'reasoning' => _string(content['text']),
      _ => '',
    },
    'textChannel': content['channel'],
    'tool': content['tool'],
    'agent': content['agent'],
    'planContent': content['content'],
    'activityGroupId': _map(content['tool'])['activityGroupId'],
  };
}

InteractionChangedPayload _canonicalInteractionPayload(Object? value) {
  final interaction = _map(_map(value)['interaction']);
  return InteractionChangedPayload(
    interaction: pendingInteractionFromJson(interaction),
    status: _string(interaction['status']),
  );
}

StudioAgentView _canonicalAgentView(Object? value) {
  final agent = _map(value);
  return StudioAgentView(
    id: _string(agent['id']),
    sessionId: _string(agent['sessionId']),
    path: _string(agent['path']),
    parentPath: _nullableString(agent['parentPath']),
    role: _string(agent['role'], fallback: 'agent'),
    task: _string(agent['task']),
    status: _string(agent['status']),
    summary: _nullableString(agent['summary']),
    depth: _int(agent['depth']),
    error: _nullableString(agent['error']),
    reason: _nullableString(agent['reason']),
    updatedAt: _dateFromUnix(_int(agent['updatedAt'])),
  );
}

StudioState applyCanonicalSessionSnapshot(
  StudioState current,
  Map<String, Object?> snapshot,
) {
  final sessionId = _string(snapshot['sessionId']);
  if (sessionId.isEmpty || current.selectedSessionId != sessionId) {
    return current;
  }
  final throughSequence = _int(snapshot['throughSequence']);
  final messages = _list(snapshot['messages'])
      .map((message) => timelineMessageFromJson(message))
      .where((message) => message.id.isNotEmpty)
      .toList();
  final parts = <String, TimelinePartSnapshot>{};
  for (final value in _list(snapshot['parts'])) {
    final partJson = _canonicalPartJson(value);
    if (_isIgnoredTimelinePartType(partJson['type'])) {
      continue;
    }
    final part = timelinePartSnapshotFromJson(partJson);
    if (part.id.isNotEmpty) {
      parts[part.id] = part;
    }
  }
  final interactions = <PendingInteraction>[];
  for (final value in _list(snapshot['interactions'])) {
    final json = _map(value);
    final interaction = pendingInteractionFromJson(json);
    if (interaction.id.isNotEmpty && _string(json['status']) == 'pending') {
      interactions.add(interaction);
    }
  }
  final agents = <String, StudioAgentView>{};
  for (final value in _list(snapshot['agents'])) {
    final agent = _canonicalAgentView(value);
    if (agent.id.isNotEmpty) {
      agents[agent.id] = agent;
    }
  }
  final timelineEvents = <String, TimelineAgentEvent>{};
  for (final value in _list(snapshot['timelineEvents'])) {
    final event = timelineAgentEventFromPayload(value);
    if (event.eventId.isNotEmpty) {
      timelineEvents[event.eventId] = event;
    }
  }
  final runtimeJson = _map(snapshot['runtime']);
  final runtime =
      (runtimeJson.isEmpty
              ? current.runtime
              : sessionRuntimeFromJson(
                  runtimeJson,
                ).copyWith(task: current.runtime.task))
          .copyWith(agentCount: agents.length);
  final turn = _map(snapshot['turn']);
  return current.copyWith(
    messagesBySession: {...current.messagesBySession, sessionId: messages},
    partSnapshotsBySession: {
      ...current.partSnapshotsBySession,
      sessionId: parts,
    },
    partOverlaysBySession: {
      ...current.partOverlaysBySession,
      sessionId: const {},
    },
    agentTimelineEventsBySession: {
      ...current.agentTimelineEventsBySession,
      sessionId: timelineEvents,
    },
    agentsBySession: {...current.agentsBySession, sessionId: agents},
    runtime: runtime,
    pendingInteractions: [
      for (final interaction in current.pendingInteractions)
        if (interaction.sessionId != sessionId) interaction,
      ...interactions,
    ],
    turnPhase: turn.isEmpty
        ? TurnPhase.idle
        : _turnPhaseFromStatus(_string(turn['status'])),
    eventCursorsBySession: {
      ...current.eventCursorsBySession,
      sessionId: throughSequence,
    },
  );
}
