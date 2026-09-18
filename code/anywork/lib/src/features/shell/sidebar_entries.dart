part of 'studio_shell.dart';

class _ProjectTile extends ConsumerWidget {
  const _ProjectTile({
    required this.project,
    required this.selected,
    required this.expanded,
    required this.onToggle,
    this.onNavigate,
    required this.recoveryIssue,
  });
  final StudioProject project;
  final bool selected;
  final bool expanded;
  final VoidCallback onToggle;
  final VoidCallback? onNavigate;
  final StudioRecoveryIssue? recoveryIssue;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final controller = ref.read(studioControllerProvider.notifier);
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.arrowRight): () {
          if (!expanded) onToggle();
        },
        const SingleActivator(LogicalKeyboardKey.arrowLeft): () {
          if (expanded) onToggle();
        },
      },
      child: Focus(
        skipTraversal: true,
        child: KeyedSubtree(
          key: StudioDriverKeys.projectRow(project.id),
          child: Column(
            children: [
              Row(
                children: [
                  IconButton(
                    key: ValueKey('project-expand-${project.id}'),
                    tooltip: context.l10n.sidebarProjects,
                    onPressed: onToggle,
                    icon: Icon(
                      expanded ? Icons.expand_more : Icons.chevron_right,
                      size: 18,
                    ),
                  ),
                  Expanded(
                    child: Tooltip(
                      message: recoveryIssue?.detail ?? project.name,
                      child: InkWell(
                        onTap: recoveryIssue == null
                            ? () async {
                                await controller.selectProject(project.id);
                                onNavigate?.call();
                              }
                            : null,
                        child: Row(
                          children: [
                            Icon(
                              recoveryIssue == null
                                  ? Icons.folder_outlined
                                  : Icons.error_outline,
                              size: 17,
                            ),
                            const SizedBox(width: 6),
                            Flexible(
                              child: Text(
                                project.name,
                                maxLines: 2,
                                overflow: TextOverflow.ellipsis,
                                style: context.text.labelLarge?.copyWith(
                                  fontWeight: selected
                                      ? FontWeight.w700
                                      : FontWeight.w600,
                                ),
                              ),
                            ),
                            const SizedBox(width: 6),
                            Text(
                              project.sshAlias == null
                                  ? context.l10n.sidebarLocal
                                  : 'SSH',
                              style: context.text.labelSmall,
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                  IconButton(
                    key: selected
                        ? StudioDriverKeys.newSession
                        : ValueKey('project-new-session-${project.id}'),
                    tooltip:
                        '${context.l10n.sidebarNewSession} · ${project.name}',
                    onPressed: recoveryIssue == null
                        ? () async {
                            await controller.selectProject(project.id);
                            if (!context.mounted) return;
                            await controller.beginNewThread();
                            onNavigate?.call();
                          }
                        : null,
                    icon: const Icon(Icons.add, size: 18),
                  ),
                  PopupMenuButton<String>(
                    key: ValueKey('project-menu-${project.id}'),
                    tooltip: context.l10n.sidebarProjects,
                    icon: const Icon(Icons.more_horiz, size: 18),
                    onSelected: (action) async {
                      if (action == 'pin') {
                        await _saveSidebarPreferences(
                          ref,
                          projectId: project.id,
                        );
                      }
                      if (action == 'copy') {
                        await Clipboard.setData(
                          ClipboardData(text: project.path),
                        );
                      }
                      if (action == 'close') {
                        await controller.archiveProject(project.id);
                      }
                    },
                    itemBuilder: (_) => [
                      PopupMenuItem(
                        value: 'pin',
                        child: Text(
                          ref
                                      .watch(studioControllerProvider)
                                      .value
                                      ?.general
                                      .pinnedProjectIds
                                      .contains(project.id) ==
                                  true
                              ? context.l10n.sidebarUnpin
                              : context.l10n.sidebarPin,
                        ),
                      ),
                      PopupMenuItem(
                        value: 'copy',
                        child: Text(context.l10n.sidebarCopyPath),
                      ),
                      PopupMenuItem(
                        key: ValueKey('project-close-${project.id}'),
                        value: 'close',
                        child: Text(context.l10n.sidebarCloseProject),
                      ),
                    ],
                  ),
                ],
              ),
              Padding(
                padding: const EdgeInsets.only(left: 40, right: 12, bottom: 6),
                child: Align(
                  alignment: Alignment.centerLeft,
                  child: Tooltip(
                    message: project.path,
                    child: Text(
                      project.path,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: context.text.bodySmall,
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _ThreadTile extends ConsumerWidget {
  const _ThreadTile({
    required this.thread,
    required this.modeDisplayName,
    required this.selected,
    required this.recoveryIssue,
    this.onNavigate,
  });

  final StudioThread thread;
  final String? modeDisplayName;
  final bool selected;
  final StudioRecoveryIssue? recoveryIssue;
  final VoidCallback? onNavigate;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final modeIcon = thread.status == ThreadStatusView.waitingInteraction
        ? Icons.help_outline
        : Icons.chat_bubble_outline;
    final colors = Theme.of(context).colorScheme;
    final issue = recoveryIssue;
    // 只读消费 Thread 的 canonical 工作区模式：worktree 会话显示工作树标识，
    // local 会话不显示。标识不改变会话或项目配置。
    final worktreeMarker = thread.workspaceMode.isWorktree
        ? Tooltip(
            message: context.l10n.sidebarSessionWorktree,
            child: Icon(
              Icons.account_tree_outlined,
              key: StudioDriverKeys.threadWorkspaceMode(thread.id),
              size: 14,
              color: colors.onSurfaceVariant,
            ),
          )
        : null;
    final tile = _SidebarTile(
      selected: selected,
      icon: issue == null ? modeIcon : Icons.error_outline,
      title: thread.title,
      showTitleTooltip: issue == null,
      titleIcon: worktreeMarker,
      subtitle: _threadSubtitle(context, thread, modeDisplayName),
      dense: true,
      iconColor: issue != null
          ? colors.error
          : selected
          ? context.colors.onPrimaryContainer
          : colors.onSurfaceVariant,
      markerColor: null,
      onTap: issue == null
          ? () async {
              await ref
                  .read(studioControllerProvider.notifier)
                  .selectThread(thread.id);
              onNavigate?.call();
            }
          : null,
      trailing: PopupMenuButton<String>(
        key: ValueKey('thread-menu-${thread.id}'),
        icon: const Icon(Icons.more_horiz, size: 18),
        onSelected: (action) async {
          if (action == 'pin') {
            await _saveSidebarPreferences(ref, threadId: thread.id);
          }
          if (action == 'rename' && context.mounted) {
            await _renameThreadFromSidebar(context, ref, thread);
          }
          if (action == 'archive') {
            if (context.mounted) {
              await _archiveThreadFromSidebar(context, ref, thread.id);
            }
          }
        },
        itemBuilder: (_) => [
          PopupMenuItem(
            value: 'pin',
            child: Text(
              ref
                          .watch(studioControllerProvider)
                          .value
                          ?.general
                          .pinnedThreadIds
                          .contains(thread.id) ==
                      true
                  ? context.l10n.sidebarUnpin
                  : context.l10n.sidebarPin,
            ),
          ),
          PopupMenuItem(
            key: StudioDriverKeys.renameThread(thread.id),
            value: 'rename',
            enabled: issue == null,
            child: Text(context.l10n.sidebarRenameSession),
          ),
          PopupMenuItem(
            key: StudioDriverKeys.archiveThread(thread.id),
            value: 'archive',
            enabled: issue == null,
            child: Text(context.l10n.sidebarArchiveSession),
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
  // 归档是破坏性动作：确认对话框是唯一入口，取消不产生任何副作用。
  final confirmed = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(context.l10n.sidebarArchiveSessionConfirmTitle),
      content: Text(context.l10n.sidebarArchiveSessionConfirmBody),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(false),
          child: Text(context.l10n.commonCancel),
        ),
        FilledButton(
          key: StudioDriverKeys.archiveThreadConfirm,
          onPressed: () => Navigator.of(context).pop(true),
          child: Text(context.l10n.sidebarArchiveSessionConfirmAction),
        ),
      ],
    ),
  );
  if (confirmed != true || !context.mounted) return;
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
