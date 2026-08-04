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
    this.pendingTurnId,
  }) : error = null;

  const ComposerThreadState.idle({this.draft = '', this.submissionRevision = 0})
    : phase = ComposerSubmissionPhase.idle,
      pendingTurnId = null,
      error = null;

  const ComposerThreadState.failure({
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

  ComposerThreadState updateDraft(String value) {
    if (isSubmissionPending) {
      return this;
    }
    return ComposerThreadState.idle(
      draft: value,
      submissionRevision: submissionRevision,
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
      pendingTurnId: receipt.turnId,
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
    if (phase != ComposerSubmissionPhase.pendingStart ||
        turn?.turnId != pendingTurnId) {
      return this;
    }
    return ComposerThreadState.idle(submissionRevision: submissionRevision);
  }

  bool _matchesSubmittingRevision(int revision) =>
      phase == ComposerSubmissionPhase.submitting &&
      submissionRevision == revision;
}
