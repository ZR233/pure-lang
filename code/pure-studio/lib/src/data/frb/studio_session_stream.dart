part of 'studio_api.dart';

sealed class SessionStreamFrame {
  const SessionStreamFrame();

  factory SessionStreamFrame.fromFrb(frb.BridgeSessionStreamFrame frame) {
    return frame.when(
      snapshot: (snapshot) =>
          SessionSnapshotFrame(snapshot: _sessionSnapshotFromFrb(snapshot)),
      event: (event) => SessionEventFrame(event: _sessionEventFromFrb(event)),
      resyncRequired: (reason) => SessionResyncRequiredFrame(reason: reason),
    );
  }
}

class SessionHistoryPage {
  const SessionHistoryPage({
    required this.turns,
    required this.nextBeforeTurnSequence,
    required this.hasMore,
  });

  final List<SessionHistoryTurn> turns;
  final int? nextBeforeTurnSequence;
  final bool hasMore;
}

class SessionHistoryTurn {
  const SessionHistoryTurn({
    required this.turnSequence,
    required this.turnId,
    required this.status,
    required this.modelJson,
    required this.errorJson,
    required this.startedAt,
    required this.completedAt,
    required this.items,
  });

  final int turnSequence;
  final String turnId;
  final String status;
  final String? modelJson;
  final String? errorJson;
  final DateTime startedAt;
  final DateTime? completedAt;
  final List<SessionHistoryItem> items;
}

class SessionHistoryItem {
  const SessionHistoryItem({
    required this.sequence,
    required this.itemId,
    required this.turnId,
    required this.itemKind,
    required this.event,
    required this.createdAt,
  });

  final int sequence;
  final String itemId;
  final String turnId;
  final String itemKind;
  final StudioBridgeEvent event;
  final DateTime createdAt;
}

final class SessionSnapshotFrame extends SessionStreamFrame {
  const SessionSnapshotFrame({required this.snapshot});

  final StudioSessionSnapshot snapshot;
}

final class SessionEventFrame extends SessionStreamFrame {
  const SessionEventFrame({required this.event});

  final StudioBridgeEvent event;
}

final class SessionResyncRequiredFrame extends SessionStreamFrame {
  const SessionResyncRequiredFrame({required this.reason});

  final frb.BridgeSessionResyncReason reason;
}

class StudioSessionSnapshot {
  const StudioSessionSnapshot({
    required this.sessionId,
    required this.throughSequence,
    required this.messages,
    required this.parts,
    required this.interactions,
    required this.agents,
    required this.timelineEvents,
    required this.runtime,
    required this.turn,
  });

  final String sessionId;
  final int throughSequence;
  final List<TimelineMessage> messages;
  final Map<String, TimelinePartSnapshot> parts;
  final List<PendingInteraction> interactions;
  final Map<String, StudioAgentView> agents;
  final Map<String, TimelineAgentEvent> timelineEvents;
  final SessionRuntimeView? runtime;
  final StudioTurnView? turn;
}

StudioSessionSnapshot _sessionSnapshotFromFrb(
  frb.BridgeSessionViewSnapshot snapshot,
) {
  final parts = <String, TimelinePartSnapshot>{};
  for (final value in snapshot.parts) {
    final part = _timelinePartFromFrb(value);
    if (!_isIgnoredTimelinePartType(part.type) && part.id.isNotEmpty) {
      parts[part.id] = part;
    }
  }
  final agents = <String, StudioAgentView>{};
  for (final value in snapshot.agents) {
    final agent = _agentFromFrb(value);
    if (agent.id.isNotEmpty) {
      agents[agent.id] = agent;
    }
  }
  final timelineEvents = <String, TimelineAgentEvent>{};
  for (final value in snapshot.timelineEvents) {
    final event = _timelineEventFromFrb(value);
    if (event.eventId.isNotEmpty) {
      timelineEvents[event.eventId] = event;
    }
  }
  return StudioSessionSnapshot(
    sessionId: snapshot.sessionId,
    throughSequence: snapshot.throughSequence.toInt(),
    messages: snapshot.messages.map(_timelineMessageFromFrb).toList(),
    parts: parts,
    interactions: snapshot.interactions
        .where(
          (interaction) =>
              interaction.status == frb.BridgeInteractionStatus.pending,
        )
        .map(_interactionFromFrb)
        .toList(),
    agents: agents,
    timelineEvents: timelineEvents,
    runtime: snapshot.runtime == null
        ? null
        : _sessionRuntimeFromFrb(snapshot.runtime!),
    turn: snapshot.turn == null ? null : _turnFromFrb(snapshot.turn!),
  );
}

StudioBridgeEvent _sessionEventFromFrb(frb.BridgeSessionEventEnvelope event) {
  final sequence = event.position.when(
    durable: (sequence) => sequence,
    transient: (_) => null,
  );
  return StudioBridgeEvent(
    eventId: event.eventId,
    sessionId: event.sessionId,
    turnId: event.turnId,
    sequence: sequence,
    createdAt: _dateFromUnix(event.emittedAt),
    payload: event.kind.when(
      turnChanged: (turn) => TurnChangedPayload(turn: _turnFromFrb(turn)),
      messageChanged: (message) => MessageUpdatedPayload(
        message: _timelineMessageFromFrb(
          message,
          sequence: sequence?.toInt() ?? 0,
        ),
      ),
      messageRemoved: (messageId) =>
          MessageRemovedPayload(messageId: messageId),
      partChanged: (part) {
        final converted = _timelinePartFromFrb(
          part,
          sequence: sequence?.toInt() ?? 0,
        );
        return _isIgnoredTimelinePartType(converted.type)
            ? const IgnoredBridgeEventPayload()
            : MessagePartUpdatedPayload(part: converted);
      },
      partRemoved: (messageId, partId) =>
          MessagePartRemovedPayload(messageId: messageId, partId: partId),
      partDelta: (delta) =>
          MessagePartDeltaPayload(delta: _timelineDeltaFromFrb(delta)),
      interactionChanged: (interaction) => InteractionChangedPayload(
        interaction: _interactionFromFrb(interaction),
        status: interaction.status.name,
      ),
      agentChanged: (agent) => AgentChangedPayload(agent: _agentFromFrb(agent)),
      timelineEventAppended: (timelineEvent) => AgentTimelineChangedPayload(
        event: _timelineEventFromFrb(timelineEvent),
      ),
      runtimeChanged: (runtime) => SessionRuntimeChangedPayload(
        runtime: _sessionRuntimeFromFrb(runtime),
        sessionId: runtime.sessionId,
        agentCount: runtime.agentCount,
      ),
      skillActivated: (activation) =>
          SkillActivatedPayload(name: activation.name),
      planChanged: (planEvent) =>
          PlanLifecycleChangedPayload(state: planEvent.state.name),
      contextCompacted: (_) => const IgnoredBridgeEventPayload(),
      errorOccurred: (_, _) => const IgnoredBridgeEventPayload(),
    ),
  );
}

TimelineMessage _timelineMessageFromFrb(
  frb.BridgeSessionMessage value, {
  int sequence = 0,
}) {
  JsonLeafDecoder.decodeObject(value.metadataJson);
  return TimelineMessage(
    id: value.messageId,
    sessionId: value.sessionId,
    turnId: value.turnId,
    role: value.role.name,
    status: value.status.name,
    createdAt: _dateFromUnix(value.createdAt),
    updatedAt: _dateFromUnix(value.updatedAt),
    completedAt: value.completedAt == null
        ? null
        : _dateFromUnix(value.completedAt!),
    error: value.error,
    sequence: sequence,
  );
}

TimelinePartSnapshot _timelinePartFromFrb(
  frb.BridgeSessionPart value, {
  int sequence = 0,
}) {
  return value.content.when(
    text: (channel, text, _) => _partSnapshot(
      value,
      type: TimelinePartType.text,
      text: text,
      textChannel: switch (channel) {
        frb.BridgeSessionTextChannel.user => TimelineTextChannel.user,
        frb.BridgeSessionTextChannel.commentary =>
          TimelineTextChannel.commentary,
        frb.BridgeSessionTextChannel.final_ => TimelineTextChannel.finalAnswer,
      },
      sequence: sequence,
    ),
    reasoning: (summary, content) => _partSnapshot(
      value,
      type: TimelinePartType.reasoning,
      text: '',
      reasoningSummary: summary,
      reasoningContent: content,
      sequence: sequence,
    ),
    tool: (tool) => _partSnapshot(
      value,
      type: TimelinePartType.tool,
      text: '',
      tool: TimelineToolPart(
        toolCallId: tool.toolCallId,
        callId: tool.callId,
        providerItemId: tool.providerItemId,
        name: tool.name,
        arguments: tool.argumentsJson,
        result: tool.result,
        outputArtifacts: tool.outputArtifactsJson
            .map(JsonLeafDecoder.decode)
            .toList(),
        exitCode: tool.exitCode,
        timedOut: tool.timedOut,
        workingDirectory: tool.workingDirectory,
        denialReason: tool.denialReason,
      ),
      sequence: sequence,
    ),
    agent: (agent) => _partSnapshot(
      value,
      type: TimelinePartType.agent,
      text: '',
      agent: TimelineAgentPart(
        id: agent.id,
        path: agent.path,
        parentPath: agent.parentPath,
        role: agent.role,
        task: agent.task,
        status: agent.status.name,
        summary: agent.summary,
        depth: agent.depth,
        error: agent.error,
        reason: agent.reason,
      ),
      sequence: sequence,
    ),
    turn: () => _partSnapshot(
      value,
      type: TimelinePartType.turn,
      text: '',
      sequence: sequence,
    ),
    inference: (_, _) => _partSnapshot(
      value,
      type: TimelinePartType.inference,
      text: '',
      sequence: sequence,
    ),
    plan: (content) => _partSnapshot(
      value,
      type: TimelinePartType.plan,
      text: '',
      planContent: content,
      sequence: sequence,
    ),
    file: (_, _) => _partSnapshot(
      value,
      type: TimelinePartType.file,
      text: '',
      sequence: sequence,
    ),
  );
}

TimelinePartSnapshot _partSnapshot(
  frb.BridgeSessionPart value, {
  required TimelinePartType type,
  required String text,
  required int sequence,
  TimelineTextChannel? textChannel,
  TimelineToolPart? tool,
  TimelineAgentPart? agent,
  String? planContent,
  List<String> reasoningSummary = const [],
  List<String> reasoningContent = const [],
}) {
  return TimelinePartSnapshot(
    id: value.partId,
    messageId: value.messageId,
    sessionId: value.sessionId,
    turnId: value.turnId,
    type: type,
    order: value.order.toInt(),
    revision: value.revision.toInt(),
    sequence: sequence,
    text: text,
    status: value.status.name,
    createdAt: _dateFromUnix(value.createdAt),
    updatedAt: _dateFromUnix(value.updatedAt),
    completedAt: value.completedAt == null
        ? null
        : _dateFromUnix(value.completedAt!),
    error: value.error,
    textChannel: textChannel,
    tool: tool,
    agent: agent,
    planContent: planContent,
    reasoningSummary: reasoningSummary,
    reasoningContent: reasoningContent,
    synthetic: value.synthetic,
    ignored: value.ignored,
  );
}

TimelinePartDelta _timelineDeltaFromFrb(frb.BridgeSessionPartDelta value) {
  final field = switch (value.field) {
    frb.BridgeSessionPartDeltaField.text => 'text',
    frb.BridgeSessionPartDeltaField.reasoningSummary => 'reasoning.summary',
    frb.BridgeSessionPartDeltaField.reasoningContent => 'reasoning.content',
    frb.BridgeSessionPartDeltaField.planContent => 'planContent',
    frb.BridgeSessionPartDeltaField.toolArguments => 'tool.arguments',
    frb.BridgeSessionPartDeltaField.toolResult => 'tool.result',
  };
  return TimelinePartDelta(
    partId: value.partId,
    revision: value.revision.toInt(),
    field: field,
    delta: value.delta,
    chunkIndex: value.chunkIndex,
  );
}

StudioTurnView _turnFromFrb(frb.BridgeSessionTurn value) {
  return StudioTurnView(
    turnId: value.turnId,
    sessionId: value.sessionId,
    state: value.state.when(
      queued: () => const StudioTurnState.queued(),
      inProgress: (activity) =>
          StudioTurnState.inProgress(_turnActivityFromFrb(activity)),
      completed: () => const StudioTurnState.completed(),
      failed: (reason) => StudioTurnState.failed(reason),
      cancelled: (reason) => StudioTurnState.cancelled(reason),
    ),
    updatedAt: _dateFromUnix(value.updatedAt),
  );
}

StudioTurnActivity _turnActivityFromFrb(
  frb.BridgeSessionTurnActivity activity,
) {
  return switch (activity) {
    frb.BridgeSessionTurnActivity.preparing => StudioTurnActivity.preparing,
    frb.BridgeSessionTurnActivity.thinking => StudioTurnActivity.thinking,
    frb.BridgeSessionTurnActivity.responding => StudioTurnActivity.responding,
    frb.BridgeSessionTurnActivity.planning => StudioTurnActivity.planning,
    frb.BridgeSessionTurnActivity.runningTool => StudioTurnActivity.runningTool,
    frb.BridgeSessionTurnActivity.waitingForApproval =>
      StudioTurnActivity.waitingForApproval,
    frb.BridgeSessionTurnActivity.waitingForUserInput =>
      StudioTurnActivity.waitingForUserInput,
    frb.BridgeSessionTurnActivity.waitingForPlanConfirmation =>
      StudioTurnActivity.waitingForPlanConfirmation,
    frb.BridgeSessionTurnActivity.persisting => StudioTurnActivity.persisting,
  };
}

StudioAgentView _agentFromFrb(frb.BridgeSessionAgentSnapshot value) {
  return StudioAgentView(
    id: value.id,
    sessionId: value.sessionId,
    path: value.path,
    parentPath: value.parentPath,
    role: value.role,
    task: value.task,
    status: value.status.name,
    summary: value.summary,
    depth: value.depth,
    error: value.error,
    reason: value.reason,
    updatedAt: _dateFromUnix(value.updatedAt),
  );
}

TimelineAgentEvent _timelineEventFromFrb(frb.BridgeSessionTimelineEvent value) {
  return TimelineAgentEvent(
    eventId: value.eventId,
    sessionId: value.sessionId,
    sequence: value.sequence.toInt(),
    createdAt: _dateFromUnix(value.createdAt),
    payload: value.kind.when(
      subAgentActivity:
          (
            callId,
            agentId,
            path,
            parentPath,
            kind,
            status,
            message,
            timedOut,
            error,
          ) => TimelineSubAgentActivity(
            callId: callId,
            kind: kind.name,
            timedOut: timedOut ?? false,
            agentId: agentId,
            path: path,
            parentPath: parentPath,
            statusValue: status?.name,
            message: message,
            error: error,
          ),
      todoListChanged: (snapshot) => TimelineTodoListUpdate(
        callId: snapshot.callId,
        agentId: snapshot.agentId,
        path: snapshot.path,
        parentPath: snapshot.parentPath,
        explanation: snapshot.explanation,
        items: snapshot.items
            .map(
              (item) =>
                  TimelineTodoItem(step: item.step, status: item.status.name),
            )
            .toList(),
      ),
    ),
  );
}

SessionRuntimeView _sessionRuntimeFromFrb(
  frb.BridgeSessionRuntimeSnapshot value,
) {
  final usage = value.usage;
  final costLabel = usage.estimatedCosts
      .map(
        (cost) => [
          cost.currency,
          _compactAmount(cost.amount.toString()),
        ].where((part) => part.isNotEmpty).join(' '),
      )
      .where((label) => label.isNotEmpty)
      .join(', ');
  return SessionRuntimeView(
    model: usage.model,
    contextTokens: usage.latestContextTokens.toInt(),
    contextWindow: usage.contextWindow?.toInt() ?? 0,
    totalTokens: usage.totalTokens.toInt(),
    costLabel: costLabel.isEmpty && usage.hasUnpricedUsage
        ? 'unpriced usage'
        : costLabel,
    activeSkills: value.activeSkills,
    activeMcpServers: value.activeMcpServers,
    activeLspServers: value.activeLspServers,
    agentCount: value.agentCount,
  );
}

PendingInteraction _interactionFromFrb(frb.BridgeInteractionRequest value) {
  final kind = switch (value.kind) {
    frb.BridgeInteractionKind.userInput => InteractionKind.userInput,
    frb.BridgeInteractionKind.toolApproval => InteractionKind.toolApproval,
    frb.BridgeInteractionKind.planConfirmation =>
      InteractionKind.planConfirmation,
  };
  final payload = value.payload.when<InteractionPayload>(
    userInput: (questions) => UserInputInteractionPayload(
      questions: [
        for (final question in questions)
          UserQuestionView(
            id: question.id,
            header: question.header,
            question: question.question,
            isOther: question.isOther,
            isSecret: question.isSecret,
            options: [
              for (final option in question.options ?? const [])
                UserQuestionOptionView(
                  label: option.label,
                  description: option.description,
                ),
            ],
          ),
      ],
    ),
    toolApproval: (name, argumentsJson, workingDirectory, parentAgentId) =>
        ToolApprovalInteractionPayload(
          toolName: name,
          arguments: JsonLeafDecoder.decode(argumentsJson),
          workingDirectory: workingDirectory ?? '',
          parentAgentId: parentAgentId,
        ),
    planConfirmation: (planId, content) =>
        PlanConfirmationInteractionPayload(planId: planId, content: content),
  );
  return PendingInteraction(
    id: value.interactionId,
    sessionId: value.scope.sessionId,
    kind: kind,
    title: _interactionTitle(kind, payload),
    body: _interactionBody(kind, payload),
    payload: payload,
  );
}

abstract final class JsonLeafDecoder {
  static Object? decode(String json) {
    try {
      return jsonDecode(json);
    } on FormatException catch (error) {
      throw FormatException('Invalid typed bridge JSON leaf: ${error.message}');
    }
  }

  static Map<String, Object?> decodeObject(String json) {
    final value = decode(json);
    if (value is Map<String, Object?>) {
      return value;
    }
    if (value is Map) {
      return value.map((key, value) => MapEntry(key.toString(), value));
    }
    throw const FormatException('Typed bridge JSON leaf must be an object');
  }
}

Object _studioFailure(Object error) {
  if (error is! frb.BridgeError) {
    return error;
  }
  return StudioFailure(
    code: StudioFailureCode.values.byName(error.code.name),
    message: error.message,
    retryable: error.retryable,
    correlationId: error.correlationId,
    detailsJson: error.detailsJson,
  );
}

frb.BridgeInteractionResolution _interactionResolutionFromDomain(
  InteractionResolutionCommand resolution,
) {
  return switch (resolution) {
    UserInputResolutionCommand(:final answers) =>
      frb.BridgeInteractionResolution.userInput(
        answers: [
          for (final answer in answers)
            frb.BridgeUserInputAnswer(
              questionId: answer.questionId,
              answers: answer.answers,
            ),
        ],
      ),
    ToolApprovalResolutionCommand(:final decision, :final reason) =>
      frb.BridgeInteractionResolution.toolApproval(
        decision: switch (decision) {
          ToolApprovalDecision.approved =>
            frb.BridgeToolApprovalResolution.approved,
          ToolApprovalDecision.denied =>
            frb.BridgeToolApprovalResolution.denied,
        },
        reason: reason,
      ),
    PlanConfirmationResolutionCommand(
      :final decision,
      :final content,
      :final reason,
    ) =>
      frb.BridgeInteractionResolution.planConfirmation(
        decision: switch (decision) {
          PlanConfirmationDecision.implementFreshContext =>
            frb.BridgePlanConfirmationResolution.implementFreshContext,
          PlanConfirmationDecision.continuePlanning =>
            frb.BridgePlanConfirmationResolution.continuePlanning,
          PlanConfirmationDecision.dismiss =>
            frb.BridgePlanConfirmationResolution.dismiss,
        },
        content: content,
        reason: reason,
      ),
  };
}

StudioState applyCanonicalSessionSnapshot(
  StudioState current,
  StudioSessionSnapshot snapshot,
) {
  final sessionId = snapshot.sessionId;
  if (sessionId.isEmpty) {
    return current;
  }
  final existingRuntime =
      current.runtimesBySession[sessionId] ?? _emptyRuntimeView();
  final runtime = (snapshot.runtime ?? existingRuntime).copyWith(
    task: existingRuntime.task,
    agentCount: snapshot.agents.length,
  );
  return current.copyWith(
    messagesBySession: {
      ...current.messagesBySession,
      sessionId: snapshot.messages,
    },
    partSnapshotsBySession: {
      ...current.partSnapshotsBySession,
      sessionId: snapshot.parts,
    },
    partOverlaysBySession: {
      ...current.partOverlaysBySession,
      sessionId: const {},
    },
    agentTimelineEventsBySession: {
      ...current.agentTimelineEventsBySession,
      sessionId: snapshot.timelineEvents,
    },
    agentsBySession: {...current.agentsBySession, sessionId: snapshot.agents},
    runtimesBySession: {...current.runtimesBySession, sessionId: runtime},
    pendingInteractions: [
      for (final interaction in current.pendingInteractions)
        if (interaction.sessionId != sessionId) interaction,
      ...snapshot.interactions,
    ],
    turnsBySession: snapshot.turn == null
        ? const {}
        : {sessionId: snapshot.turn!},
    removeTurnSessionIds: snapshot.turn == null ? {sessionId} : const {},
    workspaceSyncBySession: {
      ...current.workspaceSyncBySession,
      sessionId: AgentWorkspaceSyncState.ready,
    },
    eventCursorsBySession: {
      ...current.eventCursorsBySession,
      sessionId: snapshot.throughSequence,
    },
  );
}
