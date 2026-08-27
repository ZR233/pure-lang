import 'dart:typed_data';

import 'studio_enums.dart';

enum AttachmentModalityView { image, video, file }

sealed class AttachmentAdmissionContext {
  const AttachmentAdmissionContext();

  const factory AttachmentAdmissionContext.existingThread(String threadId) =
      ExistingThreadAttachmentAdmissionContext;
  const factory AttachmentAdmissionContext.newThread(StudioMode mode) =
      NewThreadAttachmentAdmissionContext;
}

final class ExistingThreadAttachmentAdmissionContext
    extends AttachmentAdmissionContext {
  const ExistingThreadAttachmentAdmissionContext(this.threadId);
  final String threadId;
}

final class NewThreadAttachmentAdmissionContext
    extends AttachmentAdmissionContext {
  const NewThreadAttachmentAdmissionContext(this.mode);
  final StudioMode mode;
}

sealed class AttachmentDraftSource {
  const AttachmentDraftSource();

  const factory AttachmentDraftSource.localFile(String path) =
      LocalFileAttachmentDraftSource;
  const factory AttachmentDraftSource.remoteUrl(
    String url, {
    String? filename,
  }) = RemoteUrlAttachmentDraftSource;
}

final class LocalFileAttachmentDraftSource extends AttachmentDraftSource {
  const LocalFileAttachmentDraftSource(this.path);
  final String path;
}

final class RemoteUrlAttachmentDraftSource extends AttachmentDraftSource {
  const RemoteUrlAttachmentDraftSource(this.url, {this.filename});
  final String url;
  final String? filename;
}

class AttachmentDraftView {
  const AttachmentDraftView({
    required this.id,
    required this.modality,
    required this.mediaType,
    required this.filename,
    required this.byteSize,
    this.width,
    this.height,
    this.previewBytes,
  });

  final String id;
  final AttachmentModalityView modality;
  final String mediaType;
  final String filename;
  final int byteSize;
  final int? width;
  final int? height;
  final Uint8List? previewBytes;

  AttachmentDraftView copyWith({Uint8List? previewBytes}) =>
      AttachmentDraftView(
        id: id,
        modality: modality,
        mediaType: mediaType,
        filename: filename,
        byteSize: byteSize,
        width: width,
        height: height,
        previewBytes: previewBytes ?? this.previewBytes,
      );
}

class StudioPromptInput {
  const StudioPromptInput({
    required this.text,
    required this.attachmentDraftIds,
  });

  final String text;
  final List<String> attachmentDraftIds;
}
