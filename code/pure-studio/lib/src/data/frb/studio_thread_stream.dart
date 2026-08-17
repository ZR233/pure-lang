part of 'studio_api.dart';

sealed class ThreadStreamFrame {
  const ThreadStreamFrame();

  factory ThreadStreamFrame.fromFrb(frb.BridgeThreadSubscriptionUpdate value) {
    return value.when(
      snapshot: (snapshot) => ThreadSnapshotFrame(
        workspace: _threadWorkspaceFromFrb(snapshot),
        historyCursor: snapshot.historyCursor,
      ),
      notification: (envelope) => envelope.notification.when(
        turnStarted: (turn) => ThreadNotificationFrame(
          threadId: envelope.threadId,
          revision: envelope.revision.toInt(),
          update: ThreadTurnUpdate(_turnFromFrb(turn)),
        ),
        turnUpdated: (turn) => ThreadNotificationFrame(
          threadId: envelope.threadId,
          revision: envelope.revision.toInt(),
          update: ThreadTurnUpdate(_turnFromFrb(turn)),
        ),
        turnCompleted: (turn) => ThreadNotificationFrame(
          threadId: envelope.threadId,
          revision: envelope.revision.toInt(),
          update: ThreadTurnUpdate(_turnFromFrb(turn)),
        ),
        itemStarted: (item) => ThreadNotificationFrame(
          threadId: envelope.threadId,
          revision: envelope.revision.toInt(),
          update: ThreadItemUpsert(_threadItemFromFrb(item)),
        ),
        itemDelta: (delta) => ThreadNotificationFrame(
          threadId: envelope.threadId,
          revision: envelope.revision.toInt(),
          update: ThreadItemDeltaUpdate(_threadItemDeltaFromFrb(delta)),
        ),
        itemCompleted: (item) => ThreadNotificationFrame(
          threadId: envelope.threadId,
          revision: envelope.revision.toInt(),
          update: ThreadItemUpsert(_threadItemFromFrb(item)),
        ),
        interactionChanged: (interaction) => ThreadNotificationFrame(
          threadId: envelope.threadId,
          revision: envelope.revision.toInt(),
          update: ThreadInteractionUpdate(
            interaction: _interactionFromFrb(interaction),
            pending: interaction.status == frb.BridgeInteractionStatus.pending,
          ),
        ),
        threadRuntimeUpdated: (runtime) => ThreadNotificationFrame(
          threadId: envelope.threadId,
          revision: envelope.revision.toInt(),
          update: ThreadRuntimeUpdate(
            runtime: _threadRuntimeFromFrb(runtime),
            todo: _todoFromFrb(runtime.todo),
          ),
        ),
        lagged: (dropped) => ThreadResyncRequiredFrame(
          threadId: envelope.threadId,
          dropped: dropped.toInt(),
        ),
      ),
    );
  }
}

final class ThreadSnapshotFrame extends ThreadStreamFrame {
  const ThreadSnapshotFrame({required this.workspace, this.historyCursor});

  final ThreadWorkspace workspace;

  /// 快照窗口之外的更旧历史回源锚点（Turn id，before 语义）；null = 无更旧内容。
  final String? historyCursor;
}

final class ThreadNotificationFrame extends ThreadStreamFrame {
  const ThreadNotificationFrame({
    required this.threadId,
    required this.revision,
    required this.update,
  });

  final String threadId;
  final int revision;
  final ThreadWorkspaceUpdate update;
}

final class ThreadResyncRequiredFrame extends ThreadStreamFrame {
  const ThreadResyncRequiredFrame({
    required this.threadId,
    required this.dropped,
  });

  final String threadId;
  final int dropped;
}

sealed class ThreadWorkspaceUpdate {
  const ThreadWorkspaceUpdate();
}

final class ThreadTurnUpdate extends ThreadWorkspaceUpdate {
  const ThreadTurnUpdate(this.turn);

  final StudioTurnView turn;
}

final class ThreadItemUpsert extends ThreadWorkspaceUpdate {
  const ThreadItemUpsert(this.item);

  final ThreadItemView item;
}

final class ThreadItemDeltaUpdate extends ThreadWorkspaceUpdate {
  const ThreadItemDeltaUpdate(this.delta);

  final ThreadItemDeltaView delta;
}

final class ThreadInteractionUpdate extends ThreadWorkspaceUpdate {
  const ThreadInteractionUpdate({
    required this.interaction,
    required this.pending,
  });

  final PendingInteraction interaction;
  final bool pending;
}

final class ThreadRuntimeUpdate extends ThreadWorkspaceUpdate {
  const ThreadRuntimeUpdate({required this.runtime, required this.todo});

  final ThreadRuntimeView runtime;
  final TimelineTodoListUpdate? todo;
}

class ThreadItemDeltaView {
  const ThreadItemDeltaView({
    required this.itemId,
    required this.revision,
    required this.field,
    required this.delta,
  });

  final String itemId;
  final int revision;
  final String field;
  final String delta;
}

class ThreadHistoryPage {
  const ThreadHistoryPage({required this.items, required this.nextCursor});

  final List<ThreadItemView> items;
  final String? nextCursor;
}

ThreadWorkspace _threadWorkspaceFromFrb(frb.BridgeThreadSnapshot value) {
  return ThreadWorkspace(
    thread: _threadFromFrb(value.thread),
    revision: value.revision.toInt(),
    items: value.items.map(_threadItemFromFrb).toList()
      ..sort(_compareThreadItems),
    interactions: value.interactions
        .where(
          (interaction) =>
              interaction.status == frb.BridgeInteractionStatus.pending,
        )
        .map(_interactionFromFrb)
        .toList(),
    runtime: value.runtime == null
        ? _emptyRuntimeView()
        : _threadRuntimeFromFrb(value.runtime!),
    activeTurn: value.activeTurn == null
        ? null
        : _turnFromFrb(value.activeTurn!),
    todo: _todoFromFrb(value.runtime?.todo),
  );
}

StudioThread _threadFromFrb(frb.BridgeThread value) {
  return StudioThread(
    id: value.id,
    projectId: value.projectId,
    title: value.title,
    mode: switch (value.mode) {
      frb.BridgeThreadMode.simple => StudioMode.simple,
      frb.BridgeThreadMode.task => StudioMode.task,
    },
    createdAt: _dateFromUnix(value.createdAt),
    updatedAt: _dateFromUnix(value.updatedAt),
    parentThreadId: value.parentThreadId,
    rootThreadId: value.rootThreadId,
    agentPath: value.agentPath,
    role: value.role,
    status: value.status.name,
    archived: value.archived,
  );
}

ThreadItemView _threadItemFromFrb(
  frb.BridgeThreadItem value, {
  ThreadContextDisposition contextDisposition = ThreadContextDisposition.active,
}) {
  final base = (
    id: value.id,
    threadId: value.threadId,
    turnId: value.turnId,
    ordinal: value.ordinal.toInt(),
    revision: value.revision.toInt(),
    status: value.status.name,
    createdAt: _dateFromUnix(value.createdAt),
    updatedAt: _dateFromUnix(value.updatedAt),
    completedAt: value.completedAt == null
        ? null
        : _dateFromUnix(value.completedAt!),
    error: value.error,
  );
  return value.content.when(
    userMessage: (text, attachments) => ThreadItemView(
      id: base.id,
      threadId: base.threadId,
      turnId: base.turnId,
      ordinal: base.ordinal,
      revision: base.revision,
      status: base.status,
      createdAt: base.createdAt,
      updatedAt: base.updatedAt,
      completedAt: base.completedAt,
      error: base.error,
      kind: ThreadItemKind.userMessage,
      text: text,
      attachments: attachments.map(_attachmentFromFrb).toList(),
      contextDisposition: contextDisposition,
    ),
    agentMessage: (channel, text) => ThreadItemView(
      id: base.id,
      threadId: base.threadId,
      turnId: base.turnId,
      ordinal: base.ordinal,
      revision: base.revision,
      status: base.status,
      createdAt: base.createdAt,
      updatedAt: base.updatedAt,
      completedAt: base.completedAt,
      error: base.error,
      kind: ThreadItemKind.agentMessage,
      text: text,
      channel: switch (channel) {
        frb.BridgeAgentMessageChannel.commentary =>
          AgentMessageChannel.commentary,
        frb.BridgeAgentMessageChannel.final_ => AgentMessageChannel.finalAnswer,
      },
      contextDisposition: contextDisposition,
    ),
    reasoning: (summary, content) => ThreadItemView(
      id: base.id,
      threadId: base.threadId,
      turnId: base.turnId,
      ordinal: base.ordinal,
      revision: base.revision,
      status: base.status,
      createdAt: base.createdAt,
      updatedAt: base.updatedAt,
      completedAt: base.completedAt,
      error: base.error,
      kind: ThreadItemKind.reasoning,
      reasoningSummary: summary,
      reasoningContent: content,
      contextDisposition: contextDisposition,
    ),
    plan: (content) => ThreadItemView(
      id: base.id,
      threadId: base.threadId,
      turnId: base.turnId,
      ordinal: base.ordinal,
      revision: base.revision,
      status: base.status,
      createdAt: base.createdAt,
      updatedAt: base.updatedAt,
      completedAt: base.completedAt,
      error: base.error,
      kind: ThreadItemKind.plan,
      text: content,
      contextDisposition: contextDisposition,
    ),
    toolCall: (tool) => ThreadItemView(
      id: base.id,
      threadId: base.threadId,
      turnId: base.turnId,
      ordinal: base.ordinal,
      revision: base.revision,
      status: base.status,
      createdAt: base.createdAt,
      updatedAt: base.updatedAt,
      completedAt: base.completedAt,
      error: base.error,
      kind: ThreadItemKind.toolCall,
      tool: _toolFromFrb(tool),
      contextDisposition: contextDisposition,
    ),
    file: (path, mediaType) => ThreadItemView(
      id: base.id,
      threadId: base.threadId,
      turnId: base.turnId,
      ordinal: base.ordinal,
      revision: base.revision,
      status: base.status,
      createdAt: base.createdAt,
      updatedAt: base.updatedAt,
      completedAt: base.completedAt,
      error: base.error,
      kind: ThreadItemKind.file,
      filePath: path,
      mediaType: mediaType,
      contextDisposition: contextDisposition,
    ),
  );
}

ThreadAttachmentView _attachmentFromFrb(frb.BridgeThreadAttachment value) {
  return ThreadAttachmentView(
    id: value.id,
    mediaType: value.mediaType,
    filename: value.filename,
    width: value.width,
    height: value.height,
    byteSize: value.byteSize.toInt(),
    dataUrl: value.dataUrl,
  );
}

TimelineToolPart _toolFromFrb(frb.BridgeThreadToolCall value) {
  return TimelineToolPart(
    toolCallId: value.toolCallId,
    callId: value.callId,
    providerItemId: value.providerItemId,
    name: value.name,
    arguments: value.arguments,
    result: value.result,
    outputArtifacts: value.outputArtifactsJson
        .map(JsonLeafDecoder.decode)
        .toList(),
    exitCode: value.exitCode,
    timedOut: value.timedOut,
    workingDirectory: value.workingDirectory,
    denialReason: value.denialReason,
  );
}

ThreadItemDeltaView _threadItemDeltaFromFrb(frb.BridgeThreadItemDelta value) {
  return ThreadItemDeltaView(
    itemId: value.itemId,
    revision: value.revision.toInt(),
    field: switch (value.field) {
      frb.BridgeThreadItemDeltaField.text => 'text',
      frb.BridgeThreadItemDeltaField.reasoningSummary => 'reasoning.summary',
      frb.BridgeThreadItemDeltaField.reasoningContent => 'reasoning.content',
      frb.BridgeThreadItemDeltaField.planContent => 'planContent',
      frb.BridgeThreadItemDeltaField.toolArguments => 'tool.arguments',
      frb.BridgeThreadItemDeltaField.toolResult => 'tool.result',
    },
    delta: value.delta,
  );
}

StudioTurnView _turnFromFrb(frb.BridgeTurn value) {
  return StudioTurnView(
    turnId: value.id,
    threadId: value.threadId,
    state: value.state.when(
      queued: () => const StudioTurnState.queued(),
      inProgress: (phase) => StudioTurnState.inProgress(switch (phase) {
        frb.BridgeTurnPhase.preparing => StudioTurnActivity.preparing,
        frb.BridgeTurnPhase.thinking => StudioTurnActivity.thinking,
        frb.BridgeTurnPhase.responding => StudioTurnActivity.responding,
        frb.BridgeTurnPhase.planning => StudioTurnActivity.planning,
        frb.BridgeTurnPhase.runningTool => StudioTurnActivity.runningTool,
        frb.BridgeTurnPhase.persisting => StudioTurnActivity.persisting,
      }),
      completed: () => const StudioTurnState.completed(),
      failed: StudioTurnState.failed,
      interrupted: StudioTurnState.cancelled,
    ),
    failure: value.failure == null
        ? null
        : StudioTurnFailureView(
            category: value.failure!.category,
            providerKind: value.failure!.providerKind,
            code: value.failure!.code,
            httpStatus: value.failure!.httpStatus,
            message: value.failure!.message,
            retryable: value.failure!.retryable,
            retryAfterMs: value.failure!.retryAfterMs?.toInt(),
          ),
    updatedAt: _dateFromUnix(value.updatedAt),
  );
}

ThreadRuntimeView _threadRuntimeFromFrb(frb.BridgeThreadRuntimeSnapshot value) {
  final usage = value.usage;
  final estimatedCosts = usage.estimatedCosts
      .map(
        (cost) => RuntimeCostView(currency: cost.currency, amount: cost.amount),
      )
      .toList(growable: false);
  final costLabel = formatRuntimeCosts(estimatedCosts);
  return ThreadRuntimeView(
    model: usage.model,
    contextTokens: usage.latestContextTokens.toInt(),
    contextWindow: usage.contextWindow?.toInt() ?? 0,
    totalTokens: usage.totalTokens.toInt(),
    promptTokens: usage.promptTokens.toInt(),
    completionTokens: usage.completionTokens.toInt(),
    cachedPromptTokens: usage.cachedPromptTokens.toInt(),
    cacheWriteTokens: usage.cacheWriteTokens.toInt(),
    cacheMissTokens: usage.cacheMissTokens.toInt(),
    reasoningTokens: usage.reasoningTokens.toInt(),
    inferenceCount: usage.inferenceCount.toInt(),
    cacheHitRate: usage.cacheHitRate,
    estimatedCosts: estimatedCosts,
    estimatedCacheSavings: usage.estimatedCacheSavings
        .map(
          (cost) =>
              RuntimeCostView(currency: cost.currency, amount: cost.amount),
        )
        .toList(growable: false),
    hasUnpricedUsage: usage.hasUnpricedUsage,
    promptGeneration: usage.promptGeneration?.toInt(),
    promptCachePolicy: usage.promptCachePolicy,
    prefixChangedReason: usage.prefixChangedReason?.name,
    toolRegistryRevision: value.toolRegistryRevision?.toInt(),
    toolCatalogHash: value.toolCatalogHash,
    costLabel: costLabel.isEmpty && usage.hasUnpricedUsage
        ? 'unpriced usage'
        : costLabel,
    activeSkills: value.activeSkills,
    activeMcpServers: value.activeMcpServers,
    activeLspServers: value.activeLspServers,
    agentCount: 0,
  );
}

TimelineTodoListUpdate? _todoFromFrb(frb.BridgeTodoListSnapshot? value) {
  if (value == null) return null;
  return TimelineTodoListUpdate(
    callId: value.callId,
    agentId: value.agentId,
    path: value.path,
    parentPath: value.parentPath,
    explanation: value.explanation,
    items: [
      for (final item in value.items)
        TimelineTodoItem(step: item.step, status: item.status.name),
    ],
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
    threadId: value.scope.threadId,
    turnId: value.scope.turnId,
    kind: kind,
    title: _interactionTitle(kind, payload),
    body: _interactionBody(kind, payload),
    payload: payload,
  );
}

int _compareThreadItems(ThreadItemView left, ThreadItemView right) {
  final ordinal = left.ordinal.compareTo(right.ordinal);
  return ordinal != 0 ? ordinal : left.id.compareTo(right.id);
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
    if (value is Map<String, Object?>) return value;
    if (value is Map) {
      return value.map((key, value) => MapEntry(key.toString(), value));
    }
    throw const FormatException('Typed bridge JSON leaf must be an object');
  }
}

Object _studioFailure(Object error) {
  if (error is! frb.BridgeError) return error;
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
