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
    required this.submissionRevision,
    this.pendingTurnId,
  }) : error = null;

  const ComposerSessionState.idle({
    this.draft = '',
    this.submissionRevision = 0,
  }) : phase = ComposerSubmissionPhase.idle,
       pendingTurnId = null,
       error = null;

  const ComposerSessionState.failure({
    required this.error,
    this.draft = '',
    this.submissionRevision = 0,
  }) : phase = ComposerSubmissionPhase.idle,
       pendingTurnId = null;

  final String draft;
  final ComposerSubmissionPhase phase;
  final int submissionRevision;
  final String? pendingTurnId;
  final String? error;

  bool get isSubmissionPending => phase != ComposerSubmissionPhase.idle;

  ComposerSessionState updateDraft(String value) {
    if (isSubmissionPending) {
      return this;
    }
    return ComposerSessionState.idle(
      draft: value,
      submissionRevision: submissionRevision,
    );
  }

  ComposerSessionState beginSubmission() {
    if (isSubmissionPending || draft.trim().isEmpty) {
      return this;
    }
    return _startSubmission();
  }

  ComposerSessionState beginCommandSubmission() {
    if (isSubmissionPending) {
      return this;
    }
    return _startSubmission();
  }

  ComposerSessionState _startSubmission() {
    return ComposerSessionState._(
      draft: draft,
      phase: ComposerSubmissionPhase.submitting,
      submissionRevision: submissionRevision + 1,
    );
  }

  ComposerSessionState accept(
    SubmitPromptReceipt receipt, {
    required int submissionRevision,
  }) {
    if (!_matchesSubmittingRevision(submissionRevision)) {
      return this;
    }
    return ComposerSessionState._(
      draft: '',
      phase: ComposerSubmissionPhase.pendingStart,
      submissionRevision: this.submissionRevision,
      pendingTurnId: receipt.turnId,
    );
  }

  ComposerSessionState fail(Object error, {required int submissionRevision}) {
    if (!_matchesSubmittingRevision(submissionRevision)) {
      return this;
    }
    return ComposerSessionState.failure(
      draft: draft,
      error: error.toString(),
      submissionRevision: this.submissionRevision,
    );
  }

  ComposerSessionState observeTurn(StudioTurnView? turn) {
    if (phase != ComposerSubmissionPhase.pendingStart ||
        turn?.turnId != pendingTurnId) {
      return this;
    }
    return ComposerSessionState.idle(submissionRevision: submissionRevision);
  }

  bool _matchesSubmittingRevision(int revision) =>
      phase == ComposerSubmissionPhase.submitting &&
      submissionRevision == revision;
}
