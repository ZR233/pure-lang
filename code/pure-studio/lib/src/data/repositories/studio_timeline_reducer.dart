part of 'studio_state_reducer.dart';

StudioState _upsertMessageSnapshot(
  StudioState current,
  TimelineMessage message,
) {
  if (message.id.isEmpty || message.sessionId.isEmpty) {
    return current;
  }
  final messages = _messagesFor(current, message.sessionId);
  final index = messages.indexWhere((candidate) => candidate.id == message.id);
  if (index >= 0) {
    final existing = messages[index];
    if (message.sequence > 0 && message.sequence < existing.sequence) {
      return current;
    }
    messages[index] = existing.copyWith(
      turnId: message.turnId.isEmpty ? existing.turnId : message.turnId,
      role: message.role,
      status: message.status,
      updatedAt: message.updatedAt,
      completedAt: message.completedAt,
      error: message.error,
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

StudioReduceResult _upsertPartSnapshot(
  StudioState current,
  TimelinePartSnapshot snapshot, {
  bool recoverOnInvalid = true,
}) {
  final sessionId = snapshot.sessionId;
  if (snapshot.id.isEmpty || snapshot.messageId.isEmpty || sessionId.isEmpty) {
    return StudioReduceResult(current);
  }
  final existingSnapshot =
      current.partSnapshotsBySession[sessionId]?[snapshot.id];
  if (existingSnapshot != null &&
      snapshot.sequence > 0 &&
      snapshot.sequence < existingSnapshot.sequence) {
    return StudioReduceResult(current);
  }
  if (existingSnapshot != null &&
      !_canApplyPartSnapshot(existingSnapshot, snapshot)) {
    return StudioReduceResult(
      current,
      staleSessionId: recoverOnInvalid ? sessionId : null,
    );
  }
  final snapshots = {
    ...(current.partSnapshotsBySession[sessionId] ?? const {}),
    snapshot.id: snapshot,
  };
  final overlays = {...(current.partOverlaysBySession[sessionId] ?? const {})};
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
    return StudioReduceResult(current);
  }
  return StudioReduceResult(
    _withPartState(current, sessionId, snapshots, overlays),
  );
}

StudioReduceResult _appendPartDelta(
  StudioState current,
  String? eventSessionId,
  TimelinePartDelta delta,
) {
  final sessionId = eventSessionId ?? current.selectedSessionId;
  if (sessionId == null ||
      sessionId.isEmpty ||
      delta.partId.isEmpty ||
      delta.delta.isEmpty ||
      !_canAppendDeltaField(delta.field)) {
    return StudioReduceResult(current);
  }
  final snapshots = current.partSnapshotsBySession[sessionId] ?? const {};
  final snapshot = snapshots[delta.partId];
  if (snapshot == null || _isTerminalPartStatus(snapshot.status)) {
    return StudioReduceResult(current);
  }
  final currentOverlay =
      current.partOverlaysBySession[sessionId]?[delta.partId] ??
      const TimelinePartOverlay();
  final lastRevision =
      currentOverlay.lastRevisions[delta.field] ?? snapshot.revision;
  if (delta.revision <= lastRevision) {
    return StudioReduceResult(current);
  }
  if (delta.revision != lastRevision + 1) {
    final overlays = {...(current.partOverlaysBySession[sessionId] ?? const {})}
      ..remove(delta.partId);
    return StudioReduceResult(
      _withPartState(current, sessionId, snapshots, overlays),
      staleSessionId: sessionId,
    );
  }
  if (delta.chunkIndex != null) {
    final previousChunk = currentOverlay.lastChunkIndexes[delta.field] ?? -1;
    if (delta.chunkIndex! <= previousChunk) {
      return StudioReduceResult(current);
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
    ...(current.partOverlaysBySession[sessionId] ?? const {}),
    delta.partId: nextOverlay,
  };
  return StudioReduceResult(
    _withPartState(current, sessionId, snapshots, overlays),
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
  final snapshots = {...(current.partSnapshotsBySession[sessionId] ?? const {})}
    ..removeWhere((_, part) => part.messageId == messageId);
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
  final snapshots = {...(current.partSnapshotsBySession[sessionId] ?? const {})}
    ..remove(partId);
  final overlays = {...(current.partOverlaysBySession[sessionId] ?? const {})}
    ..remove(partId);
  return _withPartState(current, sessionId, snapshots, overlays);
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

bool _sameAgentPart(TimelineAgentPart? existing, TimelineAgentPart? incoming) {
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
