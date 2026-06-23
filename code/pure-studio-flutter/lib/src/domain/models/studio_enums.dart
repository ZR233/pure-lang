enum PermissionMode { requestApproval, autoReview, fullAccess }

enum CompileMode { auto, plan }

enum TurnPhase {
  idle,
  queued,
  contextLoading,
  waitingForModel,
  streaming,
  waitingForInteraction,
  runningTool,
  completed,
  failed,
  cancelled,
}

enum TimelinePartType { text, reasoning, tool, plan, agent }

enum InteractionKind { toolApproval, userInput, planConfirmation }
