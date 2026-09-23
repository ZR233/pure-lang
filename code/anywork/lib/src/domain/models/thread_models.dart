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

/// SQL 历史页单条正文的客户端预览预算（UTF-16 code units）。
/// 运行中的正文完整保留在 overlay，待终态由 SQL 确认后释放。
const int kTimelineItemBodyBudget = 8 * 1024;

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
    this.summaryChunkBase = 0,
    this.contentChunkBase = 0,
  });

  final List<String> summary;
  final List<String> content;
  final ThreadContentLifecycleView lifecycle;

  /// 生产者 chunkIndex 是**逻辑**下标：本地列表因正文预算丢弃最旧整块后，下标整体
  /// 前移，必须靠 base 还原，后续 delta 才会写进正确的分块而不是错位或伪造缺口。
  final int summaryChunkBase;
  final int contentChunkBase;
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
    this.bodyOmittedUnits = 0,
    this.bodyLoaded = false,
    this.saved = true,
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

  /// 正文被客户端预算省略的 code units；0 表示内存里的正文是完整的。
  final int bodyOmittedUnits;

  /// 只有用户显式回源（`loadItemBody`）后为 true：完整正文才会驻留并整篇渲染。
  final bool bodyLoaded;

  /// Execution completion and history durability are independent.
  final bool saved;

  /// 条目正文超过预算、只以有界预览驻留：需要显式回源完整正文。
  bool get bodyPreviewed => bodyOmittedUnits > 0 && !bodyLoaded;

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
    ThreadItemStateView? nextState;
    var nextOmittedUnits = bodyOmittedUnits;
    switch ((state, delta)) {
      case (
        ThreadTextItemStateView(
          :final channel,
          :final text,
          :final attachments,
          lifecycle: StreamingThreadContentView(),
        ),
        ThreadTextDeltaView(:final delta),
      ):
        nextState = ThreadTextItemStateView(
          channel: channel,
          text: '$text$delta',
          attachments: attachments,
          lifecycle: const StreamingThreadContentView(),
        );
      case (
        ThreadThinkingItemStateView(
          :final summary,
          :final content,
          :final summaryChunkBase,
          :final contentChunkBase,
          lifecycle: StreamingThreadContentView(),
        ),
        ThreadThinkingSummaryDeltaView(:final chunkIndex, :final delta),
      ):
        final bounded = _boundedReasoningAppend(
          summary: summary,
          summaryChunkBase: summaryChunkBase,
          content: content,
          contentChunkBase: contentChunkBase,
          toSummary: true,
          chunkIndex: chunkIndex,
          delta: delta,
          omittedUnits: bodyOmittedUnits,
        );
        nextState = ThreadThinkingItemStateView(
          summary: bounded.summary,
          content: bounded.content,
          summaryChunkBase: bounded.summaryChunkBase,
          contentChunkBase: bounded.contentChunkBase,
          lifecycle: const StreamingThreadContentView(),
        );
        nextOmittedUnits = bounded.omittedUnits;
      case (
        ThreadThinkingItemStateView(
          :final summary,
          :final content,
          :final summaryChunkBase,
          :final contentChunkBase,
          lifecycle: StreamingThreadContentView(),
        ),
        ThreadThinkingContentDeltaView(:final chunkIndex, :final delta),
      ):
        final bounded = _boundedReasoningAppend(
          summary: summary,
          summaryChunkBase: summaryChunkBase,
          content: content,
          contentChunkBase: contentChunkBase,
          toSummary: false,
          chunkIndex: chunkIndex,
          delta: delta,
          omittedUnits: bodyOmittedUnits,
        );
        nextState = ThreadThinkingItemStateView(
          summary: bounded.summary,
          content: bounded.content,
          summaryChunkBase: bounded.summaryChunkBase,
          contentChunkBase: bounded.contentChunkBase,
          lifecycle: const StreamingThreadContentView(),
        );
        nextOmittedUnits = bounded.omittedUnits;
      case (
        ThreadToolItemStateView(
          :final invocation,
          lifecycle: StartedThreadToolView() || StreamingThreadToolView(),
        ),
        ThreadToolArgumentsDeltaView(:final delta),
      ):
        nextState = ThreadToolItemStateView(
          invocation: invocation.withArguments('${invocation.arguments}$delta'),
          lifecycle: const StreamingThreadToolView(),
        );
      case (
        ThreadToolItemStateView(
          :final invocation,
          lifecycle: RunningThreadToolView(:final streamedOutput),
        ),
        ThreadToolResultDeltaView(:final delta),
      ):
        nextState = ThreadToolItemStateView(
          invocation: invocation,
          lifecycle: RunningThreadToolView('$streamedOutput$delta'),
        );
      default:
        nextState = null;
    }
    return nextState == null
        ? null
        : copyWith(
            revision: nextRevision,
            state: nextState,
            bodyOmittedUnits: nextOmittedUnits,
          );
  }

  ThreadItemView copyWith({
    int? ordinal,
    int? revision,
    DateTime? updatedAt,
    ThreadItemStateView? state,
    ThreadContextDisposition? contextDisposition,
    int? bodyOmittedUnits,
    bool? bodyLoaded,
    bool? saved,
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
      bodyOmittedUnits: bodyOmittedUnits ?? this.bodyOmittedUnits,
      bodyLoaded: bodyLoaded ?? this.bodyLoaded,
      saved: saved ?? this.saved,
    );
  }
}

class ThreadWorkspace {
  const ThreadWorkspace({
    required this.thread,
    required this.revision,
    required List<ThreadItemView> items,
    required this.interactions,
    required this.runtime,
    this.activeTurn,
    this.latestTurn,
    this.liveItems = const {},
    this.timelineTurns = const {},
    this.todo,
  }) : historyItems = items;

  /// Thread 身份；由 Thread directory 重绑，不作为 mode/role/status 的事实源。
  final StudioThread thread;

  /// 当前状态 revision（snapshot 与实时事件共用）；历史页 watermark 不属于它。
  final int revision;

  /// SQL-backed reading window. Live frames never change this page.
  final List<ThreadItemView> historyItems;
  final Map<String, ThreadItemView> liveItems;

  /// Current visible history plus the live overlay, keyed by canonical item ID.
  List<ThreadItemView> get items {
    if (liveItems.isEmpty) return historyItems;
    final merged = <String, ThreadItemView>{
      for (final item in historyItems) item.id: item,
    };
    for (final item in liveItems.values) {
      final existing = merged[item.id];
      if (existing == null || item.revision >= existing.revision) {
        merged[item.id] = item;
      }
    }
    return merged.values.toList()..sort((a, b) {
      final order = a.ordinal.compareTo(b.ordinal);
      return order != 0 ? order : a.id.compareTo(b.id);
    });
  }

  final List<PendingInteraction> interactions;
  final ThreadRuntimeView runtime;

  /// 当前执行中的 Turn；Terminal Turn 不留在当前状态里。
  final StudioTurnView? activeTurn;

  /// 最近一次已知 Turn 事实（live turn 通知或历史页 turn 摘要）。
  final StudioTurnView? latestTurn;

  /// 窗口覆盖的 Turn 摘要（来自历史页），用于行投影与终态行去重。
  final Map<String, TimelineTurnView> timelineTurns;
  final TimelineTodoListUpdate? todo;

  /// 最近 Turn 事实：当前执行的 Turn 不早于已观测到的终态 Turn。
  StudioTurnView? get lastTurn {
    final observed = latestTurn;
    final active = activeTurn;
    if (active == null) return observed;
    if (observed == null || active.revision >= observed.revision) return active;
    return observed;
  }

  ThreadWorkspace copyWith({
    StudioThread? thread,
    int? revision,
    List<ThreadItemView>? items,
    List<PendingInteraction>? interactions,
    ThreadRuntimeView? runtime,
    Object? activeTurn = _workspaceUnset,
    Object? latestTurn = _workspaceUnset,
    Map<String, ThreadItemView>? liveItems,
    Map<String, TimelineTurnView>? timelineTurns,
    Object? todo = _workspaceUnset,
  }) {
    return ThreadWorkspace(
      thread: thread ?? this.thread,
      revision: revision ?? this.revision,
      items: items ?? historyItems,
      liveItems: liveItems ?? this.liveItems,
      timelineTurns: timelineTurns ?? this.timelineTurns,
      interactions: interactions ?? this.interactions,
      runtime: runtime ?? this.runtime,
      activeTurn: identical(activeTurn, _workspaceUnset)
          ? this.activeTurn
          : activeTurn as StudioTurnView?,
      latestTurn: identical(latestTurn, _workspaceUnset)
          ? this.latestTurn
          : latestTurn as StudioTurnView?,
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
    this.loadError,
    this.subscriptionGeneration = 0,
    this.history = const ThreadHistoryWindow(),
  });

  final ComposerThreadState composer;
  final AgentWorkspaceSyncState syncState;
  final String? loadError;
  final int subscriptionGeneration;
  final ThreadHistoryWindow history;

  WorkspaceUiState copyWith({
    ComposerThreadState? composer,
    AgentWorkspaceSyncState? syncState,
    String? loadError,
    int? subscriptionGeneration,
    ThreadHistoryWindow? history,
  }) {
    return WorkspaceUiState(
      composer: composer ?? this.composer,
      syncState: syncState ?? this.syncState,
      loadError:
          syncState == AgentWorkspaceSyncState.loading ||
              syncState == AgentWorkspaceSyncState.ready
          ? null
          : loadError ?? this.loadError,
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
    this.databaseId = '',
    this.appliedWriteSequence = 0,
    this.previewedItemIds = const {},
    this.loadingItemIds = const {},
    this.itemBodyErrors = const {},
    this.pendingItemBodyIds = const {},
    this.unavailableItemIds = const {},
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

  /// 当前窗口来自哪个 history 数据库实体；与页的 databaseId 不一致表示窗口过期。
  final String databaseId;

  /// 当前窗口已采纳的 applied write sequence；比它更旧的历史页必须拒绝。
  final int appliedWriteSequence;

  /// 窗口内因超单条预览预算而只以预览呈现的条目 ID。
  final Set<String> previewedItemIds;

  /// 正在按 item identity 回源完整正文的条目 ID。
  final Set<String> loadingItemIds;

  /// 回源完整正文失败的条目 ID 与其错误文案；下次成功回源时清除。
  final Map<String, String> itemBodyErrors;

  /// 回源请求已发出、但完整正文尚未可取（例如历史事务尚未 durable）的条目 ID。
  ///
  /// 这类条目**仍然可见可重试**：数据源没有给出完整正文，也没说身份不存在，因此只是
  /// “在途/尚未落盘”，不能像 [unavailableItemIds] 那样永久禁用入口。
  final Set<String> pendingItemBodyIds;

  /// 数据源明确无法解析该身份（真正缺席）的条目 ID；这些条目不再假装可回源。
  final Set<String> unavailableItemIds;
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
    String? databaseId,
    int? appliedWriteSequence,
    Set<String>? previewedItemIds,
    Set<String>? loadingItemIds,
    Map<String, String>? itemBodyErrors,
    Set<String>? pendingItemBodyIds,
    Set<String>? unavailableItemIds,
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
    databaseId: databaseId ?? this.databaseId,
    appliedWriteSequence: appliedWriteSequence ?? this.appliedWriteSequence,
    previewedItemIds: previewedItemIds ?? this.previewedItemIds,
    loadingItemIds: loadingItemIds ?? this.loadingItemIds,
    itemBodyErrors: itemBodyErrors ?? this.itemBodyErrors,
    pendingItemBodyIds: pendingItemBodyIds ?? this.pendingItemBodyIds,
    unavailableItemIds: unavailableItemIds ?? this.unavailableItemIds,
  );
}

const _workspaceUnset = Object();

/// 把一条从历史页/流进入客户端的条目收敛到客户端预算。
///
/// [previewOmittedUnits] 是该页声明的既有省略量（协议单条预览）：正文本身没有
/// 超过预算时它归零（该页已给出完整正文），超过预算时保留尾部并按实际丢弃量累计。
/// 已由用户显式回源（`bodyLoaded`）的条目保持完整正文，绝不被再次压缩。
ThreadItemView boundThreadItemBody(
  ThreadItemView item, {
  int previewOmittedUnits = 0,
}) {
  if (item.bodyLoaded) return item;
  switch (item.state) {
    case ThreadTextItemStateView(
      :final channel,
      :final text,
      :final attachments,
      :final lifecycle,
    ):
      final bounded = _boundedTail(text, previewOmittedUnits);
      return item.copyWith(
        state: ThreadTextItemStateView(
          channel: channel,
          text: bounded.text,
          attachments: attachments,
          lifecycle: lifecycle,
        ),
        bodyOmittedUnits: bounded.omittedUnits,
      );
    case ThreadThinkingItemStateView(
      :final summary,
      :final content,
      :final lifecycle,
    ):
      // 页/终态载荷是权威的完整分块序列：逻辑下标从 0 重新计算，两通道共享预算。
      final bounded = _boundedReasoningChannels(
        summary: summary,
        summaryChunkBase: 0,
        content: content,
        contentChunkBase: 0,
        omittedUnits: previewOmittedUnits,
      );
      return item.copyWith(
        state: ThreadThinkingItemStateView(
          summary: bounded.summary,
          content: bounded.content,
          summaryChunkBase: bounded.summaryChunkBase,
          contentChunkBase: bounded.contentChunkBase,
          lifecycle: lifecycle,
        ),
        bodyOmittedUnits: bounded.omittedUnits,
      );
    case ThreadToolItemStateView(:final invocation, :final lifecycle):
      final boundedArguments = _boundedTail(
        invocation.arguments,
        previewOmittedUnits,
      );
      final boundedCall = _boundToolLifecycle(lifecycle);
      return item.copyWith(
        state: ThreadToolItemStateView(
          invocation: invocation.withArguments(boundedArguments.text),
          lifecycle: boundedCall.lifecycle,
        ),
        bodyOmittedUnits:
            boundedArguments.omittedUnits + boundedCall.omittedUnits,
      );
    default:
      return item;
  }
}

({ThreadToolLifecycleView lifecycle, int omittedUnits}) _boundToolLifecycle(
  ThreadToolLifecycleView lifecycle,
) {
  switch (lifecycle) {
    case RunningThreadToolView(:final streamedOutput):
      final bounded = _boundedTail(streamedOutput, 0);
      return (
        lifecycle: RunningThreadToolView(bounded.text),
        omittedUnits: bounded.omittedUnits,
      );
    case CancellingThreadToolView(:final streamedOutput):
      final bounded = _boundedTail(streamedOutput, 0);
      return (
        lifecycle: CancellingThreadToolView(bounded.text),
        omittedUnits: bounded.omittedUnits,
      );
    case SucceededThreadToolView(:final completedAt, :final output):
      final bounded = _boundedTail(output.result, 0);
      return (
        lifecycle: SucceededThreadToolView(
          completedAt,
          ThreadToolOutputView(
            result: bounded.text,
            attachments: output.attachments,
            outputArtifacts: output.outputArtifacts,
            exitCode: output.exitCode,
          ),
        ),
        omittedUnits: bounded.omittedUnits,
      );
    case FailedThreadToolView(:final failedAt, :final failure, :final output):
      final bounded = _boundedTail(output?.result ?? '', 0);
      return (
        lifecycle: FailedThreadToolView(
          failedAt,
          failure,
          output == null
              ? null
              : ThreadToolOutputView(
                  result: bounded.text,
                  attachments: output.attachments,
                  outputArtifacts: output.outputArtifacts,
                  exitCode: output.exitCode,
                ),
        ),
        omittedUnits: bounded.omittedUnits,
      );
    default:
      return (lifecycle: lifecycle, omittedUnits: 0);
  }
}

/// 保留 [text] 的尾部并在 [baseOmittedUnits] 之上累计丢弃量。
({String text, int omittedUnits}) _boundedTail(
  String text,
  int baseOmittedUnits,
) {
  final drop = text.length - kTimelineItemBodyBudget;
  if (drop <= 0) return (text: text, omittedUnits: baseOmittedUnits);
  var start = drop;
  while (start < text.length && _isLowSurrogate(text.codeUnitAt(start))) {
    start += 1;
  }
  return (text: text.substring(start), omittedUnits: baseOmittedUnits + start);
}

/// 实时推理正文按 chunkIndex 完整追加；历史页的预算另行处理。
({
  List<String> summary,
  int summaryChunkBase,
  List<String> content,
  int contentChunkBase,
  int omittedUnits,
})
_boundedReasoningAppend({
  required List<String> summary,
  required int summaryChunkBase,
  required List<String> content,
  required int contentChunkBase,
  required bool toSummary,
  required int chunkIndex,
  required String delta,
  required int omittedUnits,
}) {
  var nextSummary = summary;
  var nextContent = content;
  var omitted = omittedUnits;
  final appended = _appendChunkAt(
    toSummary ? summary : content,
    toSummary ? summaryChunkBase : contentChunkBase,
    chunkIndex,
    delta,
  );
  if (appended == null) {
    omitted += delta.length;
  } else if (toSummary) {
    nextSummary = appended;
  } else {
    nextContent = appended;
  }
  return (
    summary: nextSummary,
    summaryChunkBase: summaryChunkBase,
    content: nextContent,
    contentChunkBase: contentChunkBase,
    omittedUnits: omitted,
  );
}

/// 把 delta 写入逻辑 [chunkIndex] 对应的本地分块。
///
/// 返回 null 表示该逻辑分块已不在保留窗口内（内容只累计省略量）；逻辑下标超出本地
/// 列表尾部意味着生产者真的缺块，仍抛出以保持既有缺口保护。
List<String>? _appendChunkAt(
  List<String> chunks,
  int chunkBase,
  int chunkIndex,
  String delta,
) {
  final local = chunkIndex - chunkBase;
  if (local < 0) return null;
  if (local > chunks.length) {
    throw StateError('Thread Item delta skipped an earlier chunk');
  }
  if (local == chunks.length) {
    return [...chunks, delta];
  }
  return [
    ...chunks.take(local),
    '${chunks[local]}$delta',
    ...chunks.skip(local + 1),
  ];
}

/// summary 与 content 合计不得超过 [kTimelineItemBodyBudget]。
///
/// 超出时从保留内容更多的一侧先丢弃最旧内容（整块优先，再裁首块头部），两个通道
/// 都保留最近的尾部，省略量精确累计。
({
  List<String> summary,
  int summaryChunkBase,
  List<String> content,
  int contentChunkBase,
  int omittedUnits,
})
_boundedReasoningChannels({
  required List<String> summary,
  required int summaryChunkBase,
  required List<String> content,
  required int contentChunkBase,
  required int omittedUnits,
}) {
  var nextSummary = summary;
  var nextSummaryBase = summaryChunkBase;
  var nextContent = content;
  var nextContentBase = contentChunkBase;
  var omitted = omittedUnits;
  while (true) {
    final summaryLength = _chunksLength(nextSummary);
    final contentLength = _chunksLength(nextContent);
    final excess = summaryLength + contentLength - kTimelineItemBodyBudget;
    if (excess <= 0) break;
    if (summaryLength >= contentLength && summaryLength > 0) {
      final dropped = _dropChunksHead(nextSummary, nextSummaryBase, excess);
      nextSummary = dropped.chunks;
      nextSummaryBase = dropped.chunkBase;
      omitted += dropped.droppedUnits;
    } else if (contentLength > 0) {
      final dropped = _dropChunksHead(nextContent, nextContentBase, excess);
      nextContent = dropped.chunks;
      nextContentBase = dropped.chunkBase;
      omitted += dropped.droppedUnits;
    } else {
      break;
    }
  }
  return (
    summary: nextSummary,
    summaryChunkBase: nextSummaryBase,
    content: nextContent,
    contentChunkBase: nextContentBase,
    omittedUnits: omitted,
  );
}

({List<String> chunks, int chunkBase, int droppedUnits}) _dropChunksHead(
  List<String> chunks,
  int chunkBase,
  int units,
) {
  var remaining = units;
  var dropped = 0;
  var index = 0;
  while (index < chunks.length && remaining >= chunks[index].length) {
    remaining -= chunks[index].length;
    dropped += chunks[index].length;
    index += 1;
  }
  var kept = chunks.sublist(index);
  if (remaining > 0 && kept.isNotEmpty) {
    final head = kept.first;
    var start = remaining;
    while (start < head.length && _isLowSurrogate(head.codeUnitAt(start))) {
      start += 1;
    }
    if (start > head.length) start = head.length;
    dropped += start;
    kept = [head.substring(start), ...kept.skip(1)];
  }
  return (chunks: kept, chunkBase: chunkBase + index, droppedUnits: dropped);
}

int _chunksLength(List<String> chunks) {
  var total = 0;
  for (final chunk in chunks) {
    total += chunk.length;
  }
  return total;
}

/// 代理对不能被切开：按 code unit 截断时跳过落在低位代理上的起点。
bool _isLowSurrogate(int codeUnit) => codeUnit >= 0xDC00 && codeUnit <= 0xDFFF;
