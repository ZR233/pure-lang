part of 'composer_dock.dart';

class _AttachmentMenu extends StatelessWidget {
  const _AttachmentMenu({
    required this.enabled,
    required this.localCapabilities,
    required this.remoteCapabilities,
    required this.onPickLocal,
    required this.onAddUrl,
  });

  final bool enabled;
  final List<ModelInputCapabilityView> localCapabilities;
  final List<ModelInputCapabilityView> remoteCapabilities;
  final Future<void> Function(List<ModelInputCapabilityView>) onPickLocal;
  final Future<void> Function() onAddUrl;

  @override
  Widget build(BuildContext context) {
    final hasAny =
        localCapabilities.isNotEmpty || remoteCapabilities.isNotEmpty;
    return PopupMenuButton<String>(
      key: StudioDriverKeys.attachmentEntry,
      tooltip: hasAny
          ? context.l10n.composerAttachmentAddTooltip
          : context.l10n.composerAttachmentUnsupportedTooltip,
      enabled: enabled && hasAny,
      icon: const Icon(Icons.attach_file),
      onSelected: (value) {
        if (value == 'local') unawaited(onPickLocal(localCapabilities));
        if (value == 'url') unawaited(onAddUrl());
      },
      itemBuilder: (context) => [
        if (localCapabilities.isNotEmpty)
          PopupMenuItem(
            key: StudioDriverKeys.attachmentLocal,
            value: 'local',
            child: ListTile(
              leading: const Icon(Icons.folder_open_outlined),
              title: Text(context.l10n.composerAttachmentPickLocal),
            ),
          ),
        if (remoteCapabilities.isNotEmpty)
          PopupMenuItem(
            key: StudioDriverKeys.attachmentUrl,
            value: 'url',
            child: ListTile(
              leading: const Icon(Icons.link),
              title: Text(context.l10n.composerAddUrlTitle),
            ),
          ),
      ],
    );
  }
}

class _AttachmentDraftRail extends StatelessWidget {
  const _AttachmentDraftRail({
    required this.attachments,
    required this.enabled,
    required this.onRemove,
  });

  final List<AttachmentDraftView> attachments;
  final bool enabled;
  final ValueChanged<String> onRemove;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      key: StudioDriverKeys.attachmentDraftRail,
      height: 72,
      child: ListView.separated(
        scrollDirection: Axis.horizontal,
        itemCount: attachments.length,
        separatorBuilder: (_, _) => const SizedBox(width: 8),
        itemBuilder: (context, index) {
          final attachment = attachments[index];
          return Container(
            key: StudioDriverKeys.attachmentDraft(attachment.id),
            width: 210,
            padding: const EdgeInsets.all(7),
            decoration: BoxDecoration(
              color: Theme.of(context).colorScheme.surfaceContainerLow,
              borderRadius: BorderRadius.circular(10),
            ),
            child: Row(
              children: [
                SizedBox.square(
                  dimension: 48,
                  child:
                      attachment.modality == AttachmentModalityView.image &&
                          attachment.previewBytes?.isNotEmpty == true
                      ? ClipRRect(
                          borderRadius: BorderRadius.circular(7),
                          child: Image.memory(
                            attachment.previewBytes!,
                            fit: BoxFit.cover,
                          ),
                        )
                      : Icon(_attachmentIcon(attachment.modality)),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      Text(
                        attachment.filename,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                      Text(
                        '${context.attachmentModalityLabel(attachment.modality)} · ${_formatBytes(attachment.byteSize)}',
                        key: StudioDriverKeys.attachmentModality(attachment.id),
                        style: Theme.of(context).textTheme.labelSmall,
                      ),
                    ],
                  ),
                ),
                IconButton(
                  key: StudioDriverKeys.attachmentRemove(attachment.id),
                  visualDensity: VisualDensity.compact,
                  tooltip: context.l10n.composerAttachmentRemoveTooltip,
                  onPressed: enabled ? () => onRemove(attachment.id) : null,
                  icon: const Icon(Icons.close, size: 18),
                ),
              ],
            ),
          );
        },
      ),
    );
  }
}

IconData _attachmentIcon(AttachmentModalityView modality) => switch (modality) {
  AttachmentModalityView.image => Icons.image_outlined,
  AttachmentModalityView.video => Icons.movie_outlined,
  AttachmentModalityView.file => Icons.insert_drive_file_outlined,
};

String _formatBytes(int bytes) {
  if (bytes < 1024) return '$bytes B';
  if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)} KB';
  return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
}
