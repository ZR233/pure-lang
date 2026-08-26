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
            pending: _interactionIsPending(interaction),
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
    required this.state,
  });

  final String itemId;
  final int revision;
  final ThreadItemDeltaStateView state;
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
        .where(_interactionIsPending)
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
    status: switch (value.status) {
      frb.BridgeThreadStatus.idle => ThreadStatusView.idle,
      frb.BridgeThreadStatus.queued => ThreadStatusView.queued,
      frb.BridgeThreadStatus.running => ThreadStatusView.running,
      frb.BridgeThreadStatus.waitingTool => ThreadStatusView.waitingTool,
      frb.BridgeThreadStatus.waitingInteraction =>
        ThreadStatusView.waitingInteraction,
      frb.BridgeThreadStatus.cancelling => ThreadStatusView.cancelling,
      frb.BridgeThreadStatus.closing => ThreadStatusView.closing,
      frb.BridgeThreadStatus.closed => ThreadStatusView.closed,
      frb.BridgeThreadStatus.faulted => ThreadStatusView.faulted,
    },
    archived: value.archived,
  );
}

ThreadItemView _threadItemFromFrb(
  frb_item.BridgeThreadItem value, {
  ThreadContextDisposition contextDisposition = ThreadContextDisposition.active,
}) {
  return ThreadItemView(
    id: value.id,
    threadId: value.threadId,
    turnId: value.turnId,
    ordinal: value.ordinal.toInt(),
    revision: value.revision.toInt(),
    createdAt: _dateFromUnix(value.createdAt),
    updatedAt: _dateFromUnix(value.updatedAt),
    state: _threadItemStateFromFrb(value.state),
    contextDisposition: contextDisposition,
  );
}

ThreadItemStateView _threadItemStateFromFrb(
  frb_item.BridgeThreadItemState value,
) {
  return value.when(
    text: (channel, text, attachments, lifecycle) => ThreadTextItemStateView(
      channel: switch (channel) {
        frb_item.BridgeThreadTextChannel.user => ThreadTextChannel.user,
        frb_item.BridgeThreadTextChannel.commentary =>
          ThreadTextChannel.commentary,
        frb_item.BridgeThreadTextChannel.final_ =>
          ThreadTextChannel.finalAnswer,
      },
      text: text,
      attachments: attachments.map(_attachmentFromFrb).toList(),
      lifecycle: _contentLifecycleFromFrb(lifecycle),
    ),
    thinking: (summary, content, lifecycle) => ThreadThinkingItemStateView(
      summary: summary,
      content: content,
      lifecycle: _contentLifecycleFromFrb(lifecycle),
    ),
    tool: (invocation, state) => ThreadToolItemStateView(
      invocation: ThreadToolInvocationView(
        toolCallId: invocation.toolCallId,
        callId: invocation.callId,
        providerItemId: invocation.providerItemId,
        name: invocation.name,
        arguments: invocation.arguments,
        workingDirectory: invocation.workingDirectory,
      ),
      lifecycle: _toolLifecycleFromFrb(state),
    ),
    agent: (identity, state) => ThreadAgentItemStateView(
      identity: ThreadAgentIdentityView(
        id: identity.id,
        path: identity.path,
        parentPath: identity.parentPath,
        role: identity.role,
        task: identity.task,
        depth: identity.depth,
      ),
      lifecycle: _agentLifecycleFromFrb(state),
    ),
    turn: (state) => ThreadTurnItemStateView(_turnStateFromFrb(state)),
    inference: (inferenceId, model, state) => ThreadInferenceItemStateView(
      inferenceId: inferenceId,
      model: model,
      lifecycle: _inferenceLifecycleFromFrb(state),
    ),
    plan: (content, lifecycle) => ThreadPlanItemStateView(
      content: content,
      lifecycle: _contentLifecycleFromFrb(lifecycle),
    ),
    skill: (name, source, providerId, resourceBase, cause, activatedAt) =>
        ThreadSkillItemStateView(
          name: name,
          source: source,
          providerId: providerId,
          resourceBase: resourceBase.when(
            directory: (path) =>
                SkillResourceBaseView(SkillResourceBaseKind.directory, path),
            url: (url) => SkillResourceBaseView(SkillResourceBaseKind.url, url),
            opaque: (description) => SkillResourceBaseView(
              SkillResourceBaseKind.opaque,
              description,
            ),
          ),
          cause: cause.when(
            tool: (toolCallId) => SkillActivationCauseView(
              SkillActivationCauseKind.tool,
              toolCallId,
            ),
            userGesture: (invocationId) => SkillActivationCauseView(
              SkillActivationCauseKind.userGesture,
              invocationId,
            ),
          ),
          activatedAt: _dateFromUnix(activatedAt),
        ),
    file: (path, mediaType, completedAt) =>
        ThreadFileItemStateView(path, mediaType, _dateFromUnix(completedAt)),
    contextCompaction: (beforeTokens, afterTokens, compactedAt) =>
        ThreadContextCompactionItemStateView(
          beforeTokens.toInt(),
          afterTokens.toInt(),
          _dateFromUnix(compactedAt),
        ),
  );
}

ThreadContentLifecycleView _contentLifecycleFromFrb(
  frb_item.BridgeThreadContentLifecycle value,
) {
  return value.when(
    streaming: () => const StreamingThreadContentView(),
    completed: (completedAt) =>
        CompletedThreadContentView(_dateFromUnix(completedAt)),
    failed: (failedAt, error) =>
        FailedThreadContentView(_dateFromUnix(failedAt), error),
    cancelled: (cancelledAt, reason) =>
        CancelledThreadContentView(_dateFromUnix(cancelledAt), reason),
  );
}

ThreadAttachmentView _attachmentFromFrb(frb_item.BridgeThreadAttachment value) {
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

ThreadToolLifecycleView _toolLifecycleFromFrb(
  frb_item.BridgeThreadToolState value,
) {
  return value.when(
    started: () => const StartedThreadToolView(),
    streaming: () => const StreamingThreadToolView(),
    awaitingApproval: () => const AwaitingApprovalThreadToolView(),
    approved: () => const ApprovedThreadToolView(),
    running: (streamedOutput) => RunningThreadToolView(streamedOutput),
    succeeded: (completedAt, output) => SucceededThreadToolView(
      _dateFromUnix(completedAt),
      _toolOutputFromFrb(output),
    ),
    failed: (failedAt, failure, output) => FailedThreadToolView(
      _dateFromUnix(failedAt),
      ThreadToolFailureView(
        kind: switch (failure.kind) {
          frb_item.BridgeThreadToolFailureKind.execution =>
            ThreadToolFailureKindView.execution,
          frb_item.BridgeThreadToolFailureKind.timedOut =>
            ThreadToolFailureKindView.timedOut,
          frb_item.BridgeThreadToolFailureKind.budgetLimited =>
            ThreadToolFailureKindView.budgetLimited,
        },
        message: failure.message,
      ),
      output == null ? null : _toolOutputFromFrb(output),
    ),
    denied: (deniedAt, reason) =>
        DeniedThreadToolView(_dateFromUnix(deniedAt), reason),
    cancelled: (cancelledAt, reason) =>
        CancelledThreadToolView(_dateFromUnix(cancelledAt), reason),
  );
}

ThreadToolOutputView _toolOutputFromFrb(frb_item.BridgeThreadToolOutput value) {
  return ThreadToolOutputView(
    result: value.result,
    outputArtifacts: value.outputArtifactsJson
        .map(JsonLeafDecoder.decode)
        .toList(),
    exitCode: value.exitCode,
  );
}

ThreadAgentLifecycleView _agentLifecycleFromFrb(
  frb_item.BridgeThreadAgentState value,
) {
  return value.when(
    queued: () => const QueuedThreadAgentView(),
    running: () => const RunningThreadAgentView(),
    succeeded: (completedAt, summary) =>
        SucceededThreadAgentView(_dateFromUnix(completedAt), summary),
    denied: (deniedAt, reason) =>
        DeniedThreadAgentView(_dateFromUnix(deniedAt), reason),
    cancelled: (cancelledAt, reason) =>
        CancelledThreadAgentView(_dateFromUnix(cancelledAt), reason),
    failed: (failedAt, error) =>
        FailedThreadAgentView(_dateFromUnix(failedAt), error),
  );
}

ThreadInferenceLifecycleView _inferenceLifecycleFromFrb(
  frb_item.BridgeThreadInferenceState value,
) {
  return value.when(
    running: () => const RunningThreadInferenceView(),
    completed: (completedAt, usage) => CompletedThreadInferenceView(
      _dateFromUnix(completedAt),
      ThreadInferenceUsageView(
        promptTokens: usage.promptTokens.toInt(),
        completionTokens: usage.completionTokens.toInt(),
        cachedPromptTokens: usage.cachedPromptTokens.toInt(),
        totalTokens: usage.totalTokens.toInt(),
      ),
    ),
    failed: (failedAt, error) =>
        FailedThreadInferenceView(_dateFromUnix(failedAt), error),
    cancelled: (cancelledAt, reason) =>
        CancelledThreadInferenceView(_dateFromUnix(cancelledAt), reason),
  );
}

ThreadItemDeltaView _threadItemDeltaFromFrb(
  frb_item.BridgeThreadItemDelta value,
) {
  return ThreadItemDeltaView(
    itemId: value.itemId,
    revision: value.revision.toInt(),
    state: value.delta.when(
      text: ThreadTextDeltaView.new,
      thinkingSummary: ThreadThinkingSummaryDeltaView.new,
      thinkingContent: ThreadThinkingContentDeltaView.new,
      plan: ThreadPlanDeltaView.new,
      toolArguments: ThreadToolArgumentsDeltaView.new,
      toolResult: ThreadToolResultDeltaView.new,
    ),
  );
}

StudioTurnView _turnFromFrb(frb.BridgeTurn value) {
  return StudioTurnView(
    turnId: value.id,
    threadId: value.threadId,
    revision: value.revision.toInt(),
    state: _turnStateFromFrb(value.state),
    updatedAt: _dateFromUnix(value.updatedAt),
  );
}

StudioTurnState _turnStateFromFrb(frb.BridgeTurnState value) {
  return value.when(
    queued: (queuedAt) => QueuedStudioTurnState(queuedAt: queuedAt),
    running: (startedAt, phase) => RunningStudioTurnState(
      startedAt: startedAt,
      activity: switch (phase) {
        frb.BridgeTurnPhase.preparing => StudioTurnActivity.preparing,
        frb.BridgeTurnPhase.thinking => StudioTurnActivity.thinking,
        frb.BridgeTurnPhase.responding => StudioTurnActivity.responding,
        frb.BridgeTurnPhase.planning => StudioTurnActivity.planning,
        frb.BridgeTurnPhase.runningTool => StudioTurnActivity.runningTool,
        frb.BridgeTurnPhase.persisting => StudioTurnActivity.persisting,
      },
    ),
    completed: (startedAt, completedAt, completion) => CompletedStudioTurnState(
      startedAt: startedAt,
      completedAt: completedAt,
      completion: switch (completion) {
        frb.BridgeTurnCompletion.normal => StudioTurnCompletion.normal,
        frb.BridgeTurnCompletion.interactionRequested =>
          StudioTurnCompletion.interactionRequested,
      },
    ),
    cancelled: (startedAt, requestedAt, completedAt, cause) =>
        CancelledStudioTurnState(
          startedAt: startedAt,
          requestedAt: requestedAt,
          completedAt: completedAt,
          cause: _turnCancellationCauseFromFrb(cause),
        ),
    failed: (startedAt, completedAt, failure) => FailedStudioTurnState(
      startedAt: startedAt,
      completedAt: completedAt,
      failure: _turnFailureFromFrb(failure),
    ),
    budgetLimited: (startedAt, completedAt, limit, rollover) =>
        BudgetLimitedStudioTurnState(
          startedAt: startedAt,
          completedAt: completedAt,
          limit: StudioTurnBudgetLimit(
            kind: StudioTurnBudgetLimitKind.values.byName(limit.kind.name),
            usage: StudioTurnBudgetUsage(
              modelSteps: limit.usage.modelSteps,
              toolCalls: limit.usage.toolCalls,
              waitCalls: limit.usage.waitCalls,
              elapsedMs: limit.usage.elapsedMs.toInt(),
            ),
          ),
          rollover: rollover.when(
            notAttempted: () => const RolloverNotAttempted(),
            succeeded: () => const RolloverSucceeded(),
            failed: (error) => RolloverFailed(error: error),
          ),
        ),
  );
}

StudioTurnCancellationCause _turnCancellationCauseFromFrb(
  frb.BridgeTurnCancellationCause value,
) {
  return value.when(
    userRequested: () => const UserRequestedTurnCancellation(),
    runtimeShutdown: () => const RuntimeShutdownTurnCancellation(),
    agentClosed: () => const AgentClosedTurnCancellation(),
    recovery: () => const RecoveryTurnCancellation(),
    coalesced: (targetTurnId) =>
        CoalescedTurnCancellation(targetTurnId: targetTurnId),
  );
}

StudioTurnFailureView _turnFailureFromFrb(frb.BridgeTurnFailureDto value) {
  return StudioTurnFailureView(
    category: value.category,
    providerKind: value.providerKind,
    code: value.code,
    httpStatus: value.httpStatus,
    message: value.message,
    retryable: value.retryable,
    retryAfterMs: value.retryAfterMs?.toInt(),
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
    costLabel: costLabel,
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
  final mapped = value.content.when(
    userInput: (questions, state) => (
      kind: InteractionKind.userInput,
      payload: UserInputInteractionPayload(
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
      ) as InteractionPayload,
    ),
    toolApproval:
        (name, argumentsJson, workingDirectory, parentAgentId, state) => (
          kind: InteractionKind.toolApproval,
          payload: ToolApprovalInteractionPayload(
            toolName: name,
            arguments: JsonLeafDecoder.decode(argumentsJson),
            workingDirectory: workingDirectory ?? '',
            parentAgentId: parentAgentId,
          ) as InteractionPayload,
        ),
    planConfirmation: (planId, content, state) => (
      kind: InteractionKind.planConfirmation,
      payload: PlanConfirmationInteractionPayload(
        planId: planId,
        content: content,
      ) as InteractionPayload,
    ),
  );
  return PendingInteraction(
    id: value.interactionId,
    threadId: value.scope.threadId,
    turnId: value.scope.turnId,
    kind: mapped.kind,
    title: _interactionTitle(mapped.kind, mapped.payload),
    body: _interactionBody(mapped.kind, mapped.payload),
    payload: mapped.payload,
  );
}

bool _interactionIsPending(frb.BridgeInteractionRequest value) {
  return value.content.when(
    userInput: (questions, state) => state.when(
      pending: (operationId) => true,
      resolved: (operationId, resolvedAt, answers) => false,
      cancelled: (operationId, cancelledAt, reason) => false,
      expired: (operationId, expiredAt) => false,
    ),
    toolApproval:
        (name, argumentsJson, workingDirectory, parentAgentId, state) =>
            state.when(
              pending: (operationId) => true,
              resolved: (operationId, resolvedAt, decision, reason) => false,
              cancelled: (operationId, cancelledAt, reason) => false,
              expired: (operationId, expiredAt) => false,
            ),
    planConfirmation: (planId, content, state) => state.when(
      pending: (operationId) => true,
      resolved: (operationId, resolvedAt, decision, resolvedContent, reason) =>
          false,
      cancelled: (operationId, cancelledAt, reason) => false,
      expired: (operationId, expiredAt) => false,
    ),
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
          PlanConfirmationDecision.confirm =>
            frb.BridgePlanConfirmationResolution.confirm,
          PlanConfirmationDecision.revisePlan =>
            frb.BridgePlanConfirmationResolution.revisePlan,
        },
        content: content,
        reason: reason,
      ),
  };
}
