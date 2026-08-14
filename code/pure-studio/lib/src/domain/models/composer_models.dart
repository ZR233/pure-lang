import 'turn_models.dart';

enum ComposerSubmissionPhase { idle, submitting, pendingStart }

class SubmitPromptReceipt {
  const SubmitPromptReceipt({
    required this.threadId,
    required this.turnId,
    required this.cursor,
  });

  final String threadId;
  final String turnId;
  final int cursor;
}

class ComposerThreadState {
  const ComposerThreadState._({
    required this.draft,
    required this.phase,
    required this.submissionRevision,
    this.acceptedTurnId,
  }) : error = null;

  const ComposerThreadState.idle({
    this.draft = '',
    this.submissionRevision = 0,
    this.acceptedTurnId,
  }) : phase = ComposerSubmissionPhase.idle,
       error = null;

  const ComposerThreadState.failure({
    required this.error,
    this.draft = '',
    this.submissionRevision = 0,
  }) : phase = ComposerSubmissionPhase.idle,
       acceptedTurnId = null;

  final String draft;
  final ComposerSubmissionPhase phase;
  final int submissionRevision;
  final String? acceptedTurnId;
  final String? error;

  bool get isSubmissionPending => phase != ComposerSubmissionPhase.idle;

  ComposerThreadState updateDraft(String value) {
    if (isSubmissionPending) {
      return this;
    }
    return ComposerThreadState.idle(
      draft: value,
      submissionRevision: submissionRevision,
      acceptedTurnId: acceptedTurnId,
    );
  }

  ComposerThreadState beginSubmission() {
    if (isSubmissionPending || draft.trim().isEmpty) {
      return this;
    }
    return _startSubmission();
  }

  ComposerThreadState beginCommandSubmission() {
    if (isSubmissionPending) {
      return this;
    }
    return _startSubmission();
  }

  ComposerThreadState _startSubmission() {
    return ComposerThreadState._(
      draft: draft,
      phase: ComposerSubmissionPhase.submitting,
      submissionRevision: submissionRevision + 1,
      acceptedTurnId: acceptedTurnId,
    );
  }

  ComposerThreadState accept(
    SubmitPromptReceipt receipt, {
    required int submissionRevision,
  }) {
    if (!_matchesSubmittingRevision(submissionRevision)) {
      return this;
    }
    return ComposerThreadState._(
      draft: '',
      phase: ComposerSubmissionPhase.pendingStart,
      submissionRevision: this.submissionRevision,
      acceptedTurnId: receipt.turnId,
    );
  }

  ComposerThreadState fail(Object error, {required int submissionRevision}) {
    if (!_matchesSubmittingRevision(submissionRevision)) {
      return this;
    }
    return ComposerThreadState.failure(
      draft: draft,
      error: error.toString(),
      submissionRevision: this.submissionRevision,
    );
  }

  ComposerThreadState observeTurn(StudioTurnView? turn) {
    if (turn == null || turn.turnId != acceptedTurnId) {
      return this;
    }
    if (turn.state.status == StudioTurnStatus.failed) {
      final message = turn.failure?.message.trim();
      final reason = turn.state.reason?.trim();
      return ComposerThreadState.failure(
        draft: draft,
        error: message?.isNotEmpty == true
            ? message!
            : reason?.isNotEmpty == true
            ? reason!
            : 'Turn failed',
        submissionRevision: submissionRevision,
      );
    }
    if (turn.state.isTerminal) {
      return ComposerThreadState.idle(
        draft: draft,
        submissionRevision: submissionRevision,
      );
    }
    if (phase != ComposerSubmissionPhase.pendingStart) {
      return this;
    }
    return ComposerThreadState.idle(
      submissionRevision: submissionRevision,
      acceptedTurnId: acceptedTurnId,
    );
  }

  bool _matchesSubmittingRevision(int revision) =>
      phase == ComposerSubmissionPhase.submitting &&
      submissionRevision == revision;
}
