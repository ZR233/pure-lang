import 'attachment_models.dart';
import 'thread_directory_models.dart';

import 'dart:math';
import 'dart:typed_data';

import 'package:flutter/foundation.dart' show listEquals;

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

/// A target-scoped diagnostic that remains observable even after the target
/// composer or workspace has been removed from canonical state.
class ComposerTargetDiagnostic {
  ComposerTargetDiagnostic({
    required this.projectId,
    required this.threadId,
    required this.message,
    List<String> cleanupFailureDraftIds = const [],
  }) : cleanupFailureDraftIds = List.unmodifiable(cleanupFailureDraftIds);

  final String projectId;
  final String? threadId;
  final String message;
  final List<String> cleanupFailureDraftIds;

  String get targetKey => attachmentTargetKey(projectId, threadId);

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is ComposerTargetDiagnostic &&
            projectId == other.projectId &&
            threadId == other.threadId &&
            message == other.message &&
            listEquals(cleanupFailureDraftIds, other.cleanupFailureDraftIds);
  }

  @override
  int get hashCode => Object.hash(
    projectId,
    threadId,
    message,
    Object.hashAll(cleanupFailureDraftIds),
  );
}

String attachmentTargetKey(String projectId, String? threadId) =>
    '$projectId\u0000${threadId ?? '<new>'}';

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
    int attachmentGeneration,
    List<String> cleanupFailureDraftIds,
  }) = IdleComposerThreadState;
  const factory ComposerThreadState.failure({
    required String error,
    String draft,
    List<AttachmentDraftView> attachments,
    int submissionRevision,
    int attachmentGeneration,
    String? inputId,
    List<String> cleanupFailureDraftIds,
  }) = FailedComposerThreadState;

  String get draft;
  List<AttachmentDraftView> get attachments;
  int get submissionRevision;
  int get attachmentGeneration;
  List<String> get cleanupFailureDraftIds;
  String? get inputId => switch (this) {
    SubmittingComposerThreadState(:final inputId) => inputId,
    FailedComposerThreadState(:final inputId) => inputId,
    IdleComposerThreadState() => null,
  };
  String? get error => switch (this) {
    FailedComposerThreadState(:final error, :final cleanupFailureDraftIds) =>
      _composerError(error, cleanupFailureDraftIds),
    IdleComposerThreadState(:final cleanupFailureDraftIds) =>
      _cleanupFailureMessage(cleanupFailureDraftIds),
    SubmittingComposerThreadState(:final cleanupFailureDraftIds) =>
      _cleanupFailureMessage(cleanupFailureDraftIds),
  };
  bool get isSubmissionPending => this is SubmittingComposerThreadState;

  ComposerThreadState updateDraft(String value) {
    if (isSubmissionPending || value == draft) return this;
    return IdleComposerThreadState(
      draft: value,
      attachments: attachments,
      submissionRevision: submissionRevision,
      attachmentGeneration: attachmentGeneration,
      cleanupFailureDraftIds: cleanupFailureDraftIds,
    );
  }

  ComposerThreadState updateAttachments(List<AttachmentDraftView> value) {
    if (isSubmissionPending) return this;
    return IdleComposerThreadState(
      draft: draft,
      attachments: List.unmodifiable(value),
      submissionRevision: submissionRevision,
      attachmentGeneration: attachmentGeneration + 1,
      cleanupFailureDraftIds: cleanupFailureDraftIds,
    );
  }

  /// Remove one durable draft from the current canonical composer. The
  /// collection generation advances so previews that captured the old
  /// collection must revalidate before writing; the controller reschedules
  /// still-present drafts against that newer generation.
  ComposerThreadState removeAttachment(String draftId) {
    if (!attachments.any((attachment) => attachment.id == draftId)) {
      return this;
    }
    final next = <AttachmentDraftView>[
      for (final attachment in attachments)
        if (attachment.id != draftId) attachment,
    ];
    if (this case SubmittingComposerThreadState()) {
      return SubmittingComposerThreadState(
        draft: draft,
        attachments: List.unmodifiable(next),
        submissionRevision: submissionRevision,
        attachmentGeneration: attachmentGeneration + 1,
        inputId: inputId!,
        cleanupFailureDraftIds: cleanupFailureDraftIds,
      );
    }
    return updateAttachments(next);
  }

  /// Replace one admitted draft's preview without starting another attachment
  /// generation. All previews from one admission batch can therefore finish
  /// independently while retaining the same race guard.
  ComposerThreadState updateAttachmentPreview(
    String draftId,
    Uint8List previewBytes,
  ) {
    if (isSubmissionPending) return this;
    final index = attachments.indexWhere(
      (attachment) => attachment.id == draftId,
    );
    if (index < 0 ||
        listEquals(attachments[index].previewBytes, previewBytes)) {
      return this;
    }
    final next = <AttachmentDraftView>[...attachments];
    next[index] = next[index].copyWith(previewBytes: previewBytes);
    return _replaceAttachments(next);
  }

  ComposerThreadState _replaceAttachments(List<AttachmentDraftView> value) {
    final List<AttachmentDraftView> next = List.unmodifiable(value);
    return switch (this) {
      IdleComposerThreadState() => IdleComposerThreadState(
        draft: draft,
        attachments: next,
        submissionRevision: submissionRevision,
        attachmentGeneration: attachmentGeneration,
        cleanupFailureDraftIds: cleanupFailureDraftIds,
      ),
      FailedComposerThreadState(:final error) => FailedComposerThreadState(
        error: error,
        draft: draft,
        attachments: next,
        submissionRevision: submissionRevision,
        attachmentGeneration: attachmentGeneration,
        inputId: inputId,
        cleanupFailureDraftIds: cleanupFailureDraftIds,
      ),
      SubmittingComposerThreadState() => this,
    };
  }

  /// Preserve an already admitted batch when a preview fails.
  ComposerThreadState reportAttachmentFailure(Object error) {
    if (isSubmissionPending) return this;
    return FailedComposerThreadState(
      draft: draft,
      attachments: attachments,
      error: error.toString(),
      submissionRevision: submissionRevision,
      attachmentGeneration: attachmentGeneration,
      inputId: inputId,
      cleanupFailureDraftIds: cleanupFailureDraftIds,
    );
  }

  /// Preserve cleanup obligations even while a submission is in flight. A
  /// normal composer failure cannot represent an error on a submitting state,
  /// so the IDs are carried as a dedicated typed diagnostic.
  ComposerThreadState reportAttachmentCleanupFailure(
    Iterable<String> draftIds,
  ) {
    final ids = {...cleanupFailureDraftIds, ...draftIds}.toList()..sort();
    if (ids.isEmpty) return this;
    final message = _cleanupFailureMessage(ids)!;
    return switch (this) {
      IdleComposerThreadState() => IdleComposerThreadState(
        draft: draft,
        attachments: attachments,
        submissionRevision: submissionRevision,
        attachmentGeneration: attachmentGeneration,
        cleanupFailureDraftIds: ids,
      ),
      SubmittingComposerThreadState() => SubmittingComposerThreadState(
        draft: draft,
        attachments: attachments,
        submissionRevision: submissionRevision,
        attachmentGeneration: attachmentGeneration,
        inputId: inputId!,
        cleanupFailureDraftIds: ids,
      ),
      FailedComposerThreadState() => FailedComposerThreadState(
        error: error ?? message,
        draft: draft,
        attachments: attachments,
        submissionRevision: submissionRevision,
        attachmentGeneration: attachmentGeneration,
        inputId: inputId,
        cleanupFailureDraftIds: ids,
      ),
    };
  }

  @override
  bool operator ==(Object other) {
    return identical(this, other) ||
        other is ComposerThreadState &&
            runtimeType == other.runtimeType &&
            draft == other.draft &&
            listEquals(attachments, other.attachments) &&
            submissionRevision == other.submissionRevision &&
            attachmentGeneration == other.attachmentGeneration &&
            inputId == other.inputId &&
            error == other.error &&
            listEquals(cleanupFailureDraftIds, other.cleanupFailureDraftIds);
  }

  @override
  int get hashCode => Object.hash(
    runtimeType,
    draft,
    Object.hashAll(attachments),
    submissionRevision,
    attachmentGeneration,
    inputId,
    error,
    Object.hashAll(cleanupFailureDraftIds),
  );

  ComposerThreadState reportFailure(Object error) {
    if (isSubmissionPending) return this;
    return FailedComposerThreadState(
      draft: draft,
      attachments: attachments,
      error: error.toString(),
      submissionRevision: submissionRevision,
      attachmentGeneration: attachmentGeneration,
      inputId: inputId,
      cleanupFailureDraftIds: cleanupFailureDraftIds,
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
    attachmentGeneration: attachmentGeneration,
    inputId: inputId ?? newPromptInputId(),
    cleanupFailureDraftIds: cleanupFailureDraftIds,
  );

  ComposerThreadState accept(
    SubmitPromptReceipt receipt, {
    required int submissionRevision,
    required String inputId,
  }) {
    if (!_matchesSubmittingRevision(submissionRevision) ||
        receipt.inputId != inputId) {
      return this;
    }
    return IdleComposerThreadState(
      submissionRevision: this.submissionRevision,
      attachmentGeneration: attachmentGeneration,
      cleanupFailureDraftIds: cleanupFailureDraftIds,
    );
  }

  ComposerThreadState fail(Object error, {required int submissionRevision}) {
    if (!_matchesSubmittingRevision(submissionRevision)) return this;
    return FailedComposerThreadState(
      draft: draft,
      attachments: attachments,
      error: error.toString(),
      submissionRevision: this.submissionRevision,
      attachmentGeneration: attachmentGeneration,
      inputId: inputId,
      cleanupFailureDraftIds: cleanupFailureDraftIds,
    );
  }

  bool _matchesSubmittingRevision(int revision) =>
      this is SubmittingComposerThreadState && submissionRevision == revision;
}

String? _cleanupFailureMessage(List<String> draftIds) {
  if (draftIds.isEmpty) return null;
  return 'Attachment cleanup failed for draft IDs: ${draftIds.join(', ')}';
}

String? _composerError(String? error, List<String> cleanupFailureDraftIds) {
  final cleanup = _cleanupFailureMessage(cleanupFailureDraftIds);
  if (cleanup == null || error == cleanup) return error ?? cleanup;
  if (error == null || error.isEmpty) return cleanup;
  return '$cleanup\n$error';
}

final class IdleComposerThreadState extends ComposerThreadState {
  const IdleComposerThreadState({
    this.draft = '',
    this.attachments = const [],
    this.submissionRevision = 0,
    this.attachmentGeneration = 0,
    this.cleanupFailureDraftIds = const [],
  });
  @override
  final String draft;
  @override
  final List<AttachmentDraftView> attachments;
  @override
  final int submissionRevision;
  @override
  final int attachmentGeneration;
  @override
  final List<String> cleanupFailureDraftIds;
}

final class SubmittingComposerThreadState extends ComposerThreadState {
  const SubmittingComposerThreadState({
    required this.draft,
    required this.attachments,
    required this.submissionRevision,
    required this.attachmentGeneration,
    required this.inputId,
    this.cleanupFailureDraftIds = const [],
  });
  @override
  final String draft;
  @override
  final List<AttachmentDraftView> attachments;
  @override
  final int submissionRevision;
  @override
  final int attachmentGeneration;
  @override
  final String inputId;
  @override
  final List<String> cleanupFailureDraftIds;
}

final class FailedComposerThreadState extends ComposerThreadState {
  const FailedComposerThreadState({
    required this.error,
    this.draft = '',
    this.attachments = const [],
    this.submissionRevision = 0,
    this.attachmentGeneration = 0,
    this.inputId,
    this.cleanupFailureDraftIds = const [],
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
  final int attachmentGeneration;
  @override
  final String? inputId;
  @override
  final List<String> cleanupFailureDraftIds;
}
