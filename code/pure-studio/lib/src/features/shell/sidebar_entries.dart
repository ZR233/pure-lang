part of 'studio_shell.dart';

class _ProjectTile extends ConsumerWidget {
  const _ProjectTile({
    required this.project,
    required this.compact,
    required this.selected,
    required this.recoveryIssue,
  });

  final StudioProject project;
  final bool compact;
  final bool selected;
  final StudioRecoveryIssue? recoveryIssue;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final colors = Theme.of(context).colorScheme;
    final controller = ref.read(studioControllerProvider.notifier);
    final issue = recoveryIssue;
    if (compact) {
      return KeyedSubtree(
        key: StudioDriverKeys.projectRow(project.id),
        child: _CompactSidebarTile(
          selected: selected,
          tooltip: issue?.detail ?? project.name,
          icon: issue != null
              ? Icons.error_outline
              : selected
              ? Icons.folder
              : Icons.folder_open,
          iconColor: issue == null ? null : colors.error,
          onTap: issue == null
              ? () => controller.selectProject(project.id)
              : null,
          actionTooltip: context.l10n.sidebarCloseProject,
          actionIcon: Icons.close,
          onAction: issue == null
              ? () => unawaited(controller.archiveProject(project.id))
              : null,
        ),
      );
    }
    final tile = _SidebarTile(
      selected: selected,
      icon: issue != null
          ? Icons.error_outline
          : selected
          ? Icons.folder
          : Icons.folder_open,
      title: project.name,
      showTitleTooltip: issue == null,
      subtitle: project.path,
      dense: true,
      iconColor: issue != null
          ? colors.error
          : selected
          ? context.colors.onPrimaryContainer
          : colors.onSurfaceVariant,
      onTap: issue == null ? () => controller.selectProject(project.id) : null,
      trailing: IconButton(
        key: ValueKey('project-close-${project.id}'),
        tooltip: context.l10n.sidebarCloseProject,
        style: IconButton.styleFrom(
          minimumSize: const Size.square(30),
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        ),
        icon: const Icon(Icons.close, size: 17),
        onPressed: issue == null
            ? () => unawaited(controller.archiveProject(project.id))
            : null,
      ),
    );
    return KeyedSubtree(
      key: StudioDriverKeys.projectRow(project.id),
      child: issue == null ? tile : Tooltip(message: issue.detail, child: tile),
    );
  }
}

class _ThreadTile extends ConsumerWidget {
  const _ThreadTile({
    required this.thread,
    required this.modeDisplayName,
    required this.selected,
    required this.compact,
    required this.recoveryIssue,
    required this.canArchive,
  });

  final StudioThread thread;
  final String? modeDisplayName;
  final bool selected;
  final bool compact;
  final StudioRecoveryIssue? recoveryIssue;
  final bool canArchive;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final modeIcon = thread.mode == ThreadModeId.simple
        ? Icons.flash_on
        : Icons.route;
    final colors = Theme.of(context).colorScheme;
    final issue = recoveryIssue;
    if (compact) {
      return KeyedSubtree(
        key: StudioDriverKeys.threadRow(thread.id),
        child: _CompactSidebarTile(
          selected: selected,
          tooltip: issue?.detail ?? thread.title,
          icon: issue == null ? modeIcon : Icons.error_outline,
          iconColor: issue == null ? null : colors.error,
          onTap: issue == null
              ? () => ref
                    .read(studioControllerProvider.notifier)
                    .selectThread(thread.id)
              : null,
          actionKey: StudioDriverKeys.archiveThread(thread.id),
          actionTooltip: context.l10n.sidebarArchiveSession,
          actionIcon: Icons.archive_outlined,
          onAction: issue == null && canArchive
              ? () => unawaited(
                  _archiveThreadFromSidebar(context, ref, thread.id),
                )
              : null,
          secondaryActionKey: StudioDriverKeys.renameThread(thread.id),
          secondaryActionTooltip: context.l10n.sidebarRenameSession,
          secondaryActionIcon: Icons.edit_outlined,
          onSecondaryAction: issue == null
              ? () => unawaited(_renameThreadFromSidebar(context, ref, thread))
              : null,
        ),
      );
    }
    final tile = _SidebarTile(
      selected: selected,
      icon: issue == null ? modeIcon : Icons.error_outline,
      title: thread.title,
      showTitleTooltip: issue == null,
      subtitle: _threadSubtitle(context, thread, modeDisplayName),
      dense: true,
      iconColor: issue != null
          ? colors.error
          : selected
          ? context.colors.onPrimaryContainer
          : colors.onSurfaceVariant,
      markerColor: issue != null
          ? null
          : thread.mode == ThreadModeId.simple
          ? context.colors.primary
          : context.colors.onSurfaceVariant,
      onTap: issue == null
          ? () => ref
                .read(studioControllerProvider.notifier)
                .selectThread(thread.id)
          : null,
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            key: StudioDriverKeys.renameThread(thread.id),
            tooltip: context.l10n.sidebarRenameSession,
            style: IconButton.styleFrom(
              minimumSize: const Size.square(30),
              tapTargetSize: MaterialTapTargetSize.shrinkWrap,
            ),
            icon: const Icon(Icons.edit_outlined, size: 17),
            onPressed: issue == null
                ? () =>
                      unawaited(_renameThreadFromSidebar(context, ref, thread))
                : null,
          ),
          IconButton(
            key: StudioDriverKeys.archiveThread(thread.id),
            tooltip: context.l10n.sidebarArchiveSession,
            style: IconButton.styleFrom(
              minimumSize: const Size.square(30),
              tapTargetSize: MaterialTapTargetSize.shrinkWrap,
            ),
            icon: const Icon(Icons.archive_outlined, size: 18),
            onPressed: issue == null && canArchive
                ? () => unawaited(
                    _archiveThreadFromSidebar(context, ref, thread.id),
                  )
                : null,
          ),
        ],
      ),
    );
    return KeyedSubtree(
      key: StudioDriverKeys.threadRow(thread.id),
      child: issue == null ? tile : Tooltip(message: issue.detail, child: tile),
    );
  }
}

Future<void> _archiveThreadFromSidebar(
  BuildContext context,
  WidgetRef ref,
  String threadId,
) async {
  try {
    await ref.read(studioControllerProvider.notifier).archiveThread(threadId);
  } on Object {
    if (!context.mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(context.l10n.sidebarArchiveSessionFailed)),
    );
  }
}

Future<void> _renameThreadFromSidebar(
  BuildContext context,
  WidgetRef ref,
  StudioThread thread,
) async {
  final title = await showDialog<String>(
    context: context,
    builder: (context) => _RenameThreadDialog(thread: thread),
  );
  if (title == null || !context.mounted) return;
  try {
    await ref
        .read(studioControllerProvider.notifier)
        .renameThread(thread.id, title);
  } on Object {
    if (!context.mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(context.l10n.sidebarRenameSessionFailed)),
    );
  }
}

class _RenameThreadDialog extends StatefulWidget {
  const _RenameThreadDialog({required this.thread});

  final StudioThread thread;

  @override
  State<_RenameThreadDialog> createState() => _RenameThreadDialogState();
}

class _RenameThreadDialogState extends State<_RenameThreadDialog> {
  late final TextEditingController _controller = TextEditingController(
    text: widget.thread.title,
  );
  String? _error;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _submit() {
    final title = _controller.text.trim();
    if (title.isEmpty) {
      setState(() => _error = context.l10n.sidebarRenameSessionEmpty);
      return;
    }
    if (title.runes.length > 80) {
      setState(() => _error = context.l10n.sidebarRenameSessionTooLong);
      return;
    }
    Navigator.of(context).pop(title);
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      key: StudioDriverKeys.renameThreadDialog(widget.thread.id),
      title: Text(context.l10n.sidebarRenameSessionTitle),
      content: TextField(
        key: StudioDriverKeys.renameThreadInput(widget.thread.id),
        controller: _controller,
        autofocus: true,
        maxLength: 80,
        textInputAction: TextInputAction.done,
        onSubmitted: (_) => _submit(),
        decoration: InputDecoration(
          labelText: context.l10n.sidebarRenameSessionInput,
          errorText: _error,
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(context.l10n.commonCancel),
        ),
        FilledButton(
          key: StudioDriverKeys.renameThreadSave(widget.thread.id),
          onPressed: _submit,
          child: Text(context.l10n.commonSave),
        ),
      ],
    );
  }
}
