import 'composer_models.dart';
import 'attachment_models.dart';
import 'agent_workspace_view.dart';
import 'interaction_models.dart';
import 'runtime_models.dart';
import 'studio_enums.dart';
import 'thread_directory_models.dart';
import 'timeline_models.dart';
import 'turn_models.dart';

enum ThreadItemKind {
  raw,
  userMessage,
  parentAgentMessage,
  agentMessage,
  reasoning,
  toolCall,
  agent,
  turn,
  inference,
  skill,
  file,
  contextCompaction,
}

enum AgentMessageChannel { commentary, finalAnswer }

enum ThreadTextChannel { user, parentAgent, commentary, finalAnswer }

class ThreadAttachmentView {
  const ThreadAttachmentView({
    required this.id,
    required this.modality,
    required this.mediaType,
    required this.byteSize,
    this.filename,
    this.width,
    this.height,
  });

  final String id;
  final AttachmentModalityView modality;
  final String mediaType;
  final String? filename;
  final int? width;
  final int? height;
  final int byteSize;
}

sealed class ThreadItemDeltaStateView {
  const ThreadItemDeltaStateView();
}

final class ThreadTextDeltaView extends ThreadItemDeltaStateView {
  const ThreadTextDeltaView(this.delta);
  final String delta;
}

final class ThreadThinkingSummaryDeltaView extends ThreadItemDeltaStateView {
  const ThreadThinkingSummaryDeltaView(this.chunkIndex, this.delta);
  final int chunkIndex;
  final String delta;
}

final class ThreadThinkingContentDeltaView extends ThreadItemDeltaStateView {
  const ThreadThinkingContentDeltaView(this.chunkIndex, this.delta);
  final int chunkIndex;
  final String delta;
}

final class ThreadToolArgumentsDeltaView extends ThreadItemDeltaStateView {
  const ThreadToolArgumentsDeltaView(this.delta);
  final String delta;
}

final class ThreadToolResultDeltaView extends ThreadItemDeltaStateView {
  const ThreadToolResultDeltaView(this.delta);
  final String delta;
}

sealed class ThreadContentLifecycleView {
  const ThreadContentLifecycleView();

  String get status => switch (this) {
    StreamingThreadContentView() => 'streaming',
    CompletedThreadContentView() => 'completed',
    FailedThreadContentView() => 'failed',
    CancelledThreadContentView() => 'cancelled',
  };

  bool get isTerminal => this is! StreamingThreadContentView;

  DateTime? get terminalAt => switch (this) {
    StreamingThreadContentView() => null,
    CompletedThreadContentView(:final completedAt) => completedAt,
    FailedThreadContentView(:final failedAt) => failedAt,
    CancelledThreadContentView(:final cancelledAt) => cancelledAt,
  };

  String? get failure => switch (this) {
    FailedThreadContentView(:final error) => error,
    StreamingThreadContentView() ||
    CompletedThreadContentView() ||
    CancelledThreadContentView() => null,
  };
}

final class StreamingThreadContentView extends ThreadContentLifecycleView {
  const StreamingThreadContentView();
}

final class CompletedThreadContentView extends ThreadContentLifecycleView {
  const CompletedThreadContentView(this.completedAt);
  final DateTime completedAt;
}

final class FailedThreadContentView extends ThreadContentLifecycleView {
  const FailedThreadContentView(this.failedAt, this.error);
  final DateTime failedAt;
  final String error;
}

final class CancelledThreadContentView extends ThreadContentLifecycleView {
  const CancelledThreadContentView(this.cancelledAt, this.reason);
  final DateTime cancelledAt;
  final String reason;
}

sealed class ThreadItemStateView {
  const ThreadItemStateView();
}

final class ThreadTextItemStateView extends ThreadItemStateView {
  const ThreadTextItemStateView({
    required this.channel,
    required this.text,
    required this.attachments,
    required this.lifecycle,
  });

  final ThreadTextChannel channel;
  final String text;
  final List<ThreadAttachmentView> attachments;
  final ThreadContentLifecycleView lifecycle;
}

final class ThreadThinkingItemStateView extends ThreadItemStateView {
  const ThreadThinkingItemStateView({
    required this.summary,
    required this.content,
    required this.lifecycle,
  });

  final List<String> summary;
  final List<String> content;
  final ThreadContentLifecycleView lifecycle;
}

final class ThreadSkillItemStateView extends ThreadItemStateView {
  const ThreadSkillItemStateView({
    required this.name,
    required this.source,
    required this.providerId,
    required this.resourceBase,
    required this.cause,
    required this.activatedAt,
  });

  final String name;
  final String source;
  final String providerId;
  final SkillResourceBaseView resourceBase;
  final SkillActivationCauseView cause;
  final DateTime activatedAt;
}

enum SkillResourceBaseKind { directory, url, opaque }

class SkillResourceBaseView {
  const SkillResourceBaseView(this.kind, this.value);
  final SkillResourceBaseKind kind;
  final String value;
}

enum SkillActivationCauseKind { tool, userGesture }

class SkillActivationCauseView {
  const SkillActivationCauseView(this.kind, this.id);
  final SkillActivationCauseKind kind;
  final String id;
}

class ThreadToolInvocationView {
  const ThreadToolInvocationView({
    required this.toolCallId,
    required this.name,
    required this.arguments,
    this.callId,
    this.providerItemId,
    this.workingDirectory,
    this.taskId,
  });

  final String toolCallId;
  final String? callId;
  final String? providerItemId;
  final String name;
  final String arguments;
  final String? workingDirectory;
  final String? taskId;

  ThreadToolInvocationView withArguments(String arguments) {
    return ThreadToolInvocationView(
      toolCallId: toolCallId,
      callId: callId,
      providerItemId: providerItemId,
      name: name,
      arguments: arguments,
      workingDirectory: workingDirectory,
      taskId: taskId,
    );
  }
}

class ThreadToolOutputView {
  const ThreadToolOutputView({
    required this.result,
    required this.attachments,
    required this.outputArtifacts,
    this.exitCode,
  });

  final String result;
  final List<ThreadAttachmentView> attachments;
  final List<Object?> outputArtifacts;
  final int? exitCode;
}

enum ThreadToolFailureKindView { execution, timedOut, budgetLimited }

class ThreadToolFailureView {
  const ThreadToolFailureView({required this.kind, required this.message});
  final ThreadToolFailureKindView kind;
  final String message;
}

sealed class ThreadToolLifecycleView {
  const ThreadToolLifecycleView();

  String get status => switch (this) {
    QueuedThreadToolView() => 'queued',
    CancellingThreadToolView() => 'cancelling',
    InterruptedThreadToolView() => 'interrupted',
    StartedThreadToolView() => 'started',
    StreamingThreadToolView() => 'streaming',
    AwaitingApprovalThreadToolView() => 'awaitingApproval',
    ApprovedThreadToolView() => 'approved',
    RunningThreadToolView() => 'running',
    SucceededThreadToolView() => 'succeeded',
    FailedThreadToolView() => 'failed',
    DeniedThreadToolView() => 'denied',
    CancelledThreadToolView() => 'cancelled',
  };

  bool get isTerminal => switch (this) {
    InterruptedThreadToolView() => true,
    QueuedThreadToolView() || CancellingThreadToolView() => false,
    SucceededThreadToolView() ||
    FailedThreadToolView() ||
    DeniedThreadToolView() ||
    CancelledThreadToolView() => true,
    StartedThreadToolView() ||
    StreamingThreadToolView() ||
    AwaitingApprovalThreadToolView() ||
    ApprovedThreadToolView() ||
    RunningThreadToolView() => false,
  };
}

final class StartedThreadToolView extends ThreadToolLifecycleView {
  const StartedThreadToolView();
}

final class QueuedThreadToolView extends ThreadToolLifecycleView {
  const QueuedThreadToolView();
}

final class CancellingThreadToolView extends ThreadToolLifecycleView {
  const CancellingThreadToolView(this.streamedOutput);
  final String streamedOutput;
}

final class InterruptedThreadToolView extends ThreadToolLifecycleView {
  const InterruptedThreadToolView(this.interruptedAt, this.reason);
  final DateTime interruptedAt;
  final String reason;
}

final class StreamingThreadToolView extends ThreadToolLifecycleView {
  const StreamingThreadToolView();
}

final class AwaitingApprovalThreadToolView extends ThreadToolLifecycleView {
  const AwaitingApprovalThreadToolView();
}

final class ApprovedThreadToolView extends ThreadToolLifecycleView {
  const ApprovedThreadToolView();
}

final class RunningThreadToolView extends ThreadToolLifecycleView {
  const RunningThreadToolView(this.streamedOutput);
  final String streamedOutput;
}

final class SucceededThreadToolView extends ThreadToolLifecycleView {
  const SucceededThreadToolView(this.completedAt, this.output);
  final DateTime completedAt;
  final ThreadToolOutputView output;
}

final class FailedThreadToolView extends ThreadToolLifecycleView {
  const FailedThreadToolView(this.failedAt, this.failure, this.output);
  final DateTime failedAt;
  final ThreadToolFailureView failure;
  final ThreadToolOutputView? output;
}

final class DeniedThreadToolView extends ThreadToolLifecycleView {
  const DeniedThreadToolView(this.deniedAt, this.reason);
  final DateTime deniedAt;
  final String reason;
}

final class CancelledThreadToolView extends ThreadToolLifecycleView {
  const CancelledThreadToolView(this.cancelledAt, this.reason);
  final DateTime cancelledAt;
  final String reason;
}

final class ThreadToolItemStateView extends ThreadItemStateView {
  const ThreadToolItemStateView({
    required this.invocation,
    required this.lifecycle,
  });

  final ThreadToolInvocationView invocation;
  final ThreadToolLifecycleView lifecycle;
}

class ThreadAgentIdentityView {
  const ThreadAgentIdentityView({
    required this.id,
    required this.path,
    required this.role,
    required this.task,
    required this.depth,
    this.parentPath,
  });

  final String id;
  final String path;
  final String? parentPath;
  final String role;
  final String task;
  final int depth;
}

sealed class ThreadAgentLifecycleView {
  const ThreadAgentLifecycleView();
}

final class QueuedThreadAgentView extends ThreadAgentLifecycleView {
  const QueuedThreadAgentView();
}

final class RunningThreadAgentView extends ThreadAgentLifecycleView {
  const RunningThreadAgentView();
}

final class SucceededThreadAgentView extends ThreadAgentLifecycleView {
  const SucceededThreadAgentView(this.completedAt, this.summary);
  final DateTime completedAt;
  final String summary;
}

final class DeniedThreadAgentView extends ThreadAgentLifecycleView {
  const DeniedThreadAgentView(this.deniedAt, this.reason);
  final DateTime deniedAt;
  final String reason;
}

final class CancelledThreadAgentView extends ThreadAgentLifecycleView {
  const CancelledThreadAgentView(this.cancelledAt, this.reason);
  final DateTime cancelledAt;
  final String reason;
}

final class FailedThreadAgentView extends ThreadAgentLifecycleView {
  const FailedThreadAgentView(this.failedAt, this.error);
  final DateTime failedAt;
  final String error;
}

final class ThreadAgentItemStateView extends ThreadItemStateView {
  const ThreadAgentItemStateView({
    required this.identity,
    required this.lifecycle,
  });
  final ThreadAgentIdentityView identity;
  final ThreadAgentLifecycleView lifecycle;
}

final class ThreadTurnItemStateView extends ThreadItemStateView {
  const ThreadTurnItemStateView(this.state, {this.inputId});
  final StudioTurnState state;
  final String? inputId;
}

sealed class ThreadInferenceLifecycleView {
  const ThreadInferenceLifecycleView();
}

final class RunningThreadInferenceView extends ThreadInferenceLifecycleView {
  const RunningThreadInferenceView();
}

final class CompletedThreadInferenceView extends ThreadInferenceLifecycleView {
  const CompletedThreadInferenceView(this.completedAt, this.usage);
  final DateTime completedAt;
  final ThreadInferenceUsageView usage;
}

class ThreadInferenceUsageView {
  const ThreadInferenceUsageView({
    required this.promptTokens,
    required this.completionTokens,
    required this.cachedPromptTokens,
    required this.totalTokens,
  });
  final int promptTokens;
  final int completionTokens;
  final int cachedPromptTokens;
  final int totalTokens;
}

final class FailedThreadInferenceView extends ThreadInferenceLifecycleView {
  const FailedThreadInferenceView(this.failedAt, this.error);
  final DateTime failedAt;
  final String error;
}

final class CancelledThreadInferenceView extends ThreadInferenceLifecycleView {
  const CancelledThreadInferenceView(this.cancelledAt, this.reason);
  final DateTime cancelledAt;
  final String reason;
}

final class ThreadInferenceItemStateView extends ThreadItemStateView {
  const ThreadInferenceItemStateView({
    required this.inferenceId,
    required this.model,
    required this.lifecycle,
  });
  final String inferenceId;
  final String model;
  final ThreadInferenceLifecycleView lifecycle;
}

class RawHistoryPayload {
  const RawHistoryPayload(this.format, this.version, this.content);
  final String format;
  final int version;
  final String content;
}

final class ThreadRawItemStateView extends ThreadItemStateView {
  const ThreadRawItemStateView(this.payloads, this.notice, this.recordedAt);
  final List<RawHistoryPayload> payloads;
  final String notice;
  final DateTime recordedAt;
}

final class ThreadFileItemStateView extends ThreadItemStateView {
  const ThreadFileItemStateView(this.path, this.mediaType, this.completedAt);
  final String path;
  final String? mediaType;
  final DateTime completedAt;
}

final class ThreadContextCompactionItemStateView extends ThreadItemStateView {
  const ThreadContextCompactionItemStateView(
    this.beforeTokens,
    this.afterTokens,
    this.compactedAt,
  );
  final int? beforeTokens;
  final int? afterTokens;
  final DateTime compactedAt;
}

class ThreadItemView {
  const ThreadItemView({
    required this.id,
    required this.threadId,
    required this.turnId,
    required this.ordinal,
    required this.revision,
    required this.createdAt,
    required this.updatedAt,
    required this.state,
    this.contextDisposition = ThreadContextDisposition.active,
  });

  final String id;
  final String threadId;
  final String turnId;
  final int ordinal;
  final int revision;
  final DateTime createdAt;
  final DateTime updatedAt;
  final ThreadItemStateView state;
  final ThreadContextDisposition contextDisposition;

  ThreadItemKind get kind => switch (state) {
    ThreadTextItemStateView(:final channel) =>
      channel == ThreadTextChannel.user
          ? ThreadItemKind.userMessage
          : channel == ThreadTextChannel.parentAgent
          ? ThreadItemKind.parentAgentMessage
          : ThreadItemKind.agentMessage,
    ThreadThinkingItemStateView() => ThreadItemKind.reasoning,
    ThreadToolItemStateView() => ThreadItemKind.toolCall,
    ThreadAgentItemStateView() => ThreadItemKind.agent,
    ThreadTurnItemStateView() => ThreadItemKind.turn,
    ThreadInferenceItemStateView() => ThreadItemKind.inference,
    ThreadSkillItemStateView() => ThreadItemKind.skill,
    ThreadRawItemStateView() => ThreadItemKind.raw,
    ThreadFileItemStateView() => ThreadItemKind.file,
    ThreadContextCompactionItemStateView() => ThreadItemKind.contextCompaction,
  };

  String get status => switch (state) {
    ThreadTextItemStateView(:final lifecycle) ||
    ThreadThinkingItemStateView(:final lifecycle) => lifecycle.status,
    ThreadToolItemStateView(:final lifecycle) => lifecycle.status,
    ThreadAgentItemStateView(:final lifecycle) => switch (lifecycle) {
      QueuedThreadAgentView() => 'queued',
      RunningThreadAgentView() => 'running',
      SucceededThreadAgentView() => 'succeeded',
      DeniedThreadAgentView() => 'denied',
      CancelledThreadAgentView() => 'cancelled',
      FailedThreadAgentView() => 'failed',
    },
    ThreadTurnItemStateView(:final state) => state.status.name,
    ThreadInferenceItemStateView(:final lifecycle) => switch (lifecycle) {
      RunningThreadInferenceView() => 'running',
      CompletedThreadInferenceView() => 'completed',
      FailedThreadInferenceView() => 'failed',
      CancelledThreadInferenceView() => 'cancelled',
    },
    ThreadSkillItemStateView() ||
    ThreadFileItemStateView() ||
    ThreadRawItemStateView() ||
    ThreadContextCompactionItemStateView() => 'completed',
  };

  bool get isTerminal => switch (state) {
    ThreadTextItemStateView(:final lifecycle) ||
    ThreadThinkingItemStateView(:final lifecycle) => lifecycle.isTerminal,
    ThreadToolItemStateView(:final lifecycle) => lifecycle.isTerminal,
    ThreadAgentItemStateView(:final lifecycle) =>
      lifecycle is! QueuedThreadAgentView &&
          lifecycle is! RunningThreadAgentView,
    ThreadTurnItemStateView(:final state) => state.isTerminal,
    ThreadInferenceItemStateView(:final lifecycle) =>
      lifecycle is! RunningThreadInferenceView,
    ThreadSkillItemStateView() ||
    ThreadFileItemStateView() ||
    ThreadRawItemStateView() ||
    ThreadContextCompactionItemStateView() => true,
  };

  DateTime? get completedAt => switch (state) {
    ThreadTextItemStateView(:final lifecycle) ||
    ThreadThinkingItemStateView(:final lifecycle) => lifecycle.terminalAt,
    ThreadToolItemStateView(:final lifecycle) => switch (lifecycle) {
      InterruptedThreadToolView(:final interruptedAt) => interruptedAt,
      QueuedThreadToolView() || CancellingThreadToolView() => null,
      SucceededThreadToolView(:final completedAt) => completedAt,
      FailedThreadToolView(:final failedAt) => failedAt,
      DeniedThreadToolView(:final deniedAt) => deniedAt,
      CancelledThreadToolView(:final cancelledAt) => cancelledAt,
      StartedThreadToolView() ||
      StreamingThreadToolView() ||
      AwaitingApprovalThreadToolView() ||
      ApprovedThreadToolView() ||
      RunningThreadToolView() => null,
    },
    ThreadAgentItemStateView(:final lifecycle) => switch (lifecycle) {
      SucceededThreadAgentView(:final completedAt) => completedAt,
      DeniedThreadAgentView(:final deniedAt) => deniedAt,
      CancelledThreadAgentView(:final cancelledAt) => cancelledAt,
      FailedThreadAgentView(:final failedAt) => failedAt,
      QueuedThreadAgentView() || RunningThreadAgentView() => null,
    },
    ThreadTurnItemStateView(:final state) => switch (state) {
      CompletedStudioTurnState(:final completedAt) ||
      CancelledStudioTurnState(:final completedAt) ||
      FailedStudioTurnState(:final completedAt) ||
      BudgetLimitedStudioTurnState(
        :final completedAt,
      ) => DateTime.fromMillisecondsSinceEpoch(completedAt * 1000),
      QueuedStudioTurnState() || RunningStudioTurnState() => null,
    },
    ThreadInferenceItemStateView(:final lifecycle) => switch (lifecycle) {
      CompletedThreadInferenceView(:final completedAt) => completedAt,
      FailedThreadInferenceView(:final failedAt) => failedAt,
      CancelledThreadInferenceView(:final cancelledAt) => cancelledAt,
      RunningThreadInferenceView() => null,
    },
    ThreadSkillItemStateView(:final activatedAt) => activatedAt,
    ThreadRawItemStateView(:final recordedAt) => recordedAt,
    ThreadFileItemStateView(:final completedAt) => completedAt,
    ThreadContextCompactionItemStateView(:final compactedAt) => compactedAt,
  };

  String? get error => switch (state) {
    ThreadTextItemStateView(:final lifecycle) ||
    ThreadThinkingItemStateView(:final lifecycle) => lifecycle.failure,
    ThreadToolItemStateView(:final lifecycle) => switch (lifecycle) {
      FailedThreadToolView(:final failure) => failure.message,
      InterruptedThreadToolView(:final reason) => reason,
      _ => null,
    },
    ThreadAgentItemStateView(:final lifecycle) => switch (lifecycle) {
      FailedThreadAgentView(:final error) => error,
      _ => null,
    },
    ThreadTurnItemStateView(:final state) => state.reason,
    ThreadInferenceItemStateView(:final lifecycle) => switch (lifecycle) {
      FailedThreadInferenceView(:final error) => error,
      _ => null,
    },
    ThreadSkillItemStateView() ||
    ThreadFileItemStateView() ||
    ThreadRawItemStateView() ||
    ThreadContextCompactionItemStateView() => null,
  };

  String get text => switch (state) {
    ThreadRawItemStateView(:final notice) => notice,
    ThreadTextItemStateView(:final text) => text,
    ThreadAgentItemStateView(:final lifecycle) => switch (lifecycle) {
      SucceededThreadAgentView(:final summary) => summary,
      DeniedThreadAgentView(:final reason) ||
      CancelledThreadAgentView(:final reason) => reason,
      FailedThreadAgentView(:final error) => error,
      QueuedThreadAgentView() || RunningThreadAgentView() => '',
    },
    ThreadSkillItemStateView(:final name) => name,
    _ => '',
  };

  AgentMessageChannel? get channel => switch (state) {
    ThreadTextItemStateView(channel: ThreadTextChannel.commentary) =>
      AgentMessageChannel.commentary,
    ThreadTextItemStateView(channel: ThreadTextChannel.finalAnswer) =>
      AgentMessageChannel.finalAnswer,
    _ => null,
  };

  List<ThreadAttachmentView> get attachments => switch (state) {
    ThreadTextItemStateView(:final attachments) => attachments,
    _ => const [],
  };

  List<String> get reasoningSummary => switch (state) {
    ThreadThinkingItemStateView(:final summary) => summary,
    _ => const [],
  };

  List<String> get reasoningContent => switch (state) {
    ThreadThinkingItemStateView(:final content) => content,
    _ => const [],
  };

  String? get filePath => switch (state) {
    ThreadFileItemStateView(:final path) => path,
    _ => null,
  };

  String? get mediaType => switch (state) {
    ThreadFileItemStateView(:final mediaType) => mediaType,
    _ => null,
  };

  ThreadSkillItemStateView? get skill => switch (state) {
    final ThreadSkillItemStateView skill => skill,
    _ => null,
  };

  TimelineToolPart? get tool => switch (state) {
    ThreadToolItemStateView(:final invocation, :final lifecycle) =>
      TimelineToolPart(
        taskId: invocation.taskId,
        toolCallId: invocation.toolCallId,
        callId: invocation.callId,
        providerItemId: invocation.providerItemId,
        name: invocation.name,
        arguments: invocation.arguments,
        result: switch (lifecycle) {
          RunningThreadToolView(:final streamedOutput) ||
          CancellingThreadToolView(:final streamedOutput) => streamedOutput,
          SucceededThreadToolView(:final output) => output.result,
          FailedThreadToolView(:final output) => output?.result,
          _ => null,
        },
        outputArtifacts: switch (lifecycle) {
          SucceededThreadToolView(:final output) => output.outputArtifacts,
          FailedThreadToolView(:final output) =>
            output?.outputArtifacts ?? const [],
          _ => const [],
        },
        attachments: switch (lifecycle) {
          SucceededThreadToolView(:final output) => output.attachments,
          FailedThreadToolView(:final output) =>
            output?.attachments ?? const [],
          _ => const [],
        },
        exitCode: switch (lifecycle) {
          SucceededThreadToolView(:final output) => output.exitCode,
          FailedThreadToolView(:final output) => output?.exitCode,
          _ => null,
        },
        timedOut: switch (lifecycle) {
          FailedThreadToolView(
            failure: ThreadToolFailureView(
              kind: ThreadToolFailureKindView.timedOut,
            ),
          ) =>
            true,
          _ => false,
        },
        workingDirectory: invocation.workingDirectory,
        denialReason: switch (lifecycle) {
          DeniedThreadToolView(:final reason) => reason,
          _ => null,
        },
      ),
    _ => null,
  };

  ThreadItemView? appendDelta({
    required ThreadItemDeltaStateView delta,
    required int nextRevision,
  }) {
    if (nextRevision <= revision) {
      return this;
    }
    final nextState = switch ((state, delta)) {
      (
        ThreadTextItemStateView(
          :final channel,
          :final text,
          :final attachments,
          lifecycle: StreamingThreadContentView(),
        ),
        ThreadTextDeltaView(:final delta),
      ) =>
        ThreadTextItemStateView(
          channel: channel,
          text: '$text$delta',
          attachments: attachments,
          lifecycle: const StreamingThreadContentView(),
        ),
      (
        ThreadThinkingItemStateView(
          :final summary,
          :final content,
          lifecycle: StreamingThreadContentView(),
        ),
        ThreadThinkingSummaryDeltaView(:final chunkIndex, :final delta),
      ) =>
        ThreadThinkingItemStateView(
          summary: _appendChunk(summary, chunkIndex, delta),
          content: content,
          lifecycle: const StreamingThreadContentView(),
        ),
      (
        ThreadThinkingItemStateView(
          :final summary,
          :final content,
          lifecycle: StreamingThreadContentView(),
        ),
        ThreadThinkingContentDeltaView(:final chunkIndex, :final delta),
      ) =>
        ThreadThinkingItemStateView(
          summary: summary,
          content: _appendChunk(content, chunkIndex, delta),
          lifecycle: const StreamingThreadContentView(),
        ),
      (
        ThreadToolItemStateView(
          :final invocation,
          lifecycle: StartedThreadToolView() || StreamingThreadToolView(),
        ),
        ThreadToolArgumentsDeltaView(:final delta),
      ) =>
        ThreadToolItemStateView(
          invocation: invocation.withArguments('${invocation.arguments}$delta'),
          lifecycle: const StreamingThreadToolView(),
        ),
      (
        ThreadToolItemStateView(
          :final invocation,
          lifecycle: RunningThreadToolView(:final streamedOutput),
        ),
        ThreadToolResultDeltaView(:final delta),
      ) =>
        ThreadToolItemStateView(
          invocation: invocation,
          lifecycle: RunningThreadToolView('$streamedOutput$delta'),
        ),
      _ => null,
    };
    return nextState == null
        ? null
        : copyWith(revision: nextRevision, state: nextState);
  }

  ThreadItemView copyWith({
    int? ordinal,
    int? revision,
    DateTime? updatedAt,
    ThreadItemStateView? state,
    ThreadContextDisposition? contextDisposition,
  }) {
    return ThreadItemView(
      id: id,
      threadId: threadId,
      turnId: turnId,
      ordinal: ordinal ?? this.ordinal,
      revision: revision ?? this.revision,
      createdAt: createdAt,
      updatedAt: updatedAt ?? this.updatedAt,
      state: state ?? this.state,
      contextDisposition: contextDisposition ?? this.contextDisposition,
    );
  }
}

class ThreadWorkspace {
  const ThreadWorkspace({
    required this.thread,
    required this.revision,
    required this.items,
    required this.interactions,
    required this.runtime,
    this.activeTurn,
    this.observedLastTurn,
    this.cachedItems = const {},
    this.latestItemIds = const [],
    this.timelineTurns = const {},
    this.todo,
  });

  final StudioThread thread;
  final int revision;
  final List<ThreadItemView> items;
  final List<PendingInteraction> interactions;
  final ThreadRuntimeView runtime;
  final StudioTurnView? activeTurn;
  final StudioTurnView? observedLastTurn;
  final Map<String, ThreadItemView> cachedItems;
  final List<String> latestItemIds;
  final Map<String, TimelineTurnView> timelineTurns;
  List<ThreadItemView> get latestItems => [
    for (final id in latestItemIds) ?cachedItems[id],
  ];
  final TimelineTodoListUpdate? todo;

  /// Canonical latest turn, including terminal facts carried by thread items.
  StudioTurnView? get lastTurn {
    var observed = observedLastTurn;
    final active = activeTurn;
    if (active != null &&
        (observed == null || active.revision > observed.revision)) {
      observed = active;
    }
    ThreadItemView? latest;
    for (final item in items) {
      if (item.state is ThreadTurnItemStateView &&
          (latest == null ||
              item.ordinal > latest.ordinal ||
              (item.ordinal == latest.ordinal &&
                  item.revision > latest.revision))) {
        latest = item;
      }
    }
    if (latest == null ||
        (observed != null && observed.revision >= latest.revision)) {
      return observed;
    }
    final state = latest.state as ThreadTurnItemStateView;
    return StudioTurnView(
      inputId: state.inputId,
      turnId: latest.turnId,
      threadId: latest.threadId,
      revision: latest.revision,
      state: state.state,
      updatedAt: latest.updatedAt,
    );
  }

  ThreadWorkspace copyWith({
    StudioThread? thread,
    int? revision,
    List<ThreadItemView>? items,
    List<PendingInteraction>? interactions,
    ThreadRuntimeView? runtime,
    Object? activeTurn = _workspaceUnset,
    Object? observedLastTurn = _workspaceUnset,
    Map<String, ThreadItemView>? cachedItems,
    List<String>? latestItemIds,
    Map<String, TimelineTurnView>? timelineTurns,
    Object? todo = _workspaceUnset,
  }) {
    return ThreadWorkspace(
      thread: thread ?? this.thread,
      revision: revision ?? this.revision,
      items: items ?? this.items,
      cachedItems: cachedItems ?? this.cachedItems,
      latestItemIds: latestItemIds ?? this.latestItemIds,
      timelineTurns: timelineTurns ?? this.timelineTurns,
      interactions: interactions ?? this.interactions,
      runtime: runtime ?? this.runtime,
      activeTurn: identical(activeTurn, _workspaceUnset)
          ? this.activeTurn
          : activeTurn as StudioTurnView?,
      observedLastTurn: identical(observedLastTurn, _workspaceUnset)
          ? this.observedLastTurn
          : observedLastTurn as StudioTurnView?,
      todo: identical(todo, _workspaceUnset)
          ? this.todo
          : todo as TimelineTodoListUpdate?,
    );
  }
}

class WorkspaceUiState {
  const WorkspaceUiState({
    this.composer = const ComposerThreadState.idle(),
    this.syncState = AgentWorkspaceSyncState.loading,
    this.subscriptionGeneration = 0,
    this.history = const ThreadHistoryWindow(),
  });

  final ComposerThreadState composer;
  final AgentWorkspaceSyncState syncState;
  final int subscriptionGeneration;
  final ThreadHistoryWindow history;

  WorkspaceUiState copyWith({
    ComposerThreadState? composer,
    AgentWorkspaceSyncState? syncState,
    int? subscriptionGeneration,
    ThreadHistoryWindow? history,
  }) {
    return WorkspaceUiState(
      composer: composer ?? this.composer,
      syncState: syncState ?? this.syncState,
      subscriptionGeneration:
          subscriptionGeneration ?? this.subscriptionGeneration,
      history: history ?? this.history,
    );
  }
}

enum TimelineDirection { older, newer }

class TimelineTurnView {
  const TimelineTurnView({
    required this.turn,
    required this.lastItemId,
    this.disposition = ThreadContextDisposition.active,
  });
  final StudioTurnView turn;
  final String lastItemId;
  final ThreadContextDisposition disposition;
}

class TimelineAnchor {
  const TimelineAnchor(
    this.itemId,
    this.offset, {
    this.followingBottom = false,
  });
  final String itemId;
  final double offset;
  final bool followingBottom;
}

/// Reading-window ownership is independent from subscription ownership.
class ThreadHistoryWindow {
  const ThreadHistoryWindow({
    this.hasOlder = false,
    this.hasNewer = false,
    this.olderCursor,
    this.newerCursor,
    this.isLoading = false,
    this.direction = TimelineDirection.older,
    this.epoch = 0,
    this.errorMessage,
    this.newerError,
    this.detached = false,
    this.anchor,
  });
  final bool hasOlder;
  final bool hasNewer;
  final String? olderCursor;
  final String? newerCursor;
  final bool isLoading;
  final TimelineDirection direction;
  final int epoch;
  final String? errorMessage;
  final String? newerError;
  final bool detached;
  final TimelineAnchor? anchor;
  ThreadHistoryWindow copyWith({
    bool? hasOlder,
    bool? hasNewer,
    Object? olderCursor = _workspaceUnset,
    Object? newerCursor = _workspaceUnset,
    bool? isLoading,
    TimelineDirection? direction,
    int? epoch,
    Object? errorMessage = _workspaceUnset,
    Object? newerError = _workspaceUnset,
    bool? detached,
    Object? anchor = _workspaceUnset,
  }) => ThreadHistoryWindow(
    hasOlder: hasOlder ?? this.hasOlder,
    hasNewer: hasNewer ?? this.hasNewer,
    olderCursor: identical(olderCursor, _workspaceUnset)
        ? this.olderCursor
        : olderCursor as String?,
    newerCursor: identical(newerCursor, _workspaceUnset)
        ? this.newerCursor
        : newerCursor as String?,
    isLoading: isLoading ?? this.isLoading,
    direction: direction ?? this.direction,
    epoch: epoch ?? this.epoch,
    detached: detached ?? this.detached,
    errorMessage: identical(errorMessage, _workspaceUnset)
        ? this.errorMessage
        : errorMessage as String?,
    newerError: identical(newerError, _workspaceUnset)
        ? this.newerError
        : newerError as String?,
    anchor: identical(anchor, _workspaceUnset)
        ? this.anchor
        : anchor as TimelineAnchor?,
  );
}

const _workspaceUnset = Object();

List<String> _appendChunk(List<String> current, int chunkIndex, String delta) {
  if (chunkIndex < 0 || chunkIndex > current.length) {
    throw StateError('Thread Item delta skipped an earlier chunk');
  }
  if (chunkIndex == current.length) {
    return [...current, delta];
  }
  return [
    ...current.take(chunkIndex),
    '${current[chunkIndex]}$delta',
    ...current.skip(chunkIndex + 1),
  ];
}
