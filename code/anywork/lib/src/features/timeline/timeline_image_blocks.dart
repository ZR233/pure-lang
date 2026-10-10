part of 'timeline_view.dart';

class _ThreadImageLoader {
  final Map<String, Future<Uint8List>> _images = {};
  final Set<String> expandedCalls = {};

  void retainWindow(String? threadId, List<TimelineRow> rows) {
    final attachments = <String>{};
    final entries = <String>{};
    for (final row in rows) {
      attachments.addAll(
        row.part?.attachments.map((item) => item.id) ?? const <String>[],
      );
      final group = row.toolGroup;
      if (group == null) continue;
      for (final entry in _toolImageEntries(group.items)) {
        attachments.add(entry.attachment.id);
        entries.add(_toolImageCallId(entry));
      }
    }
    expandedCalls.retainAll(entries);
    _images.removeWhere(
      (key, _) => !attachments.any((id) => key == '$threadId\u0000$id'),
    );
  }

  Future<Uint8List> load(
    String threadId,
    String attachmentId,
    Future<Uint8List> Function() read,
  ) => _images.putIfAbsent('$threadId\u0000$attachmentId', read);

  void invalidate(String threadId, String attachmentId) {
    _images.remove('$threadId\u0000$attachmentId');
  }

  void clear() {
    _images.clear();
    expandedCalls.clear();
  }
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
          .read(studioControllerProvider.notifier)
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
          color: context.colors.surface,
          border: Border.all(color: context.colors.outlineVariant),
          borderRadius: BorderRadius.circular(10),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            SizedBox.square(
              dimension: 54,
              child: attachment.modality == AttachmentModalityView.image
                  ? _ThreadImageFuture(
                      presentationId: attachment.id,
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
                    '${context.attachmentModalityLabel(attachment.modality)} · ${_formatBytes(attachment.byteSize)}',
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

/// 一个工具图片条目的稳定身份。
///
/// 资源 id 内容寻址：同一工具组内不同调用读取同一图片会得到相同的附件 id，因此
/// 条目身份必须叠加 owning 调用身份，不能仅用附件 id。`entryId` 由 owning 调用的
/// `callId`（缺失时回退该调用的 `toolCallId`）与附件 id 组合而成，并在同一调用结果
/// 内出现相同资源时追加稳定序号；该序号只取决于同一调用自身的附件列表，不随其它
/// 调用流式新增而变化。
typedef _ToolImageEntryRef = ({
  TimelineToolGroupItem item,
  ThreadAttachmentView attachment,
  String entryId,
});

/// 按 owning 调用身份 + 附件 id 构造工具组内每个图片条目的唯一身份。
///
/// 不按资源去重、不丢弃调用：同一调用重复引用同一资源时追加 `#<序号>`；跨调用同一
/// 资源仍各自保留独立条目与展开状态。字节读取仍由 Thread 附件缓存按
/// `threadId + attachmentId` 去重。
List<_ToolImageEntryRef> _toolImageEntries(List<TimelineToolGroupItem> items) {
  final entries = <_ToolImageEntryRef>[];
  final occurrences = <String, int>{};
  for (final item in items) {
    final tool = item.tool;
    if (tool == null) continue;
    final callKey = tool.callId ?? tool.toolCallId;
    for (final attachment in tool.attachments) {
      if (attachment.modality != AttachmentModalityView.image) continue;
      final base = '$callKey:${attachment.id}';
      final index = occurrences[base] ?? 0;
      occurrences[base] = index + 1;
      entries.add((
        item: item,
        attachment: attachment,
        entryId: index == 0 ? base : '$base#$index',
      ));
    }
  }
  return entries;
}

String _toolImageCallId(_ToolImageEntryRef entry) =>
    entry.item.tool!.callId ?? entry.item.tool!.toolCallId;

/// A call owns expansion; each attachment keeps its own stable presentation ID.
class _ThreadImageGallery extends StatelessWidget {
  const _ThreadImageGallery({
    required this.threadId,
    required this.entries,
    required this.groupId,
  });
  final String threadId;
  final List<_ToolImageEntryRef> entries;
  final String groupId;

  @override
  Widget build(BuildContext context) {
    final calls = <String, List<_ToolImageEntryRef>>{};
    for (final entry in entries) {
      calls.putIfAbsent(_toolImageCallId(entry), () => []).add(entry);
    }
    return Column(
      key: StudioDriverKeys.toolImageGallery(groupId),
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (final call in calls.entries)
          _ThreadToolImageCall(
            key: ValueKey('tool-image-call:${call.key}'),
            threadId: threadId,
            callId: call.key,
            entries: call.value,
          ),
      ],
    );
  }
}

class _ThreadToolImageCall extends StatefulWidget {
  const _ThreadToolImageCall({
    required this.threadId,
    required this.callId,
    required this.entries,
    super.key,
  });
  final String threadId;
  final String callId;
  final List<_ToolImageEntryRef> entries;
  @override
  State<_ThreadToolImageCall> createState() => _ThreadToolImageCallState();
}

class _ThreadToolImageCallState extends State<_ThreadToolImageCall> {
  @override
  Widget build(BuildContext context) {
    final loader = _ThreadImageCacheScope.of(context);
    final expanded = loader.expandedCalls.contains(widget.callId);
    final first = widget.entries.first;
    final item = first.item;
    final count = widget.entries.length;
    final label = item.name == 'view_image'
        ? item.part.status == 'succeeded'
              ? context.l10n.timelineImagesViewed(count)
              : _toolTitle(context, item)
        : context.l10n.timelineImagesProduced(count);
    void toggle() {
      _TimelineItemLayoutScope.report(
        context,
        expanded
            ? _TimelineLayoutChangeKind.collapse
            : _TimelineLayoutChangeKind.expand,
      );
      setState(() {
        if (!loader.expandedCalls.remove(widget.callId)) {
          loader.expandedCalls.add(widget.callId);
        }
      });
    }

    return Padding(
      padding: const EdgeInsets.only(top: 3),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Tooltip(
            message:
                _toolTarget(item) ??
                _attachmentDescription(context, first.attachment),
            child: Semantics(
              button: true,
              expanded: expanded,
              child: InkWell(
                key: StudioDriverKeys.viewImageToggle(first.entryId),
                borderRadius: BorderRadius.circular(StudioRadii.xs),
                onTap: toggle,
                child: Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 4,
                    vertical: 5,
                  ),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(
                        Icons.collections_outlined,
                        size: 17,
                        color: context.colors.onSurfaceVariant,
                      ),
                      const SizedBox(width: 8),
                      Flexible(
                        child: Text(
                          label,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: context.text.bodySmall?.copyWith(
                            color: expanded
                                ? context.colors.onSurface
                                : context.colors.onSurfaceVariant,
                          ),
                        ),
                      ),
                      const SizedBox(width: 4),
                      Icon(
                        expanded
                            ? Icons.keyboard_arrow_down_rounded
                            : Icons.chevron_right_rounded,
                        size: 17,
                        color: context.colors.onSurfaceVariant,
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ),
          if (expanded)
            Padding(
              padding: const EdgeInsets.only(left: 4, top: 6, bottom: 4),
              child: Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  for (final entry in widget.entries)
                    _ThreadToolImageThumbnail(
                      key: ValueKey('tool-image:${entry.entryId}'),
                      threadId: widget.threadId,
                      entry: entry,
                    ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}

class _ThreadToolImageThumbnail extends ConsumerStatefulWidget {
  const _ThreadToolImageThumbnail({
    required this.threadId,
    required this.entry,
    super.key,
  });
  final String threadId;
  final _ToolImageEntryRef entry;
  @override
  ConsumerState<_ThreadToolImageThumbnail> createState() =>
      _ThreadToolImageThumbnailState();
}

class _ThreadToolImageThumbnailState
    extends ConsumerState<_ThreadToolImageThumbnail> {
  Future<Uint8List>? _image;
  @override
  void didUpdateWidget(covariant _ThreadToolImageThumbnail oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.threadId != oldWidget.threadId ||
        widget.entry.attachment.id != oldWidget.entry.attachment.id) {
      _image = null;
    }
  }

  Future<Uint8List> _loadImage() => _ThreadImageCacheScope.of(context).load(
    widget.threadId,
    widget.entry.attachment.id,
    () => ref
        .read(studioControllerProvider.notifier)
        .readThreadAttachment(widget.threadId, widget.entry.attachment.id),
  );
  @override
  Widget build(BuildContext context) {
    // Subscribe in the same frame, including when a cached read already failed.
    _image ??= _loadImage();
    return Tooltip(
      message: _attachmentDescription(context, widget.entry.attachment),
      child: Container(
        width: 120,
        height: 120,
        clipBehavior: Clip.antiAlias,
        decoration: BoxDecoration(
          color: context.colors.surface,
          border: Border.all(color: context.colors.outlineVariant),
          borderRadius: BorderRadius.circular(10),
        ),
        child: _ThreadImageFuture(
          presentationId: widget.entry.entryId,
          image: _image!,
          fit: BoxFit.contain,
          onRetry: _retry,
          loadedKey: StudioDriverKeys.viewImageThumbnail(widget.entry.entryId),
          onTap: _showImage,
        ),
      ),
    );
  }

  void _retry() {
    _ThreadImageCacheScope.of(context)
        .invalidate(widget.threadId, widget.entry.attachment.id);
    setState(() => _image = _loadImage());
  }

  Future<void> _showImage() async {
    Uint8List image;
    try {
      image = await _image!;
    } on Object {
      return;
    }
    if (!mounted) return;
    await _showStudioImageDialog(
      context,
      MemoryImage(image),
      dialogKey: StudioDriverKeys.viewImageDialog(widget.entry.entryId),
      label: widget.entry.attachment.filename,
    );
  }
}

class _ThreadImageFuture extends StatelessWidget {
  const _ThreadImageFuture({
    required this.presentationId,
    required this.image,
    required this.fit,
    required this.onRetry,
    this.loadedKey,
    this.onTap,
  });

  /// 失败与重试 key 使用的展示身份。
  ///
  /// 资源 id 内容寻址：不同调用读取同一图片会得到相同附件 id，若用附件 id 生成 key，
  /// 两个条目同时失败会挂载重复的全树 ValueKey，Driver 无法唯一定位。工具图片条目
  /// 传入 owning 调用身份叠加附件 id 的 `entryId`；用户上传附件仍传附件 id，key 不变。
  /// 字节读取与缓存不经过此字段，始终按真实 `threadId + attachmentId` 去重。
  final String presentationId;
  final Future<Uint8List> image;
  final BoxFit fit;
  final VoidCallback onRetry;

  /// 仅在字节成功加载后挂载的 key，用于让 Driver 以 key 存在证明图片已读取。
  final Key? loadedKey;

  /// 成功加载后的放大回调；与 [loadedKey] 同时给出。
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return FutureBuilder<Uint8List>(
      future: image,
      builder: (context, snapshot) {
        if (snapshot.hasError) {
          return Tooltip(
            message: context.l10n.timelineImageLoadFailed,
            child: IconButton(
              key: StudioDriverKeys.timelineImageRetry(presentationId),
              onPressed: onRetry,
              icon: Icon(
                Icons.refresh,
                key: ValueKey('attachment-load-failed-$presentationId'),
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
        final loaded = Image.memory(snapshot.data!, fit: fit);
        if (loadedKey == null && onTap == null) {
          return loaded;
        }
        return InkWell(
          key: loadedKey,
          onTap: onTap,
          child: SizedBox.expand(child: loaded),
        );
      },
    );
  }
}

String _attachmentDescription(
  BuildContext context,
  ThreadAttachmentView attachment,
) {
  final dimensions = attachment.width != null && attachment.height != null
      ? ' · ${attachment.width}×${attachment.height}'
      : '';
  final label = attachment.filename ?? context.l10n.attachmentFallback;
  return '$label$dimensions · ${_formatBytes(attachment.byteSize)}';
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
