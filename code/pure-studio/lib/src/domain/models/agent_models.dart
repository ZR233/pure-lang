sealed class StudioAgentState {
  const StudioAgentState();

  bool get isActive => switch (this) {
    QueuedStudioAgent() ||
    RunningStudioAgent() ||
    WaitingToolStudioAgent() ||
    WaitingInteractionStudioAgent() ||
    CancellingStudioAgent() => true,
    IdleStudioAgent() ||
    ClosingStudioAgent() ||
    ClosedStudioAgent() ||
    FaultedStudioAgent() => false,
  };

  String get label => switch (this) {
    IdleStudioAgent() => 'idle',
    QueuedStudioAgent() => 'queued',
    RunningStudioAgent() => 'running',
    WaitingToolStudioAgent() => 'waitingTool',
    WaitingInteractionStudioAgent() => 'waitingInteraction',
    CancellingStudioAgent() => 'cancelling',
    ClosingStudioAgent() => 'closing',
    ClosedStudioAgent() => 'closed',
    FaultedStudioAgent() => 'faulted',
  };

  String? get errorMessage => switch (this) {
    FaultedStudioAgent(:final failure) => failure.message,
    IdleStudioAgent() ||
    QueuedStudioAgent() ||
    RunningStudioAgent() ||
    WaitingToolStudioAgent() ||
    WaitingInteractionStudioAgent() ||
    CancellingStudioAgent() ||
    ClosingStudioAgent() ||
    ClosedStudioAgent() => null,
  };

  @override
  bool operator ==(Object other) => runtimeType == other.runtimeType;

  @override
  int get hashCode => runtimeType.hashCode;
}

final class IdleStudioAgent extends StudioAgentState {
  const IdleStudioAgent();
}

final class QueuedStudioAgent extends StudioAgentState {
  const QueuedStudioAgent(this.turnId);
  final String turnId;

  @override
  bool operator ==(Object other) =>
      other is QueuedStudioAgent && turnId == other.turnId;

  @override
  int get hashCode => Object.hash(runtimeType, turnId);
}

final class RunningStudioAgent extends StudioAgentState {
  const RunningStudioAgent(this.turnId);
  final String turnId;

  @override
  bool operator ==(Object other) =>
      other is RunningStudioAgent && turnId == other.turnId;

  @override
  int get hashCode => Object.hash(runtimeType, turnId);
}

final class WaitingToolStudioAgent extends StudioAgentState {
  const WaitingToolStudioAgent(this.turnId);
  final String turnId;

  @override
  bool operator ==(Object other) =>
      other is WaitingToolStudioAgent && turnId == other.turnId;

  @override
  int get hashCode => Object.hash(runtimeType, turnId);
}

final class WaitingInteractionStudioAgent extends StudioAgentState {
  const WaitingInteractionStudioAgent(this.turnId, this.interactionId);
  final String turnId;
  final String interactionId;

  @override
  bool operator ==(Object other) =>
      other is WaitingInteractionStudioAgent &&
      turnId == other.turnId &&
      interactionId == other.interactionId;

  @override
  int get hashCode => Object.hash(runtimeType, turnId, interactionId);
}

final class CancellingStudioAgent extends StudioAgentState {
  const CancellingStudioAgent(this.turnId);
  final String turnId;

  @override
  bool operator ==(Object other) =>
      other is CancellingStudioAgent && turnId == other.turnId;

  @override
  int get hashCode => Object.hash(runtimeType, turnId);
}

final class ClosingStudioAgent extends StudioAgentState {
  const ClosingStudioAgent();
}

final class ClosedStudioAgent extends StudioAgentState {
  const ClosedStudioAgent();
}

final class FaultedStudioAgent extends StudioAgentState {
  const FaultedStudioAgent({required this.failure, this.diagnosticTurnId});

  final AgentStateError failure;
  final String? diagnosticTurnId;

  @override
  bool operator ==(Object other) =>
      other is FaultedStudioAgent &&
      failure == other.failure &&
      diagnosticTurnId == other.diagnosticTurnId;

  @override
  int get hashCode => Object.hash(runtimeType, failure, diagnosticTurnId);
}

class AgentStateError {
  const AgentStateError({
    required this.code,
    required this.message,
    required this.retryable,
  });

  final String code;
  final String message;
  final bool retryable;

  @override
  bool operator ==(Object other) =>
      other is AgentStateError &&
      code == other.code &&
      message == other.message &&
      retryable == other.retryable;

  @override
  int get hashCode => Object.hash(code, message, retryable);
}

class StudioAgentView {
  const StudioAgentView({
    required this.id,
    required this.threadId,
    required this.path,
    required this.role,
    required this.task,
    required this.state,
    required this.updatedAt,
    this.parentPath,
    this.summary,
    this.depth = 0,
    this.rootThreadId,
    this.progress,
    this.summaryAgeSeconds,
  });

  final String id;
  final String threadId;
  final String path;
  final String? parentPath;
  final String role;
  final String task;
  final String? summary;
  final int depth;
  final String? rootThreadId;
  final StudioAgentState state;
  final AgentProgressView? progress;
  final int? summaryAgeSeconds;
  final DateTime updatedAt;

  String get status => state.label;

  String? get error => state.errorMessage;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is StudioAgentView &&
          id == other.id &&
          threadId == other.threadId &&
          path == other.path &&
          parentPath == other.parentPath &&
          role == other.role &&
          task == other.task &&
          summary == other.summary &&
          depth == other.depth &&
          rootThreadId == other.rootThreadId &&
          state == other.state &&
          progress == other.progress &&
          summaryAgeSeconds == other.summaryAgeSeconds &&
          updatedAt == other.updatedAt;

  @override
  int get hashCode => Object.hash(
    id,
    threadId,
    path,
    parentPath,
    role,
    task,
    summary,
    depth,
    rootThreadId,
    state,
    progress,
    summaryAgeSeconds,
    updatedAt,
  );
}

class AgentProgressView {
  const AgentProgressView({
    required this.stage,
    required this.summary,
    required this.nextStep,
    required this.revision,
    required this.updatedAt,
  });

  final String stage;
  final String summary;
  final String nextStep;
  final int revision;
  final DateTime updatedAt;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is AgentProgressView &&
          stage == other.stage &&
          summary == other.summary &&
          nextStep == other.nextStep &&
          revision == other.revision &&
          updatedAt == other.updatedAt;

  @override
  int get hashCode =>
      Object.hash(stage, summary, nextStep, revision, updatedAt);
}
