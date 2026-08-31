part of 'timeline_view.dart';

class _ThreadImageLoader {
  final Map<String, Future<Uint8List>> _images = {};

  Future<Uint8List> load(
    String threadId,
    String attachmentId,
    Future<Uint8List> Function() read,
  ) => _images.putIfAbsent('$threadId\u0000$attachmentId', read);

  void invalidate(String threadId, String attachmentId) {
    _images.remove('$threadId\u0000$attachmentId');
  }

  void clear() => _images.clear();
}

class _ThreadImageCacheScope extends InheritedWidget {
  const _ThreadImageCacheScope({required this.loader, required super.child});

  final _ThreadImageLoader loader;

  static _ThreadImageLoader of(BuildContext context) => context
      .dependOnInheritedWidgetOfExactType<_ThreadImageCacheScope>()!
      .loader;

  @override
  bool updateShouldNotify(_ThreadImageCacheScope oldWidget) =>
      loader != oldWidget.loader;
}

class _ThreadAttachmentCard extends ConsumerStatefulWidget {
  const _ThreadAttachmentCard({
    required this.threadId,
    required this.attachment,
    required this.driverKey,
  });

  final String threadId;
  final ThreadAttachmentView attachment;
  final Key driverKey;

  @override
  ConsumerState<_ThreadAttachmentCard> createState() =>
      _ThreadAttachmentCardState();
}

class _ThreadAttachmentCardState extends ConsumerState<_ThreadAttachmentCard> {
  Future<Uint8List>? _image;

  @override
  void didUpdateWidget(covariant _ThreadAttachmentCard oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.threadId != oldWidget.threadId ||
        widget.attachment.id != oldWidget.attachment.id) {
      _image = null;
    }
  }

  Future<Uint8List> _loadImage() {
    return _ThreadImageCacheScope.of(context).load(
      widget.threadId,
      widget.attachment.id,
      () => ref
          .read(studioApiProvider)
          .readThreadAttachment(widget.threadId, widget.attachment.id),
    );
  }

  @override
  Widget build(BuildContext context) {
    final attachment = widget.attachment;
    if (attachment.modality == AttachmentModalityView.image) {
      _image ??= _loadImage();
    }
    return InkWell(
      key: widget.driverKey,
      borderRadius: BorderRadius.circular(10),
      onTap: attachment.modality == AttachmentModalityView.image
          ? _showImage
          : null,
      child: Container(
        constraints: const BoxConstraints(maxWidth: 230),
        padding: const EdgeInsets.all(7),
        decoration: BoxDecoration(
          color: context.studioPaper,
          border: Border.all(color: context.studioLine),
          borderRadius: BorderRadius.circular(10),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            SizedBox.square(
              dimension: 54,
              child: attachment.modality == AttachmentModalityView.image
                  ? _ThreadImageFuture(
                      attachmentId: attachment.id,
                      image: _image!,
                      fit: BoxFit.cover,
                      onRetry: _retry,
                    )
                  : Icon(_attachmentIcon(attachment.modality)),
            ),
            const SizedBox(width: 8),
            Flexible(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(
                    attachment.filename ?? context.l10n.timelineAttachment,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  Text(
                    '${_attachmentModalityLabel(attachment.modality)} · ${_formatBytes(attachment.byteSize)}',
                    style: Theme.of(context).textTheme.labelSmall,
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  void _retry() {
    _ThreadImageCacheScope.of(context)
        .invalidate(widget.threadId, widget.attachment.id);
    final image = _loadImage();
    setState(() {
      _image = image;
    });
  }

  Future<void> _showImage() async {
    Uint8List? image;
    try {
      image = await _image;
    } on Object {
      return;
    }
    if (!mounted || image == null) return;
    await _showStudioImageDialog(
      context,
      MemoryImage(image),
      dialogKey: StudioDriverKeys.timelineImageDialog(widget.attachment.id),
      label: widget.attachment.filename,
    );
  }
}

class _ThreadImageGallery extends StatelessWidget {
  const _ThreadImageGallery({
    required this.threadId,
    required this.attachments,
    required this.groupId,
  });

  final String threadId;
  final List<ThreadAttachmentView> attachments;
  final String groupId;

  @override
  Widget build(BuildContext context) {
    final images = attachments
        .where(
          (attachment) => attachment.modality == AttachmentModalityView.image,
        )
        .toList(growable: false);
    if (images.isEmpty) return const SizedBox.shrink();
    final multiple = images.length > 1;
    return Wrap(
      key: StudioDriverKeys.toolImageGallery(groupId),
      spacing: 8,
      runSpacing: 8,
      children: [
        for (final attachment in images)
          _ThreadImageThumbnail(
            key: ValueKey('tool-image:${attachment.id}'),
            threadId: threadId,
            attachment: attachment,
            compact: multiple,
          ),
      ],
    );
  }
}

class _ThreadImageThumbnail extends ConsumerStatefulWidget {
  const _ThreadImageThumbnail({
    required this.threadId,
    required this.attachment,
    required this.compact,
    super.key,
  });

  final String threadId;
  final ThreadAttachmentView attachment;
  final bool compact;

  @override
  ConsumerState<_ThreadImageThumbnail> createState() =>
      _ThreadImageThumbnailState();
}

class _ThreadImageThumbnailState extends ConsumerState<_ThreadImageThumbnail> {
  Future<Uint8List>? _image;

  @override
  void didUpdateWidget(covariant _ThreadImageThumbnail oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.threadId != oldWidget.threadId ||
        widget.attachment.id != oldWidget.attachment.id) {
      _image = null;
    }
  }

  Future<Uint8List> _loadImage() => _ThreadImageCacheScope.of(context).load(
    widget.threadId,
    widget.attachment.id,
    () => ref
        .read(studioApiProvider)
        .readThreadAttachment(widget.threadId, widget.attachment.id),
  );

  @override
  Widget build(BuildContext context) {
    _image ??= _loadImage();
    final size = _toolImagePreviewSize(widget.attachment, widget.compact);
    return Tooltip(
      message: _attachmentDescription(context, widget.attachment),
      child: InkWell(
        key: StudioDriverKeys.viewImageThumbnail(widget.attachment.id),
        borderRadius: BorderRadius.circular(10),
        onTap: _showImage,
        child: Container(
          width: size.width,
          height: size.height,
          clipBehavior: Clip.antiAlias,
          decoration: BoxDecoration(
            color: context.studioPaper,
            border: Border.all(color: context.studioLine),
            borderRadius: BorderRadius.circular(10),
          ),
          child: _ThreadImageFuture(
            attachmentId: widget.attachment.id,
            image: _image!,
            fit: widget.compact ? BoxFit.cover : BoxFit.scaleDown,
            onRetry: _retry,
          ),
        ),
      ),
    );
  }

  void _retry() {
    _ThreadImageCacheScope.of(context)
        .invalidate(widget.threadId, widget.attachment.id);
    final image = _loadImage();
    setState(() {
      _image = image;
    });
  }

  Future<void> _showImage() async {
    Uint8List? image;
    try {
      image = await _image;
    } on Object {
      return;
    }
    if (!mounted || image == null) return;
    await _showStudioImageDialog(
      context,
      MemoryImage(image),
      dialogKey: StudioDriverKeys.viewImageDialog(widget.attachment.id),
      label: widget.attachment.filename,
    );
  }
}

class _ThreadImageFuture extends StatelessWidget {
  const _ThreadImageFuture({
    required this.attachmentId,
    required this.image,
    required this.fit,
    required this.onRetry,
  });

  final String attachmentId;
  final Future<Uint8List> image;
  final BoxFit fit;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    return FutureBuilder<Uint8List>(
      future: image,
      builder: (context, snapshot) {
        if (snapshot.hasError) {
          return Tooltip(
            message: context.l10n.timelineImageLoadFailed,
            child: IconButton(
              key: StudioDriverKeys.timelineImageRetry(attachmentId),
              onPressed: onRetry,
              icon: Icon(
                Icons.refresh,
                key: ValueKey('attachment-load-failed-$attachmentId'),
              ),
            ),
          );
        }
        if (!snapshot.hasData) {
          return const Center(
            child: SizedBox.square(
              dimension: 18,
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
          );
        }
        return Image.memory(snapshot.data!, fit: fit);
      },
    );
  }
}

Size _toolImagePreviewSize(ThreadAttachmentView attachment, bool compact) {
  if (compact) return const Size.square(64);
  final width = attachment.width?.toDouble() ?? 240;
  final height = attachment.height?.toDouble() ?? 180;
  final ratio = (width / math.max(height, 1)).clamp(0.25, 4.0);
  if (ratio >= 1) {
    return Size(240, math.max(64, 240 / ratio));
  }
  return Size(math.max(64, 240 * ratio), 240);
}

String _attachmentDescription(
  BuildContext context,
  ThreadAttachmentView attachment,
) {
  final dimensions = attachment.width != null && attachment.height != null
      ? ' · ${attachment.width}×${attachment.height}'
      : '';
  return '${attachment.filename ?? context.l10n.timelineImageFallback}$dimensions · ${_formatBytes(attachment.byteSize)}';
}

Future<void> _showStudioImageDialog(
  BuildContext context,
  ImageProvider image, {
  required Key dialogKey,
  String? label,
}) async {
  final previousFocus = FocusManager.instance.primaryFocus;
  await showDialog<void>(
    context: context,
    barrierDismissible: true,
    builder: (context) => Dialog(
      key: dialogKey,
      clipBehavior: Clip.antiAlias,
      child: SizedBox(
        width: math.min(
          1000,
          math.max(240, MediaQuery.sizeOf(context).width - 80),
        ),
        height: math.min(
          760,
          math.max(200, MediaQuery.sizeOf(context).height - 80),
        ),
        child: Stack(
          children: [
            Positioned.fill(
              child: InteractiveViewer(
                minScale: 0.5,
                maxScale: 5,
                child: Center(
                  child: Image(
                    image: image,
                    errorBuilder: (context, error, stackTrace) => Center(
                      child: Text(context.l10n.timelineImageLoadFailed),
                    ),
                  ),
                ),
              ),
            ),
            Positioned(
              top: 8,
              right: 8,
              child: IconButton.filledTonal(
                key: StudioDriverKeys.timelineImageClose,
                tooltip: context.l10n.timelineImageClose,
                onPressed: () => Navigator.of(context).pop(),
                icon: const Icon(Icons.close),
              ),
            ),
            if (label != null && label.isNotEmpty)
              Positioned(
                left: 12,
                bottom: 10,
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    color: Theme.of(context).colorScheme.surface
                        .withValues(alpha: 0.86),
                    borderRadius: BorderRadius.circular(6),
                  ),
                  child: Padding(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 8,
                      vertical: 4,
                    ),
                    child: Text(label),
                  ),
                ),
              ),
          ],
        ),
      ),
    ),
  );
  previousFocus?.requestFocus();
}
