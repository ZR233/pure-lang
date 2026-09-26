import 'composer_models.dart';
import 'attachment_models.dart';
import 'agent_workspace_view.dart';
import 'interaction_models.dart';
import 'runtime_models.dart';
import 'studio_enums.dart';
import 'thread_activity_models.dart';
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

/// 内容字段身份（与后端 `BridgeContentField` 一一对应）。
///
/// 身份只由字段判别式决定，**不**从内容字符串推断：正文落在哪个 domain 字段完全由这里
/// 指定。推理分块用逻辑 `chunkIndex` 保持块身份，删除一块不会让其它块移动。
sealed class ThreadContentFieldView {
  const ThreadContentFieldView();
}

final class ThreadTextFieldView extends ThreadContentFieldView {
  const ThreadTextFieldView();
}

final class ThreadThinkingSummaryFieldView extends ThreadContentFieldView {
  const ThreadThinkingSummaryFieldView(this.chunkIndex);
  final int chunkIndex;
}

final class ThreadThinkingContentFieldView extends ThreadContentFieldView {
  const ThreadThinkingContentFieldView(this.chunkIndex);
  final int chunkIndex;
}

final class ThreadToolArgumentsFieldView extends ThreadContentFieldView {
  const ThreadToolArgumentsFieldView();
}

final class ThreadToolResultFieldView extends ThreadContentFieldView {
  const ThreadToolResultFieldView();
}

/// 一个字段的 typed 变化（与后端 `BridgeFieldChange` 一一对应）。
///
/// [`Append`] 只在前缀仍有效时到达，追加到本地已交付末尾；[`Replace`] 是权威替换（含
/// 预览升级为完整正文）；[`Remove`] 丢弃该字段的本地副本；[`Unchanged`] 正文不动，但所属
/// 条目的 revision / omitted / 保存水位仍要在整组应用后提交。
sealed class ThreadFieldChangeView {
  const ThreadFieldChangeView();
}

final class UnchangedThreadFieldChangeView extends ThreadFieldChangeView {
  const UnchangedThreadFieldChangeView();
}

final class AppendThreadFieldChangeView extends ThreadFieldChangeView {
  const AppendThreadFieldChangeView(this.text);
  final String text;
}

final class ReplaceThreadFieldChangeView extends ThreadFieldChangeView {
  const ReplaceThreadFieldChangeView(this.text);
  final String text;
}

final class RemoveThreadFieldChangeView extends ThreadFieldChangeView {
  const RemoveThreadFieldChangeView();
}

/// 同一 item 的一个字段变化；一次 `UpdateItem` 携带整组，**原子应用后一次提交**版本。
class ThreadFieldUpdateView {
  const ThreadFieldUpdateView({required this.field, required this.change});

  final ThreadContentFieldView field;
  final ThreadFieldChangeView change;
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
    this.executionTerminal = false,
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

  /// 窗口条目的执行终态事实（后端 `BridgeChatLifecycle`）。
  ///
  /// 与保存水位 [saved] 严格独立：终态可以尚未保存，已保存也仍可继续增量。终态身份
  /// 拒绝任何迟到的流式字段变化，只接受保存/省略量的版本帧。
  final bool executionTerminal;

  /// Execution completion and history durability are independent.
  final bool saved;

  /// 条目正文只以数据源声明的有界预览驻留：需要按身份补齐完整正文。
  bool get bodyPreviewed => bodyOmittedUnits > 0;

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

  /// 原子应用一帧 `BridgeViewChange::UpdateItem` 的整组字段变化。
  ///
  /// 语义（与后端/core 契约一致）：
  /// 1. 本地 `revision` 必须等于 [expectedRevision]，否则基线不连续 → 返回 null 让调用方
  ///    重建权威窗口，**不逐字段升版本**。
  /// 2. 逐个应用 `fields`：`Append` 追加到本地字段末尾、`Replace` 整体替换、`Remove` 丢弃本地
  ///    副本（推理块按 `chunkIndex` 删除，不移动其它块身份）、`Unchanged` 正文不动。
  /// 3. 全部字段应用完成后**一次**提交 [revision]、[omittedUnits] 与保存水位 [saved]；即使
  ///    字段全为 `Unchanged`（仅版本推进 / 仅保存确认）也要提交，不能当成空帧丢弃。
  ///
  /// 终态身份（[executionTerminal]）区分两类帧：
  /// - **执行内容更新**（流式 `Append` / `Remove` / 新 revision 的正文推进）一律拒绝，终态
  ///   不再改变执行内容。
  /// - **显示完整性升级**允许一次：同 identity、同 `revision`，且 [omittedUnits] 严格小于本地
  ///   省略量、字段只用 `Replace`（可含 `Unchanged`）携带权威完整正文。core `read_complete`
  ///   可能先 notify 一帧同 revision 的 preview→full `Replace`，再让 `expand` 重新对齐快照；
  ///   这条合法性不能靠 Reset 绕过。终态真正的迟到 `Append` / 新 revision 仍被拒绝。
  ThreadItemView? applyFieldUpdates({
    required List<ThreadFieldUpdateView> fields,
    required int expectedRevision,
    required int revision,
    required int omittedUnits,
    required bool saved,
    required bool terminal,
  }) {
    if (this.revision != expectedRevision) return null;
    if (terminal) {
      final unchangedOnly = fields.every(
        (update) => update.change is UnchangedThreadFieldChangeView,
      );
      // 仅保存确认帧：正文、revision 与省略量都不变，只翻转保存水位。
      final savedOnly =
          unchangedOnly &&
          revision == this.revision &&
          omittedUnits == bodyOmittedUnits;
      // 一次性的显示完整性升级：同 revision、省略量严格减少、只用权威 Replace 携带完整正文。
      final completenessUpgrade =
          revision == this.revision &&
          omittedUnits < bodyOmittedUnits &&
          fields.any(
            (update) => update.change is ReplaceThreadFieldChangeView,
          ) &&
          fields.every(
            (update) =>
                update.change is ReplaceThreadFieldChangeView ||
                update.change is UnchangedThreadFieldChangeView,
          );
      if (!savedOnly && !completenessUpgrade) return null;
    }
    var nextState = state;
    var nextOmitted = omittedUnits;
    for (final update in fields) {
      final applied = _applyFieldChange(
        nextState,
        update.field,
        update.change,
        nextOmitted,
      );
      if (applied == null) return null;
      nextState = applied.state;
      nextOmitted = applied.omittedUnits;
    }
    return copyWith(
      revision: revision,
      state: nextState,
      bodyOmittedUnits: nextOmitted,
      saved: saved,
      executionTerminal: terminal,
    );
  }

  /// 把一个 typed 字段变化落进当前条目状态；返回 null 表示字段身份与条目不匹配（重同步）。
  ({ThreadItemStateView state, int omittedUnits})? _applyFieldChange(
    ThreadItemStateView current,
    ThreadContentFieldView field,
    ThreadFieldChangeView change,
    int omittedUnits,
  ) {
    switch (current) {
      case ThreadTextItemStateView(
        :final channel,
        :final text,
        :final attachments,
        :final lifecycle,
      ):
        if (field is! ThreadTextFieldView) return null;
        return (
          state: ThreadTextItemStateView(
            channel: channel,
            text: _applyTextChange(text, change),
            attachments: attachments,
            lifecycle: lifecycle,
          ),
          omittedUnits: omittedUnits,
        );
      case ThreadThinkingItemStateView(
        :final summary,
        :final content,
        :final summaryChunkBase,
        :final contentChunkBase,
        :final lifecycle,
      ):
        final addressed = switch (field) {
          ThreadThinkingSummaryFieldView(:final chunkIndex) => (
            isSummary: true,
            chunkIndex: chunkIndex,
          ),
          ThreadThinkingContentFieldView(:final chunkIndex) => (
            isSummary: false,
            chunkIndex: chunkIndex,
          ),
          _ => null,
        };
        if (addressed == null) return null;
        final isSummary = addressed.isSummary;
        final chunkIndex = addressed.chunkIndex;
        final changed = _applyChunkChange(
          isSummary ? summary : content,
          isSummary ? summaryChunkBase : contentChunkBase,
          chunkIndex,
          change,
        );
        if (changed == null) return null;
        return (
          state: ThreadThinkingItemStateView(
            summary: isSummary ? changed.chunks : summary,
            content: isSummary ? content : changed.chunks,
            summaryChunkBase: summaryChunkBase,
            contentChunkBase: contentChunkBase,
            lifecycle: lifecycle,
          ),
          omittedUnits: omittedUnits,
        );
      case ThreadToolItemStateView(:final invocation, :final lifecycle):
        switch (field) {
          case ThreadToolArgumentsFieldView():
            return (
              state: ThreadToolItemStateView(
                invocation: invocation.withArguments(
                  _applyTextChange(invocation.arguments, change),
                ),
                lifecycle: lifecycle,
              ),
              omittedUnits: omittedUnits,
            );
          case ThreadToolResultFieldView():
            return (
              state: ThreadToolItemStateView(
                invocation: invocation,
                lifecycle: _applyToolOutput(lifecycle, change),
              ),
              omittedUnits: omittedUnits,
            );
          default:
            return null;
        }
      default:
        return null;
    }
  }

  ThreadItemView copyWith({
    int? ordinal,
    int? revision,
    DateTime? updatedAt,
    ThreadItemStateView? state,
    ThreadContextDisposition? contextDisposition,
    int? bodyOmittedUnits,
    bool? executionTerminal,
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
      executionTerminal: executionTerminal ?? this.executionTerminal,
      saved: saved ?? this.saved,
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
    this.latestTurn,
    this.todo,
    this.activity,
    this.storage,
  });

  /// Thread 身份；由 Thread directory 重绑，不作为 mode/role/status 的事实源。
  final StudioThread thread;

  /// 当前状态 revision（snapshot 与实时事件共用）；历史页 watermark 不属于它。
  final int revision;

  /// 消息窗口的唯一正文：canonical 有界窗口（由 ChatView 交付），不叠加任何 state
  /// snapshot 正文或第二份 live overlay。
  final List<ThreadItemView> items;

  final List<PendingInteraction> interactions;
  final ThreadRuntimeView runtime;

  /// 当前执行中的 Turn；Terminal Turn 不留在当前状态里。
  final StudioTurnView? activeTurn;

  /// 最近一次已知 Turn 事实（live turn 通知）。
  final StudioTurnView? latestTurn;

  final TimelineTodoListUpdate? todo;

  /// 后端 typed 当前活动投影；`null` 表示当前没有活动。独立于消息窗口。
  final ThreadActivityView? activity;

  /// 后端 typed 存储状态；`null` 表示没有可报告的存储事实（不是“健康”）。
  final ThreadStorageStateView? storage;

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
    Object? todo = _workspaceUnset,
    Object? activity = _workspaceUnset,
    Object? storage = _workspaceUnset,
  }) {
    return ThreadWorkspace(
      thread: thread ?? this.thread,
      revision: revision ?? this.revision,
      items: items ?? this.items,
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
      activity: identical(activity, _workspaceUnset)
          ? this.activity
          : activity as ThreadActivityView?,
      storage: identical(storage, _workspaceUnset)
          ? this.storage
          : storage as ThreadStorageStateView?,
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
    this.activityDetail = const ThreadActivityDetailState(),
  });

  final ComposerThreadState composer;
  final AgentWorkspaceSyncState syncState;
  final String? loadError;
  final int subscriptionGeneration;
  final ThreadHistoryWindow history;

  /// 按活动身份缓存的完整详情读取状态；身份变化即失效。
  final ThreadActivityDetailState activityDetail;

  WorkspaceUiState copyWith({
    ComposerThreadState? composer,
    AgentWorkspaceSyncState? syncState,
    String? loadError,
    int? subscriptionGeneration,
    ThreadHistoryWindow? history,
    ThreadActivityDetailState? activityDetail,
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
      activityDetail: activityDetail ?? this.activityDetail,
    );
  }
}

/// 固定活动条展开详情的一次按需读取状态。
///
/// 以活动身份 + 版本为键：同一身份内版本前进即视为需要刷新；身份变化即失效。
/// 读取在途时只保留一个请求（调用方合并），旧身份的迟到结果不得覆盖新身份。
class ThreadActivityDetailState {
  const ThreadActivityDetailState({
    this.identity,
    this.revision = 0,
    this.loading = false,
    this.detail,
    this.error,
  });

  final String? identity;
  final int revision;
  final bool loading;
  final ThreadActivityDetail? detail;
  final String? error;

  bool matches(String identity) => this.identity == identity;

  /// 当前缓存是否已对同一身份的最新版本有效（无需重读）。
  bool covers(String identity, int revision) {
    if (identity.isEmpty || this.identity != identity) return false;
    if (loading) return true;
    return (detail != null || error != null) && this.revision >= revision;
  }

  ThreadActivityDetailState copyWith({
    Object? identity = _workspaceUnset,
    int? revision,
    bool? loading,
    Object? detail = _workspaceUnset,
    Object? error = _workspaceUnset,
  }) {
    return ThreadActivityDetailState(
      identity: identical(identity, _workspaceUnset)
          ? this.identity
          : identity as String?,
      revision: revision ?? this.revision,
      loading: loading ?? this.loading,
      detail: identical(detail, _workspaceUnset)
          ? this.detail
          : detail as ThreadActivityDetail?,
      error: identical(error, _workspaceUnset) ? this.error : error as String?,
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
/// 文本正文（智能体 / 用户 / parentAgent 消息）**永不折叠**：客户端不再按预算截断它，
/// 只有数据源声明的省略量（[previewOmittedUnits]，例如 durable history 的单条预览）
/// 会保留下来，作为“按身份展开完整正文”的依据（窗口的 `omittedBytes`）。推理与工具载荷
/// 仍收敛到客户端预算，它们的折叠语义由分组行表达。
ThreadItemView boundThreadItemBody(
  ThreadItemView item, {
  int previewOmittedUnits = 0,
}) {
  switch (item.state) {
    case ThreadTextItemStateView():
      return item.copyWith(bodyOmittedUnits: previewOmittedUnits);
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

/// 把一个字段变化作用在单段正文上（文本正文 / 工具参数 / 工具输出）。
///
/// `Append` 只在前缀仍有效时到达，直接拼接；`Replace` 是权威替换（含预览升级完整正文）；
/// `Remove` 丢弃本地副本（空串）；`Unchanged` 不动。
String _applyTextChange(String current, ThreadFieldChangeView change) {
  return switch (change) {
    UnchangedThreadFieldChangeView() => current,
    AppendThreadFieldChangeView(:final text) => '$current$text',
    ReplaceThreadFieldChangeView(:final text) => text,
    RemoveThreadFieldChangeView() => '',
  };
}

/// 把字段变化作用在推理分块列表的逻辑 [logicalIndex] 上。
///
/// 逻辑下标 < 已保留窗口时该块本地副本已被省略，丢弃即可；下标超出本地尾部意味着生产者
/// 真的缺块，返回 null 让调用方重同步；删除块只清空该逻辑下标，**不移动**其它块身份。
({List<String> chunks})? _applyChunkChange(
  List<String> chunks,
  int chunkBase,
  int logicalIndex,
  ThreadFieldChangeView change,
) {
  if (change is UnchangedThreadFieldChangeView) return (chunks: chunks);
  final local = logicalIndex - chunkBase;
  if (local < 0) return (chunks: chunks);
  if (local > chunks.length) return null;
  if (local == chunks.length) {
    return switch (change) {
      AppendThreadFieldChangeView(:final text) ||
      ReplaceThreadFieldChangeView(:final text) => (chunks: [...chunks, text]),
      RemoveThreadFieldChangeView() ||
      UnchangedThreadFieldChangeView() => (chunks: chunks),
    };
  }
  final applied = switch (change) {
    AppendThreadFieldChangeView(:final text) => '${chunks[local]}$text',
    ReplaceThreadFieldChangeView(:final text) => text,
    RemoveThreadFieldChangeView() => '',
    UnchangedThreadFieldChangeView() => chunks[local],
  };
  return (chunks: [...chunks.take(local), applied, ...chunks.skip(local + 1)]);
}

/// 把工具输出的字段变化落进工具生命周期：运行/取消沿用原变体；已成功/失败重建输出对象；
/// 尚无输出载体的变体在收到 `Append`/`Replace` 时进入运行态。
ThreadToolLifecycleView _applyToolOutput(
  ThreadToolLifecycleView lifecycle,
  ThreadFieldChangeView change,
) {
  if (change is UnchangedThreadFieldChangeView) return lifecycle;
  switch (lifecycle) {
    case RunningThreadToolView(:final streamedOutput):
      return RunningThreadToolView(_applyTextChange(streamedOutput, change));
    case CancellingThreadToolView(:final streamedOutput):
      return CancellingThreadToolView(_applyTextChange(streamedOutput, change));
    case SucceededThreadToolView(:final completedAt, :final output):
      return SucceededThreadToolView(
        completedAt,
        _withToolResult(output, _applyTextChange(output.result, change)),
      );
    case FailedThreadToolView(:final failedAt, :final failure, :final output):
      return FailedThreadToolView(
        failedAt,
        failure,
        output == null
            ? ThreadToolOutputView(
                result: _applyTextChange('', change),
                attachments: const [],
                outputArtifacts: const [],
              )
            : _withToolResult(output, _applyTextChange(output.result, change)),
      );
    default:
      if (change is RemoveThreadFieldChangeView) return lifecycle;
      return RunningThreadToolView(_applyTextChange('', change));
  }
}

ThreadToolOutputView _withToolResult(
  ThreadToolOutputView output,
  String result,
) {
  return ThreadToolOutputView(
    result: result,
    attachments: output.attachments,
    outputArtifacts: output.outputArtifacts,
    exitCode: output.exitCode,
  );
}
