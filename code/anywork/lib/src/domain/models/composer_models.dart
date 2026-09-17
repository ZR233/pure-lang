import 'attachment_models.dart';
import 'thread_directory_models.dart';

import 'dart:math';

class SubmitPromptReceipt {
  const SubmitPromptReceipt({
    required this.threadId,
    required this.inputId,
    required this.cursor,
  });

  final String threadId;
  final String inputId;
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

// An identity belongs to one immutable submission; unchanged failed drafts reuse it.
String newPromptInputId() {
  final random = Random.secure();
  return 'input-${List.generate(16, (_) => random.nextInt(256).toRadixString(16).padLeft(2, '0')).join()}';
}

sealed class ComposerThreadState {
  const ComposerThreadState();
  const factory ComposerThreadState.idle({
    String draft,
    List<AttachmentDraftView> attachments,
    int submissionRevision,
  }) = IdleComposerThreadState;
  const factory ComposerThreadState.failure({
    required String error,
    String draft,
    List<AttachmentDraftView> attachments,
    int submissionRevision,
    String? inputId,
  }) = FailedComposerThreadState;

  String get draft;
  List<AttachmentDraftView> get attachments;
  int get submissionRevision;
  String? get inputId => switch (this) {
    SubmittingComposerThreadState(:final inputId) => inputId,
    FailedComposerThreadState(:final inputId) => inputId,
    IdleComposerThreadState() => null,
  };
  String? get error => switch (this) {
    FailedComposerThreadState(:final error) => error,
    _ => null,
  };
  bool get isSubmissionPending => this is SubmittingComposerThreadState;

  ComposerThreadState updateDraft(String value) {
    if (isSubmissionPending || value == draft) return this;
    return IdleComposerThreadState(
      draft: value,
      attachments: attachments,
      submissionRevision: submissionRevision,
    );
  }

  ComposerThreadState updateAttachments(List<AttachmentDraftView> value) {
    if (isSubmissionPending) return this;
    return IdleComposerThreadState(
      draft: draft,
      attachments: List.unmodifiable(value),
      submissionRevision: submissionRevision,
    );
  }

  ComposerThreadState reportFailure(Object error) {
    if (isSubmissionPending) return this;
    return FailedComposerThreadState(
      draft: draft,
      attachments: attachments,
      error: error.toString(),
      submissionRevision: submissionRevision,
      inputId: inputId,
    );
  }

  ComposerThreadState beginSubmission() {
    if (isSubmissionPending || (draft.trim().isEmpty && attachments.isEmpty)) {
      return this;
    }
    return _startSubmission();
  }

  ComposerThreadState beginCommandSubmission() =>
      isSubmissionPending ? this : _startSubmission();
  ComposerThreadState _startSubmission() => SubmittingComposerThreadState(
    draft: draft,
    attachments: attachments,
    submissionRevision: submissionRevision + 1,
    inputId: inputId ?? newPromptInputId(),
  );

  ComposerThreadState accept(
    SubmitPromptReceipt receipt, {
    required int submissionRevision,
  }) {
    if (!_matchesSubmittingRevision(submissionRevision)) return this;
    return IdleComposerThreadState(submissionRevision: this.submissionRevision);
  }

  ComposerThreadState fail(Object error, {required int submissionRevision}) {
    if (!_matchesSubmittingRevision(submissionRevision)) return this;
    return FailedComposerThreadState(
      draft: draft,
      attachments: attachments,
      error: error.toString(),
      submissionRevision: this.submissionRevision,
      inputId: inputId,
    );
  }

  bool _matchesSubmittingRevision(int revision) =>
      this is SubmittingComposerThreadState && submissionRevision == revision;
}

final class IdleComposerThreadState extends ComposerThreadState {
  const IdleComposerThreadState({
    this.draft = '',
    this.attachments = const [],
    this.submissionRevision = 0,
  });
  @override
  final String draft;
  @override
  final List<AttachmentDraftView> attachments;
  @override
  final int submissionRevision;
}

final class SubmittingComposerThreadState extends ComposerThreadState {
  const SubmittingComposerThreadState({
    required this.draft,
    required this.attachments,
    required this.submissionRevision,
    required this.inputId,
  });
  @override
  final String draft;
  @override
  final List<AttachmentDraftView> attachments;
  @override
  final int submissionRevision;
  @override
  final String inputId;
}

final class FailedComposerThreadState extends ComposerThreadState {
  const FailedComposerThreadState({
    required this.error,
    this.draft = '',
    this.attachments = const [],
    this.submissionRevision = 0,
    this.inputId,
  });
  @override
  final String error;
  @override
  final String draft;
  @override
  final List<AttachmentDraftView> attachments;
  @override
  final int submissionRevision;
  @override
  final String? inputId;
}
