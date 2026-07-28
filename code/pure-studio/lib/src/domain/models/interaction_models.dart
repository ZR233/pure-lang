import 'studio_enums.dart';

class PendingInteraction {
  const PendingInteraction({
    required this.id,
    required this.sessionId,
    required this.kind,
    required this.title,
    required this.body,
    this.payload = const {},
  });

  final String id;
  final String sessionId;
  final InteractionKind kind;
  final String title;
  final String body;
  final Map<String, Object?> payload;
}

int interactionPriority(InteractionKind kind) {
  return switch (kind) {
    InteractionKind.toolApproval => 0,
    InteractionKind.userInput => 1,
    InteractionKind.planConfirmation => 2,
  };
}
