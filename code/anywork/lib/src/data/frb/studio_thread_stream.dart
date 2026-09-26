part of 'studio_api.dart';

sealed class ThreadStreamFrame {
  const ThreadStreamFrame();

  factory ThreadStreamFrame.fromFrb(frb.BridgeThreadSubscriptionUpdate value) {
    return value.when(
      snapshot: (snapshot) => ThreadSnapshotFrame(
        workspace: _threadWorkspaceFromSnapshot(snapshot),
      ),
      notification: (envelope) {
        final threadId = envelope.threadId;
        final revision = envelope.revision.toInt();
        final epoch = envelope.epoch.toInt();
        final baseRevision = envelope.baseRevision.toInt();
        return envelope.notification.when(
          turnStarted: (turn) => ThreadNotificationFrame(
            threadId: threadId,
            revision: revision,
            epoch: epoch,
            baseRevision: baseRevision,
            update: ThreadTurnUpdate(_turnFromFrb(turn)),
          ),
          turnUpdated: (turn) => ThreadNotificationFrame(
            threadId: threadId,
            revision: revision,
            epoch: epoch,
            baseRevision: baseRevision,
            update: ThreadTurnUpdate(_turnFromFrb(turn)),
          ),
          turnCompleted: (turn) => ThreadNotificationFrame(
            threadId: threadId,
            revision: revision,
            epoch: epoch,
            baseRevision: baseRevision,
            update: ThreadTurnUpdate(_turnFromFrb(turn)),
          ),
          interactionChanged: (interaction) => ThreadNotificationFrame(
            threadId: threadId,
            revision: revision,
            epoch: epoch,
            baseRevision: baseRevision,
            update: ThreadInteractionUpdate(
              interaction: _interactionFromFrb(interaction),
              pending: _interactionIsPending(interaction),
            ),
          ),
          threadRuntimeUpdated: (runtime) => ThreadNotificationFrame(
            threadId: threadId,
            revision: revision,
            epoch: epoch,
            baseRevision: baseRevision,
            update: ThreadRuntimeUpdate(
              runtime: _threadRuntimeFromFrb(runtime),
              todo: _todoFromFrb(runtime.todo),
            ),
          ),
          activityChanged: (activity) => ThreadNotificationFrame(
            threadId: threadId,
            revision: revision,
            epoch: epoch,
            baseRevision: baseRevision,
            update: ThreadActivityUpdate(
              activity: activity == null
                  ? null
                  : _threadActivityFromFrb(activity),
            ),
          ),
          storageChanged: (storage) => ThreadNotificationFrame(
            threadId: threadId,
            revision: revision,
            epoch: epoch,
            baseRevision: baseRevision,
            update: ThreadStorageUpdate(
              storage: storage == null ? null : _threadStorageFromFrb(storage),
            ),
          ),
          lagged: (dropped) => ThreadResyncRequiredFrame(
            threadId: threadId,
            dropped: dropped.toInt(),
            epoch: epoch,
          ),
        );
      },
    );
  }
}

final class ThreadSnapshotFrame extends ThreadStreamFrame {
  const ThreadSnapshotFrame({required this.workspace});

  /// 当前内存状态与实时事实；不携带 Timeline 条目，历史由分页 API 提供。
  final ThreadWorkspace workspace;
}

final class ThreadNotificationFrame extends ThreadStreamFrame {
  const ThreadNotificationFrame({
    required this.threadId,
    required this.revision,
    required this.update,
    this.epoch,
    this.baseRevision,
  });

  final String threadId;
  final int revision;

  /// 生产端连续广播生命周期；与当前 epoch 不一致时丢弃或重同步。
  final int? epoch;

  /// 本通知之前的状态水位；非空且与当前 revision 不一致表示缺口。
  final int? baseRevision;
  final ThreadWorkspaceUpdate update;
}

final class ThreadResyncRequiredFrame extends ThreadStreamFrame {
  const ThreadResyncRequiredFrame({
    required this.threadId,
    required this.dropped,
    this.epoch,
  });

  final String threadId;
  final int dropped;
  final int? epoch;
}

sealed class ThreadWorkspaceUpdate {
  const ThreadWorkspaceUpdate();
}

final class ThreadTurnUpdate extends ThreadWorkspaceUpdate {
  const ThreadTurnUpdate(this.turn);

  final StudioTurnView turn;
}

/// 后端 typed 当前活动变化；[activity] 为 `null` 表示当前没有活动（清除）。
///
/// 活动独立于消息窗口：它只带小型 typed 摘要与身份，正文不在状态流里。
final class ThreadActivityUpdate extends ThreadWorkspaceUpdate {
  const ThreadActivityUpdate({required this.activity});

  final ThreadActivityView? activity;
}

/// 后端 typed 存储状态变化；[storage] 为 `null` 表示当前没有可报告的存储事实（未知，
/// 不是“健康”）。界面据此显示“等待保存/明确暂停原因”，不从错误文本推断故障。
final class ThreadStorageUpdate extends ThreadWorkspaceUpdate {
  const ThreadStorageUpdate({required this.storage});

  final ThreadStorageStateView? storage;
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

class ThreadHistoryPage {
  const ThreadHistoryPage({required this.items, required this.nextCursor});

  final List<ThreadItemView> items;
  final String? nextCursor;
}

enum TimelineQueryKind { latest, before, after, around }

class TimelinePage {
  const TimelinePage({
    required this.threadId,
    required this.watermark,
    required this.items,
    this.olderCursor,
    this.newerCursor,
    this.firstItemId,
    this.lastItemId,
    this.turns = const [],
    this.databaseId = '',
    this.truncated = false,
    this.previews = const [],
  });
  final String threadId;

  /// 该页来自哪个 history 数据库实体；游标身份校验使用。
  final String databaseId;
  final int watermark;
  final List<ThreadItemView> items;
  final String? olderCursor;
  final String? newerCursor;
  final String? firstItemId;
  final String? lastItemId;
  final List<TimelineTurnView> turns;

  /// 是否因为总字节预算在条目上限前截断。
  final bool truncated;

  /// 因超过单条预览预算而只以预览返回的条目引用。
  final List<TimelineItemPreviewView> previews;
}

/// 一条超大条目在页面中只以预览呈现时的显式引用；身份与 ordinal 不变。
class TimelineItemPreviewView {
  const TimelineItemPreviewView({
    required this.itemId,
    required this.ordinal,
    required this.revision,
    required this.totalBytes,
    required this.previewBytes,
    required this.omittedBytes,
  });

  final String itemId;
  final int ordinal;
  final int revision;
  final int totalBytes;
  final int previewBytes;
  final int omittedBytes;
}

TimelineTurnView _timelineTurnFromFrb(frb.BridgeTimelineTurn entry) =>
    TimelineTurnView(
      turn: _turnFromFrb(entry.turn),
      lastItemId: entry.lastItemId,
      disposition:
          entry.contextDisposition ==
              frb.BridgeThreadContextDisposition.rolledBack
          ? ThreadContextDisposition.rolledBack
          : ThreadContextDisposition.active,
    );

/// 分页与按 identity 回源共用同一投影：条目、Turn 摘要、database identity、
/// watermark 与预览引用都来自同一个 page 形状。
TimelinePage _timelinePageFromFrb(frb.BridgeTimelinePage page) {
  final turns = page.turns.map(_timelineTurnFromFrb).toList();
  final dispositions = {
    for (final entry in turns) entry.turn.turnId: entry.disposition,
  };
  return TimelinePage(
    threadId: page.threadId,
    databaseId: page.databaseId,
    watermark: page.watermark.toInt(),
    items: [
      for (final item in page.items)
        _threadItemFromFrb(
          item,
          contextDisposition:
              dispositions[item.turnId] ?? ThreadContextDisposition.active,
        ),
    ],
    olderCursor: page.olderCursor,
    newerCursor: page.newerCursor,
    firstItemId: page.firstItemId,
    lastItemId: page.lastItemId,
    truncated: page.truncated,
    previews: [
      for (final preview in page.previews)
        TimelineItemPreviewView(
          itemId: preview.itemId,
          ordinal: preview.ordinal.toInt(),
          revision: preview.revision.toInt(),
          totalBytes: preview.totalBytes.toInt(),
          previewBytes: preview.previewBytes.toInt(),
          omittedBytes: preview.omittedBytes.toInt(),
        ),
    ],
    turns: turns,
  );
}

/// 订阅首帧只投影当前状态；Timeline 窗口由 `listTimelineItems` 与实时通知维护，
/// 因此这里不产生任何条目、Turn 摘要或回源锚点。
ThreadWorkspace _threadWorkspaceFromSnapshot(frb.BridgeThreadSnapshot value) {
  return ThreadWorkspace(
    thread: _threadFromFrb(value.thread),
    revision: value.revision.toInt(),
    items: const [],
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
    activity: value.activity == null
        ? null
        : _threadActivityFromFrb(value.activity!),
    storage: value.storage == null
        ? null
        : _threadStorageFromFrb(value.storage!),
  );
}

ThreadActivityView _threadActivityFromFrb(
  frb_activity.BridgeThreadActivity value,
) {
  return ThreadActivityView(
    threadId: value.threadId,
    identity: value.identity,
    revision: value.revision.toInt(),
    turnId: value.turnId,
    inputId: value.inputId,
    attemptId: value.attemptId,
    kind: switch (value.kind) {
      frb_activity.BridgeThreadActivityKind.preparing =>
        ThreadActivityKind.preparing,
      frb_activity.BridgeThreadActivityKind.waitingApi =>
        ThreadActivityKind.waitingApi,
      frb_activity.BridgeThreadActivityKind.thinking =>
        ThreadActivityKind.thinking,
      frb_activity.BridgeThreadActivityKind.responding =>
        ThreadActivityKind.responding,
      frb_activity.BridgeThreadActivityKind.planning =>
        ThreadActivityKind.planning,
      frb_activity.BridgeThreadActivityKind.runningTool =>
        ThreadActivityKind.runningTool,
      frb_activity.BridgeThreadActivityKind.awaitingApproval =>
        ThreadActivityKind.awaitingApproval,
      frb_activity.BridgeThreadActivityKind.awaitingInput =>
        ThreadActivityKind.awaitingInput,
      frb_activity.BridgeThreadActivityKind.stopping =>
        ThreadActivityKind.stopping,
    },
    summary: value.summary,
    summaryTruncated: value.summaryTruncated,
    tools: ThreadActivityTools(
      count: value.tools.count,
      background: value.tools.background,
      active: [
        for (final tool in value.tools.active) _activityToolFromFrb(tool),
      ],
      latestStarted: value.tools.latestStarted == null
          ? null
          : _activityToolFromFrb(value.tools.latestStarted!),
    ),
  );
}

ThreadActivityToolEntry _activityToolFromFrb(
  frb_activity.BridgeThreadActivityToolEntry value,
) {
  return ThreadActivityToolEntry(
    callId: value.callId,
    taskId: value.taskId,
    name: value.name,
    summary: value.summary,
    arguments: switch (value.arguments) {
      frb_activity.BridgeThreadActivityArguments.commandLine =>
        ThreadActivityArgumentsKind.commandLine,
      frb_activity.BridgeThreadActivityArguments.opaque =>
        ThreadActivityArgumentsKind.opaque,
      frb_activity.BridgeThreadActivityArguments.streaming =>
        ThreadActivityArgumentsKind.streaming,
      frb_activity.BridgeThreadActivityArguments.unavailable =>
        ThreadActivityArgumentsKind.unavailable,
    },
    state: _activityToolStateFromFrb(value.state),
    ordinal: value.ordinal?.toInt(),
    startedAt: _frbNullableInt(value.startedAt),
  );
}

ThreadActivityToolState _activityToolStateFromFrb(
  frb_activity.BridgeThreadActivityToolState value,
) {
  return switch (value) {
    frb_activity.BridgeThreadActivityToolState.running =>
      ThreadActivityToolState.running,
    frb_activity.BridgeThreadActivityToolState.awaitingApproval =>
      ThreadActivityToolState.awaitingApproval,
    frb_activity.BridgeThreadActivityToolState.cancelling =>
      ThreadActivityToolState.cancelling,
    frb_activity.BridgeThreadActivityToolState.finished =>
      ThreadActivityToolState.finished,
  };
}

ThreadStorageStateView _threadStorageFromFrb(
  frb_activity.BridgeThreadStorageState value,
) {
  return ThreadStorageStateView(
    fault: switch (value.fault) {
      frb_activity.BridgeHistoryFault.queueFull => ThreadHistoryFault.queueFull,
      frb_activity.BridgeHistoryFault.writeFailed =>
        ThreadHistoryFault.writeFailed,
      frb_activity.BridgeHistoryFault.writerUnavailable =>
        ThreadHistoryFault.writerUnavailable,
      frb_activity.BridgeHistoryFault.noProgress =>
        ThreadHistoryFault.noProgress,
      frb_activity.BridgeHistoryFault.checkpointFailed =>
        ThreadHistoryFault.checkpointFailed,
      frb_activity.BridgeHistoryFault.blobFailed =>
        ThreadHistoryFault.blobFailed,
      null => null,
    },
    faultGeneration: value.faultGeneration.toInt(),
    acceptedSequence: value.acceptedSequence?.toInt(),
    durableSequence: value.durableSequence?.toInt(),
    execution: switch (value.execution) {
      frb_activity.BridgeThreadStorageExecution.running =>
        ThreadStorageExecution.running,
      frb_activity.BridgeThreadStorageExecution.pausing =>
        ThreadStorageExecution.pausing,
      frb_activity.BridgeThreadStorageExecution.paused =>
        ThreadStorageExecution.paused,
    },
    pressurePaused: value.pressurePaused,
    resumeRequired: value.resumeRequired,
    canResume: value.canResume,
    lastError: value.lastError,
  );
}

/// 按活动身份读取的完整详情映射；`revision` 为 `null` 表示纯内存推导，如实保留未知。
ThreadActivityDetail _activityDetailFromFrb(
  frb_activity.BridgeThreadActivityDetail value,
) {
  return value.when(
    current: (activity, reasoning, response, tools) =>
        CurrentThreadActivityDetail(
          activity: _threadActivityFromFrb(activity),
          reasoning: [
            for (final part in reasoning) _activityContentPartFromFrb(part),
          ],
          response: [
            for (final part in response) _activityContentPartFromFrb(part),
          ],
          tools: [for (final tool in tools) _activityToolDetailFromFrb(tool)],
        ),
    superseded: (activity, requestedActivityId) =>
        SupersededThreadActivityDetail(
          activity: _threadActivityFromFrb(activity),
          requestedActivityId: requestedActivityId,
        ),
    ended: (threadId, activityId) =>
        EndedThreadActivityDetail(threadId: threadId, activityId: activityId),
  );
}

ThreadActivityContentPart _activityContentPartFromFrb(
  frb_activity.BridgeThreadActivityContentPart value,
) {
  return ThreadActivityContentPart(
    itemId: value.itemId,
    revision: value.revision?.toInt(),
    complete: value.complete,
    text: value.text,
  );
}

ThreadActivityToolDetail _activityToolDetailFromFrb(
  frb_activity.BridgeThreadActivityToolDetail value,
) {
  return ThreadActivityToolDetail(
    callId: value.callId,
    taskId: value.taskId,
    name: value.name,
    state: _activityToolStateFromFrb(value.state),
    arguments: value.arguments,
    output: value.output,
    ordinal: value.ordinal?.toInt(),
    startedAt: _frbNullableInt(value.startedAt),
  );
}

StudioThread _threadFromFrb(frb.BridgeThread value) {
  return StudioThread(
    id: value.id,
    projectId: value.projectId,
    title: value.title,
    mode: ThreadModeId.fromId(value.mode),
    workspaceMode: ThreadWorkspaceMode.fromId(value.workspaceMode),
    workspacePath: value.workspacePath,
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
        frb_item.BridgeThreadTextChannel.parentAgent =>
          ThreadTextChannel.parentAgent,
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
        taskId: invocation.taskId,
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
    turn: (state, inputId) =>
        ThreadTurnItemStateView(_turnStateFromFrb(state), inputId: inputId),
    inference: (inferenceId, model, state) => ThreadInferenceItemStateView(
      inferenceId: inferenceId,
      model: model,
      lifecycle: _inferenceLifecycleFromFrb(state),
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
    raw: (payloads, notice, recordedAt) => ThreadRawItemStateView(
      payloads
          .map(
            (payload) => RawHistoryPayload(
              payload.format,
              payload.version,
              payload.content,
            ),
          )
          .toList(),
      notice,
      _dateFromUnix(recordedAt),
    ),
    contextCompaction: (beforeTokens, afterTokens, compactedAt) =>
        ThreadContextCompactionItemStateView(
          beforeTokens?.toInt(),
          afterTokens?.toInt(),
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
    modality: switch (value.modality) {
      frb_attachment_types.BridgeAttachmentModality.image =>
        AttachmentModalityView.image,
      frb_attachment_types.BridgeAttachmentModality.video =>
        AttachmentModalityView.video,
      frb_attachment_types.BridgeAttachmentModality.file =>
        AttachmentModalityView.file,
    },
    mediaType: value.mediaType,
    filename: value.filename,
    width: value.width,
    height: value.height,
    byteSize: value.byteSize.toInt(),
  );
}

ThreadToolLifecycleView _toolLifecycleFromFrb(
  frb_item.BridgeThreadToolState value,
) {
  return value.when(
    queued: () => const QueuedThreadToolView(),
    cancelling: (streamedOutput) => CancellingThreadToolView(streamedOutput),
    interrupted: (interruptedAt, reason) =>
        InterruptedThreadToolView(_dateFromUnix(interruptedAt), reason),
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
    attachments: value.attachments.map(_attachmentFromFrb).toList(),
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

StudioTurnView _turnFromFrb(frb.BridgeTurn value) {
  return StudioTurnView(
    inputId: value.inputId,
    turnId: value.id,
    threadId: value.threadId,
    revision: value.revision.toInt(),
    state: _turnStateFromFrb(value.state),
    updatedAt: _dateFromUnix(value.updatedAt),
  );
}

StudioTurnState _turnStateFromFrb(frb.BridgeTurnState value) {
  return value.when(
    queued: (queuedAt) => QueuedStudioTurnState(queuedAt: _frbInt(queuedAt)),
    running: (startedAt, phase) => RunningStudioTurnState(
      startedAt: _frbInt(startedAt),
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
      startedAt: _frbNullableInt(startedAt),
      completedAt: _frbInt(completedAt),
      completion: switch (completion) {
        frb.BridgeTurnCompletion.normal => StudioTurnCompletion.normal,
        frb.BridgeTurnCompletion.interactionRequested =>
          StudioTurnCompletion.interactionRequested,
      },
    ),
    cancelled: (startedAt, requestedAt, completedAt, cause) =>
        CancelledStudioTurnState(
          startedAt: _frbNullableInt(startedAt),
          requestedAt: _frbInt(requestedAt),
          completedAt: _frbInt(completedAt),
          cause: _turnCancellationCauseFromFrb(cause),
        ),
    failed: (startedAt, completedAt, failure) => FailedStudioTurnState(
      startedAt: _frbNullableInt(startedAt),
      completedAt: _frbInt(completedAt),
      failure: _turnFailureFromFrb(failure),
    ),
    budgetLimited: (startedAt, completedAt, limit, rollover) =>
        BudgetLimitedStudioTurnState(
          startedAt: _frbNullableInt(startedAt),
          completedAt: _frbInt(completedAt),
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
    unspecified: () => const UnspecifiedTurnCancellation(),
    userRequested: () => const UserRequestedTurnCancellation(),
    runtimeShutdown: () => const RuntimeShutdownTurnCancellation(),
    agentClosed: () => const AgentClosedTurnCancellation(),
    interrupted: () => const InterruptedTurnCancellation(),
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
    modelRoute: value.modelRoute == null
        ? null
        : ThreadModelRouteView(
            providerId: value.modelRoute!.providerId,
            model: value.modelRoute!.model,
            effort: value.modelRoute!.effort,
            revision: value.modelRoute!.revision.toInt(),
            available: value.modelRoute!.available,
            unavailableReason: value.modelRoute!.unavailableReason,
          ),
    contextTokens: usage.latestContextTokens.toInt(),
    contextWindow: usage.contextWindow?.toInt() ?? 0,
    totalTokens: usage.totalTokens.toInt(),
    promptTokens: usage.promptTokens.toInt(),
    completionTokens: usage.completionTokens.toInt(),
    cachedPromptTokens: usage.cachedPromptTokens.toInt(),
    cacheWriteTokens: usage.cacheWriteTokens.toInt(),
    reasoningTokens: usage.reasoningTokens.toInt(),
    inferenceCount: usage.inferenceCount.toInt(),
    cacheUsage: CacheUsageView(
      inputTokens: usage.cacheUsage.inputTokens.toInt(),
      cacheReadTokens: usage.cacheUsage.cacheReadTokens.toInt(),
      hitRate: usage.cacheUsage.hitRate,
      hasIncompleteUsage: usage.cacheUsage.hasIncompleteUsage,
    ),
    estimatedCosts: estimatedCosts,
    estimatedCacheSavings: usage.estimatedCacheSavings
        .map(
          (cost) =>
              RuntimeCostView(currency: cost.currency, amount: cost.amount),
        )
        .toList(growable: false),
    hasUnpricedUsage: usage.hasUnpricedUsage,
    hasIncompleteUsage: usage.hasIncompleteUsage,
    promptGeneration: usage.promptGeneration?.toInt(),
    promptCachePolicy: usage.promptCachePolicy,
    prefixChangedReason: usage.prefixChangedReason?.name,
    turnCompletionTokens: value.turnCompletionTokens.toInt(),
    turnDecodeMillis: value.turnDecodeMillis.toInt(),
    costLabel: costLabel,
    activeSkills: value.activeSkills,
    activeMcpServers: value.activeMcpServers,
    activeLspServers: value.activeLspServers,
    workflow: value.workflow == null
        ? null
        : _workflowRuntimeFromFrb(value.workflow!),
  );
}

WorkflowRuntimeView _workflowRuntimeFromFrb(
  frb.BridgeWorkflowRuntimeSnapshot value,
) {
  final run = value.currentRun;
  return WorkflowRuntimeView(
    revision: value.revision.toInt(),
    currentRun: run == null
        ? null
        : WorkflowRunView(
            lineageId: run.lineageId,
            runId: run.runId,
            modeId: run.modeId,
            graphRevision: run.graphRevision.toInt(),
            graphHash: run.graphHash,
            currentStateId: run.currentStateId,
            terminal: run.lifecycle == frb.BridgeWorkflowRunLifecycle.terminal,
            startedAt: _dateFromUnix(run.startedAt),
            updatedAt: _dateFromUnix(run.updatedAt),
          ),
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
  };
}
