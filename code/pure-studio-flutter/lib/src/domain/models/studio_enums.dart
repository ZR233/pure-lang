enum PermissionMode { requestApproval, autoReview, fullAccess }

enum StudioMode { simple, task }

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

enum TimelinePartType {
  text,
  reasoning,
  tool,
  plan,
  agent,
  turn,
  inference,
  file,
}

bool isInternalTimelinePartType(TimelinePartType type) {
  return switch (type) {
    TimelinePartType.turn ||
    TimelinePartType.inference ||
    TimelinePartType.file => true,
    TimelinePartType.text ||
    TimelinePartType.reasoning ||
    TimelinePartType.tool ||
    TimelinePartType.plan ||
    TimelinePartType.agent => false,
  };
}

enum InteractionKind { toolApproval, userInput, planConfirmation }
