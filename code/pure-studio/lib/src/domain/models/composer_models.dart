import 'turn_models.dart';

enum ComposerSubmissionPhase { idle, submitting, pendingStart }

class SubmitPromptReceipt {
  const SubmitPromptReceipt({
    required this.sessionId,
    required this.turnId,
    required this.cursor,
  });

  final String sessionId;
  final String turnId;
  final int cursor;
}

class ComposerSessionState {
  const ComposerSessionState._({
    required this.draft,
    required this.phase,
    this.pendingTurnId,
    this.error,
  });

  const ComposerSessionState.idle({this.draft = '', this.error})
    : phase = ComposerSubmissionPhase.idle,
      pendingTurnId = null;

  final String draft;
  final ComposerSubmissionPhase phase;
  final String? pendingTurnId;
  final String? error;

  bool get isSubmissionPending => phase != ComposerSubmissionPhase.idle;

  ComposerSessionState updateDraft(String value) {
    if (isSubmissionPending) {
      return this;
    }
    return ComposerSessionState.idle(draft: value);
  }

  ComposerSessionState beginSubmission() {
    if (isSubmissionPending || draft.trim().isEmpty) {
      return this;
    }
    return ComposerSessionState._(
      draft: draft,
      phase: ComposerSubmissionPhase.submitting,
    );
  }

  ComposerSessionState accept(SubmitPromptReceipt receipt) {
    return ComposerSessionState._(
      draft: '',
      phase: ComposerSubmissionPhase.pendingStart,
      pendingTurnId: receipt.turnId,
    );
  }

  ComposerSessionState fail(Exception exception) {
    return ComposerSessionState.idle(
      draft: draft,
      error: exception.toString(),
    );
  }

  ComposerSessionState observeTurn(StudioTurnView? turn) {
    if (phase != ComposerSubmissionPhase.pendingStart ||
        turn?.turnId != pendingTurnId) {
      return this;
    }
    return const ComposerSessionState.idle();
  }
}
