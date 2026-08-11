enum StudioTurnStatus { queued, inProgress, completed, failed, cancelled }

enum StudioTurnActivity {
  preparing,
  thinking,
  responding,
  planning,
  runningTool,
  persisting,
}

class StudioTurnState {
  const StudioTurnState._({required this.status, this.activity, this.reason});

  const StudioTurnState.queued() : this._(status: StudioTurnStatus.queued);

  const StudioTurnState.inProgress(StudioTurnActivity activity)
    : this._(status: StudioTurnStatus.inProgress, activity: activity);

  const StudioTurnState.completed()
    : this._(status: StudioTurnStatus.completed);

  const StudioTurnState.failed(String reason)
    : this._(status: StudioTurnStatus.failed, reason: reason);

  const StudioTurnState.cancelled(String reason)
    : this._(status: StudioTurnStatus.cancelled, reason: reason);

  final StudioTurnStatus status;
  final StudioTurnActivity? activity;
  final String? reason;

  bool get isBusy =>
      status == StudioTurnStatus.queued ||
      status == StudioTurnStatus.inProgress;

  bool get isTerminal => !isBusy;

  @override
  bool operator ==(Object other) {
    return other is StudioTurnState &&
        other.status == status &&
        other.activity == activity &&
        other.reason == reason;
  }

  @override
  int get hashCode => Object.hash(status, activity, reason);
}

class StudioTurnView {
  const StudioTurnView({
    required this.turnId,
    required this.threadId,
    required this.state,
    this.failure,
    required this.updatedAt,
  });

  final String turnId;
  final String threadId;
  final StudioTurnState state;
  final StudioTurnFailureView? failure;
  final DateTime updatedAt;

  @override
  bool operator ==(Object other) {
    return other is StudioTurnView &&
        other.turnId == turnId &&
        other.threadId == threadId &&
        other.state == state &&
        other.failure == failure &&
        other.updatedAt == updatedAt;
  }

  @override
  int get hashCode => Object.hash(turnId, threadId, state, failure, updatedAt);
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
