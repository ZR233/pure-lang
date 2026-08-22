enum StudioTurnStatus {
  queued,
  running,
  completed,
  cancelled,
  failed,
  budgetLimited,
}

enum StudioTurnActivity {
  preparing,
  thinking,
  responding,
  planning,
  runningTool,
  persisting,
}

sealed class StudioTurnState {
  const StudioTurnState();

  StudioTurnStatus get status;

  StudioTurnActivity? get activity => switch (this) {
    RunningStudioTurnState(:final activity) => activity,
    QueuedStudioTurnState() ||
    CompletedStudioTurnState() ||
    CancelledStudioTurnState() ||
    FailedStudioTurnState() ||
    BudgetLimitedStudioTurnState() => null,
  };

  String? get reason => switch (this) {
    CancelledStudioTurnState(:final cause) => cause.description,
    FailedStudioTurnState(:final failure) => failure.message,
    BudgetLimitedStudioTurnState(:final limit) =>
      'Turn budget limited: ${limit.kind.name}',
    QueuedStudioTurnState() ||
    RunningStudioTurnState() ||
    CompletedStudioTurnState() => null,
  };

  bool get isBusy => switch (this) {
    QueuedStudioTurnState() || RunningStudioTurnState() => true,
    CompletedStudioTurnState() ||
    CancelledStudioTurnState() ||
    FailedStudioTurnState() ||
    BudgetLimitedStudioTurnState() => false,
  };

  bool get isTerminal => !isBusy;
}

final class QueuedStudioTurnState extends StudioTurnState {
  const QueuedStudioTurnState({required this.queuedAt});

  final int queuedAt;

  @override
  StudioTurnStatus get status => StudioTurnStatus.queued;

  @override
  bool operator ==(Object other) =>
      other is QueuedStudioTurnState && other.queuedAt == queuedAt;

  @override
  int get hashCode => Object.hash(runtimeType, queuedAt);
}

final class RunningStudioTurnState extends StudioTurnState {
  const RunningStudioTurnState({
    required this.startedAt,
    required this.activity,
  });

  final int startedAt;

  @override
  final StudioTurnActivity activity;

  @override
  StudioTurnStatus get status => StudioTurnStatus.running;

  @override
  bool operator ==(Object other) =>
      other is RunningStudioTurnState &&
      other.startedAt == startedAt &&
      other.activity == activity;

  @override
  int get hashCode => Object.hash(runtimeType, startedAt, activity);
}

enum StudioTurnCompletion { normal, interactionRequested }

final class CompletedStudioTurnState extends StudioTurnState {
  const CompletedStudioTurnState({
    required this.startedAt,
    required this.completedAt,
    required this.completion,
  });

  final int? startedAt;
  final int completedAt;
  final StudioTurnCompletion completion;

  @override
  StudioTurnStatus get status => StudioTurnStatus.completed;

  @override
  bool operator ==(Object other) =>
      other is CompletedStudioTurnState &&
      other.startedAt == startedAt &&
      other.completedAt == completedAt &&
      other.completion == completion;

  @override
  int get hashCode =>
      Object.hash(runtimeType, startedAt, completedAt, completion);
}

sealed class StudioTurnCancellationCause {
  const StudioTurnCancellationCause();

  String get description => switch (this) {
    UserRequestedTurnCancellation() => 'Cancelled by user',
    RuntimeShutdownTurnCancellation() => 'Runtime shutdown',
    AgentClosedTurnCancellation() => 'Agent closed',
    RecoveryTurnCancellation() => 'Cancelled during recovery',
    CoalescedTurnCancellation(:final targetTurnId) =>
      'Merged into Turn $targetTurnId',
  };
}

final class UserRequestedTurnCancellation extends StudioTurnCancellationCause {
  const UserRequestedTurnCancellation();
}

final class RuntimeShutdownTurnCancellation
    extends StudioTurnCancellationCause {
  const RuntimeShutdownTurnCancellation();
}

final class AgentClosedTurnCancellation extends StudioTurnCancellationCause {
  const AgentClosedTurnCancellation();
}

final class RecoveryTurnCancellation extends StudioTurnCancellationCause {
  const RecoveryTurnCancellation();
}

final class CoalescedTurnCancellation extends StudioTurnCancellationCause {
  const CoalescedTurnCancellation({required this.targetTurnId});

  final String targetTurnId;

  @override
  bool operator ==(Object other) =>
      other is CoalescedTurnCancellation && other.targetTurnId == targetTurnId;

  @override
  int get hashCode => Object.hash(runtimeType, targetTurnId);
}

final class CancelledStudioTurnState extends StudioTurnState {
  const CancelledStudioTurnState({
    required this.startedAt,
    required this.requestedAt,
    required this.completedAt,
    required this.cause,
  });

  final int? startedAt;
  final int requestedAt;
  final int completedAt;
  final StudioTurnCancellationCause cause;

  @override
  StudioTurnStatus get status => StudioTurnStatus.cancelled;

  @override
  bool operator ==(Object other) =>
      other is CancelledStudioTurnState &&
      other.startedAt == startedAt &&
      other.requestedAt == requestedAt &&
      other.completedAt == completedAt &&
      other.cause == cause;

  @override
  int get hashCode =>
      Object.hash(runtimeType, startedAt, requestedAt, completedAt, cause);
}

final class FailedStudioTurnState extends StudioTurnState {
  const FailedStudioTurnState({
    required this.startedAt,
    required this.completedAt,
    required this.failure,
  });

  final int? startedAt;
  final int completedAt;
  final StudioTurnFailureView failure;

  @override
  StudioTurnStatus get status => StudioTurnStatus.failed;

  @override
  bool operator ==(Object other) =>
      other is FailedStudioTurnState &&
      other.startedAt == startedAt &&
      other.completedAt == completedAt &&
      other.failure == failure;

  @override
  int get hashCode => Object.hash(runtimeType, startedAt, completedAt, failure);
}

enum StudioTurnBudgetLimitKind {
  modelStep,
  toolCall,
  wait,
  wallClock,
  agentCount,
  agentDepth,
  finalization,
}

class StudioTurnBudgetUsage {
  const StudioTurnBudgetUsage({
    required this.modelSteps,
    required this.toolCalls,
    required this.waitCalls,
    required this.elapsedMs,
  });

  final int modelSteps;
  final int toolCalls;
  final int waitCalls;
  final int elapsedMs;

  @override
  bool operator ==(Object other) =>
      other is StudioTurnBudgetUsage &&
      other.modelSteps == modelSteps &&
      other.toolCalls == toolCalls &&
      other.waitCalls == waitCalls &&
      other.elapsedMs == elapsedMs;

  @override
  int get hashCode => Object.hash(modelSteps, toolCalls, waitCalls, elapsedMs);
}

class StudioTurnBudgetLimit {
  const StudioTurnBudgetLimit({required this.kind, required this.usage});

  final StudioTurnBudgetLimitKind kind;
  final StudioTurnBudgetUsage usage;

  @override
  bool operator ==(Object other) =>
      other is StudioTurnBudgetLimit &&
      other.kind == kind &&
      other.usage == usage;

  @override
  int get hashCode => Object.hash(kind, usage);
}

sealed class StudioTurnRolloverOutcome {
  const StudioTurnRolloverOutcome();
}

final class RolloverNotAttempted extends StudioTurnRolloverOutcome {
  const RolloverNotAttempted();
}

final class RolloverSucceeded extends StudioTurnRolloverOutcome {
  const RolloverSucceeded();
}

final class RolloverFailed extends StudioTurnRolloverOutcome {
  const RolloverFailed({required this.error});

  final String error;

  @override
  bool operator ==(Object other) =>
      other is RolloverFailed && other.error == error;

  @override
  int get hashCode => Object.hash(runtimeType, error);
}

final class BudgetLimitedStudioTurnState extends StudioTurnState {
  const BudgetLimitedStudioTurnState({
    required this.startedAt,
    required this.completedAt,
    required this.limit,
    required this.rollover,
  });

  final int? startedAt;
  final int completedAt;
  final StudioTurnBudgetLimit limit;
  final StudioTurnRolloverOutcome rollover;

  @override
  StudioTurnStatus get status => StudioTurnStatus.budgetLimited;

  @override
  bool operator ==(Object other) =>
      other is BudgetLimitedStudioTurnState &&
      other.startedAt == startedAt &&
      other.completedAt == completedAt &&
      other.limit == limit &&
      other.rollover == rollover;

  @override
  int get hashCode =>
      Object.hash(runtimeType, startedAt, completedAt, limit, rollover);
}

class StudioTurnView {
  const StudioTurnView({
    required this.turnId,
    required this.threadId,
    required this.revision,
    required this.state,
    required this.updatedAt,
  });

  final String turnId;
  final String threadId;
  final int revision;
  final StudioTurnState state;
  final DateTime updatedAt;

  StudioTurnFailureView? get failure => switch (state) {
    FailedStudioTurnState(:final failure) => failure,
    QueuedStudioTurnState() ||
    RunningStudioTurnState() ||
    CompletedStudioTurnState() ||
    CancelledStudioTurnState() ||
    BudgetLimitedStudioTurnState() => null,
  };

  @override
  bool operator ==(Object other) {
    return other is StudioTurnView &&
        other.turnId == turnId &&
        other.threadId == threadId &&
        other.revision == revision &&
        other.state == state &&
        other.updatedAt == updatedAt;
  }

  @override
  int get hashCode => Object.hash(turnId, threadId, revision, state, updatedAt);
}

class StudioTurnFailureView {
  const StudioTurnFailureView({
    required this.category,
    required this.providerKind,
    required this.code,
    required this.httpStatus,
    required this.message,
    required this.retryable,
    required this.retryAfterMs,
  });

  final String category;
  final String? providerKind;
  final String? code;
  final int? httpStatus;
  final String message;
  final bool retryable;
  final int? retryAfterMs;

  @override
  bool operator ==(Object other) =>
      other is StudioTurnFailureView &&
      other.category == category &&
      other.providerKind == providerKind &&
      other.code == code &&
      other.httpStatus == httpStatus &&
      other.message == message &&
      other.retryable == retryable &&
      other.retryAfterMs == retryAfterMs;

  @override
  int get hashCode => Object.hash(
    category,
    providerKind,
    code,
    httpStatus,
    message,
    retryable,
    retryAfterMs,
  );
}
