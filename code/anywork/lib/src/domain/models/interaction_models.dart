import 'studio_enums.dart';

const agentSessionPlanConfirmationQuestionId = 'plan_confirmation';
const agentSessionPlanApproveAnswer = 'Approve';
const agentSessionPlanReviseAnswer = 'Revise';

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

class PlanConfirmationView {
  const PlanConfirmationView({
    required this.interaction,
    required this.question,
  });

  final PendingInteraction interaction;
  final UserQuestionView question;

  String get interactionId => interaction.id;

  String get questionId => question.id;

  String get markdown => question.question;

  String get title {
    for (final line in markdown.split('\n')) {
      final trimmed = line.trim();
      if (trimmed.startsWith('# ')) {
        final value = trimmed.substring(2).trim();
        if (value.isNotEmpty) return value;
      }
    }
    return question.header.trim();
  }

  String get summary {
    for (final line in markdown.split('\n')) {
      final trimmed = line.trim();
      if (trimmed.isEmpty || trimmed.startsWith('#')) continue;
      return trimmed
          .replaceFirst(RegExp(r'^[-*+]\s+'), '')
          .replaceFirst(RegExp(r'^\d+[.)]\s+'), '')
          .trim();
    }
    return '';
  }

  @override
  bool operator ==(Object other) {
    return other is PlanConfirmationView &&
        other.interactionId == interactionId &&
        other.questionId == questionId &&
        other.markdown == markdown;
  }

  @override
  int get hashCode => Object.hash(interactionId, questionId, markdown);
}

extension PendingInteractionPlanPresentation on PendingInteraction {
  PlanConfirmationView? get planConfirmation {
    if (kind != InteractionKind.userInput ||
        payload is! UserInputInteractionPayload) {
      return null;
    }
    final questions = (payload as UserInputInteractionPayload).questions;
    final matches = questions
        .where(
          (question) => question.id == agentSessionPlanConfirmationQuestionId,
        )
        .toList(growable: false);
    if (matches.length != 1) return null;
    return PlanConfirmationView(interaction: this, question: matches.single);
  }
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

int interactionPriority(InteractionKind kind) {
  return switch (kind) {
    InteractionKind.toolApproval => 0,
    InteractionKind.userInput => 1,
  };
}
