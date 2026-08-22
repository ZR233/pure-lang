import 'thread_directory_models.dart';
import 'turn_models.dart';

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

class StartNewThreadResult {
  const StartNewThreadResult({required this.thread, required this.receipt});

  final StudioThread thread;
  final SubmitPromptReceipt receipt;
}

class ArchiveThreadResult {
  const ArchiveThreadResult({
    required this.archivedRootId,
    required this.removedThreadIds,
    this.nextRoot,
  });

  final String archivedRootId;
  final List<String> removedThreadIds;
  final StudioThread? nextRoot;
}

sealed class ComposerThreadState {
  const ComposerThreadState();

  const factory ComposerThreadState.idle({
    String draft,
    int submissionRevision,
  }) = IdleComposerThreadState;

  const factory ComposerThreadState.failure({
    required String error,
    String draft,
    int submissionRevision,
  }) = FailedComposerThreadState;

  String get draft => switch (this) {
    IdleComposerThreadState(:final draft) ||
    SubmittingComposerThreadState(:final draft) ||
    FailedComposerThreadState(:final draft) => draft,
    PendingStartComposerThreadState() => '',
  };

  int get submissionRevision => switch (this) {
    IdleComposerThreadState(:final submissionRevision) ||
    SubmittingComposerThreadState(:final submissionRevision) ||
    PendingStartComposerThreadState(:final submissionRevision) ||
    FailedComposerThreadState(:final submissionRevision) => submissionRevision,
  };

  String? get error => switch (this) {
    FailedComposerThreadState(:final error) => error,
    IdleComposerThreadState() ||
    SubmittingComposerThreadState() ||
    PendingStartComposerThreadState() => null,
  };

  bool get isSubmissionPending =>
      this is SubmittingComposerThreadState ||
      this is PendingStartComposerThreadState;

  ComposerThreadState updateDraft(String value) {
    if (isSubmissionPending) return this;
    return IdleComposerThreadState(
      draft: value,
      submissionRevision: submissionRevision,
    );
  }

  ComposerThreadState beginSubmission() {
    if (isSubmissionPending || draft.trim().isEmpty) return this;
    return _startSubmission();
  }

  ComposerThreadState beginCommandSubmission() {
    if (isSubmissionPending) return this;
    return _startSubmission();
  }

  ComposerThreadState _startSubmission() => SubmittingComposerThreadState(
    draft: draft,
    submissionRevision: submissionRevision + 1,
  );

  ComposerThreadState accept(
    SubmitPromptReceipt receipt, {
    required int submissionRevision,
  }) {
    if (!_matchesSubmittingRevision(submissionRevision)) return this;
    return PendingStartComposerThreadState(
      submissionRevision: this.submissionRevision,
      acceptedTurnId: receipt.turnId,
    );
  }

  ComposerThreadState fail(Object error, {required int submissionRevision}) {
    if (!_matchesSubmittingRevision(submissionRevision)) return this;
    return FailedComposerThreadState(
      draft: draft,
      error: error.toString(),
      submissionRevision: this.submissionRevision,
    );
  }

  ComposerThreadState observeTurn(StudioTurnView? turn) {
    final acceptedTurnId = switch (this) {
      PendingStartComposerThreadState(:final acceptedTurnId) => acceptedTurnId,
      IdleComposerThreadState() ||
      SubmittingComposerThreadState() ||
      FailedComposerThreadState() => null,
    };
    if (turn == null || turn.turnId != acceptedTurnId) return this;
    if (turn.state.status == StudioTurnStatus.failed) {
      final message = turn.failure?.message.trim();
      final reason = turn.state.reason?.trim();
      return FailedComposerThreadState(
        draft: draft,
        error: message?.isNotEmpty == true
            ? message!
            : reason?.isNotEmpty == true
            ? reason!
            : 'Turn failed',
        submissionRevision: submissionRevision,
      );
    }
    return IdleComposerThreadState(submissionRevision: submissionRevision);
  }

  bool _matchesSubmittingRevision(int revision) =>
      this is SubmittingComposerThreadState && submissionRevision == revision;
}

final class IdleComposerThreadState extends ComposerThreadState {
  const IdleComposerThreadState({this.draft = '', this.submissionRevision = 0});

  @override
  final String draft;
  @override
  final int submissionRevision;
}

final class SubmittingComposerThreadState extends ComposerThreadState {
  const SubmittingComposerThreadState({
    required this.draft,
    required this.submissionRevision,
  });

  @override
  final String draft;
  @override
  final int submissionRevision;
}

final class PendingStartComposerThreadState extends ComposerThreadState {
  const PendingStartComposerThreadState({
    required this.submissionRevision,
    required this.acceptedTurnId,
  });

  @override
  final int submissionRevision;
  final String acceptedTurnId;
}

final class FailedComposerThreadState extends ComposerThreadState {
  const FailedComposerThreadState({
    required this.error,
    this.draft = '',
    this.submissionRevision = 0,
  });

  @override
  final String error;
  @override
  final String draft;
  @override
  final int submissionRevision;
}
