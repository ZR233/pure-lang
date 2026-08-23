import 'studio_enums.dart';

sealed class InteractionPayload {
  const InteractionPayload();
}

class UnknownInteractionPayload extends InteractionPayload {
  const UnknownInteractionPayload();
}

class UserInputInteractionPayload extends InteractionPayload {
  const UserInputInteractionPayload({required this.questions});

  final List<UserQuestionView> questions;
}

class ToolApprovalInteractionPayload extends InteractionPayload {
  const ToolApprovalInteractionPayload({
    required this.toolName,
    this.arguments,
    required this.workingDirectory,
    this.parentAgentId,
  });

  final String toolName;
  final Object? arguments;
  final String workingDirectory;
  final String? parentAgentId;
}

class PlanConfirmationInteractionPayload extends InteractionPayload {
  const PlanConfirmationInteractionPayload({
    required this.planId,
    required this.content,
  });

  final String planId;
  final String content;
}

class UserQuestionView {
  const UserQuestionView({
    required this.id,
    required this.header,
    required this.question,
    required this.isOther,
    required this.isSecret,
    required this.options,
  });

  final String id;
  final String header;
  final String question;
  final bool isOther;
  final bool isSecret;
  final List<UserQuestionOptionView> options;
}

class UserQuestionOptionView {
  const UserQuestionOptionView({
    required this.label,
    required this.description,
  });

  final String label;
  final String description;
}

class PendingInteraction {
  const PendingInteraction({
    required this.id,
    required this.threadId,
    required this.turnId,
    required this.kind,
    required this.title,
    required this.body,
    this.payload = const UnknownInteractionPayload(),
  });

  final String id;
  final String threadId;
  final String turnId;
  final InteractionKind kind;
  final String title;
  final String body;
  final InteractionPayload payload;
}

class UserInputAnswerCommand {
  const UserInputAnswerCommand({
    required this.questionId,
    required this.answers,
  });

  final String questionId;
  final List<String> answers;
}

sealed class InteractionResolutionCommand {
  const InteractionResolutionCommand();
}

class UserInputResolutionCommand extends InteractionResolutionCommand {
  const UserInputResolutionCommand({required this.answers});

  final List<UserInputAnswerCommand> answers;
}

enum ToolApprovalDecision { approved, denied }

class ToolApprovalResolutionCommand extends InteractionResolutionCommand {
  const ToolApprovalResolutionCommand({required this.decision, this.reason});

  final ToolApprovalDecision decision;
  final String? reason;
}

enum PlanConfirmationDecision { confirm, revisePlan }

class PlanConfirmationResolutionCommand extends InteractionResolutionCommand {
  const PlanConfirmationResolutionCommand({
    required this.decision,
    this.content,
    this.reason,
  });

  final PlanConfirmationDecision decision;
  final String? content;
  final String? reason;
}

int interactionPriority(InteractionKind kind) {
  return switch (kind) {
    InteractionKind.toolApproval => 0,
    InteractionKind.userInput => 1,
    InteractionKind.planConfirmation => 2,
  };
}
