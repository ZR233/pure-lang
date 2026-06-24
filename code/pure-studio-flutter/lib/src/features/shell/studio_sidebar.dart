part of 'studio_shell.dart';

class _Sidebar extends ConsumerWidget {
  const _Sidebar({required this.state, required this.compact});

  final StudioState state;
  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final width = compact ? 68.0 : 268.0;
    return SizedBox(
      width: width,
      child: Material(
        color: context.studioPaper2,
        child: Column(
          children: [
            SizedBox(
              height: 62,
              child: Center(
                child: compact
                    ? const StudioIconBadge(
                        icon: Icons.auto_awesome_motion,
                        size: 34,
                      )
                    : Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 14),
                        child: Row(
                          children: [
                            const StudioIconBadge(
                              icon: Icons.auto_awesome_motion,
                              size: 34,
                            ),
                            const SizedBox(width: 10),
                            Expanded(
                              child: Text(
                                context.l10n.appTitle,
                                overflow: TextOverflow.ellipsis,
                                style: Theme.of(context).textTheme.titleMedium
                                    ?.copyWith(
                                      fontWeight: FontWeight.w700,
                                      color: context.studioInk,
                                    ),
                              ),
                            ),
                          ],
                        ),
                      ),
              ),
            ),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.symmetric(horizontal: 8),
                children: [
                  for (final project in state.projects)
                    _ProjectTile(
                      project: project,
                      compact: compact,
                      selected: project.id == state.selectedProjectId,
                      canArchive:
                          !state.isBusy ||
                          project.id != state.selectedProjectId,
                    ),
                  const SizedBox(height: 8),
                  if (!compact)
                    Padding(
                      padding: const EdgeInsets.symmetric(
                        horizontal: 8,
                        vertical: 6,
                      ),
                      child: Text(
                        context.l10n.sidebarSessions,
                        style: Theme.of(context).textTheme.labelSmall?.copyWith(
                          color: context.studioInkSoft,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                    ),
                  for (final session in state.sessions)
                    _SessionTile(
                      session: session,
                      selected: session.id == state.selectedSessionId,
                      compact: compact,
                      canArchive: !state.isBusy,
                    ),
                ],
              ),
            ),
            Divider(height: 1, color: context.studioLine),
            _SidebarActions(state: state, compact: compact),
          ],
        ),
      ),
    );
  }
}

class _ProjectTile extends ConsumerWidget {
  const _ProjectTile({
    required this.project,
    required this.compact,
    required this.selected,
    required this.canArchive,
  });

  final StudioProject project;
  final bool compact;
  final bool selected;
  final bool canArchive;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final colors = Theme.of(context).colorScheme;
    final controller = ref.read(studioControllerProvider.notifier);
    if (compact) {
      return Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Tooltip(
            message: project.path,
            child: IconButton(
              isSelected: selected,
              tooltip: project.name,
              icon: const Icon(Icons.folder_open),
              selectedIcon: Icon(Icons.folder, color: colors.onSurface),
              onPressed: () => controller.selectProject(project.id),
            ),
          ),
          IconButton(
            tooltip: context.l10n.sidebarCloseProject,
            icon: const Icon(Icons.close, size: 18),
            onPressed: canArchive
                ? () => controller.archiveProject(project.id)
                : null,
          ),
        ],
      );
    }
    return _SidebarTile(
      selected: selected,
      icon: selected ? Icons.folder : Icons.folder_open,
      title: project.name,
      subtitle: project.path,
      iconColor: selected ? StudioColors.clayDeep : colors.onSurfaceVariant,
      onTap: () => controller.selectProject(project.id),
      trailing: IconButton(
        tooltip: context.l10n.sidebarCloseProject,
        style: IconButton.styleFrom(
          minimumSize: const Size.square(30),
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        ),
        icon: const Icon(Icons.close, size: 17),
        onPressed: canArchive
            ? () => controller.archiveProject(project.id)
            : null,
      ),
    );
  }
}

class _SessionTile extends ConsumerWidget {
  const _SessionTile({
    required this.session,
    required this.selected,
    required this.compact,
    required this.canArchive,
  });

  final StudioSession session;
  final bool selected;
  final bool compact;
  final bool canArchive;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final modeIcon = session.mode == CompileMode.plan
        ? Icons.route
        : Icons.flash_on;
    final colors = Theme.of(context).colorScheme;
    if (compact) {
      return Tooltip(
        message: session.title,
        child: IconButton(
          isSelected: selected,
          icon: Icon(modeIcon),
          onPressed: () => ref
              .read(studioControllerProvider.notifier)
              .selectSession(session.id),
        ),
      );
    }
    return _SidebarTile(
      selected: selected,
      icon: modeIcon,
      title: session.title,
      subtitle: _sessionSubtitle(context, session),
      iconColor: selected ? StudioColors.clayDeep : colors.onSurfaceVariant,
      onTap: () =>
          ref.read(studioControllerProvider.notifier).selectSession(session.id),
      trailing: IconButton(
        tooltip: context.l10n.sidebarArchiveSession,
        style: IconButton.styleFrom(
          minimumSize: const Size.square(30),
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        ),
        icon: const Icon(Icons.archive_outlined, size: 18),
        onPressed: canArchive
            ? () => ref
                  .read(studioControllerProvider.notifier)
                  .archiveSession(session.id)
            : null,
      ),
    );
  }
}

class _SidebarTile extends StatelessWidget {
  const _SidebarTile({
    required this.selected,
    required this.icon,
    required this.iconColor,
    required this.title,
    required this.subtitle,
    required this.onTap,
    required this.trailing,
  });

  final bool selected;
  final IconData icon;
  final Color iconColor;
  final String title;
  final String subtitle;
  final VoidCallback onTap;
  final Widget trailing;

  @override
  Widget build(BuildContext context) {
    final foreground = selected ? StudioColors.clayDeep : context.studioInk;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: Material(
        color: selected ? context.studioPaper : Colors.transparent,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(StudioRadii.sm),
          side: BorderSide(
            color: selected ? context.studioLine2 : Colors.transparent,
          ),
        ),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: onTap,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(10, 7, 4, 7),
            child: Row(
              children: [
                Icon(icon, size: 17, color: iconColor),
                const SizedBox(width: 10),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: context.text.labelLarge?.copyWith(
                          color: foreground,
                          fontWeight: selected
                              ? FontWeight.w600
                              : FontWeight.w500,
                        ),
                      ),
                      if (subtitle.isNotEmpty) ...[
                        const SizedBox(height: 1),
                        Text(
                          subtitle,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: context.text.bodySmall?.copyWith(
                            color: context.studioInkSoft,
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
                trailing,
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _SidebarActions extends ConsumerWidget {
  const _SidebarActions({required this.state, required this.compact});

  final StudioState state;
  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(8, 9, 8, 10),
      child: compact
          ? Column(
              children: [
                IconButton(
                  tooltip: context.l10n.sidebarNewSession,
                  icon: const Icon(Icons.add_comment_outlined),
                  onPressed: state.selectedProjectId == null || state.isBusy
                      ? null
                      : ref
                            .read(studioControllerProvider.notifier)
                            .createSession,
                ),
                IconButton(
                  tooltip: context.l10n.sidebarOpenProject,
                  icon: const Icon(Icons.create_new_folder),
                  onPressed: () => _openProject(ref),
                ),
                IconButton(
                  tooltip: context.l10n.sidebarSettings,
                  icon: const Icon(Icons.settings),
                  onPressed: () => context.go('/settings'),
                ),
              ],
            )
          : Row(
              children: [
                Expanded(
                  child: _SidebarActionButton(
                    icon: Icons.add_comment_outlined,
                    label: context.l10n.sidebarNew,
                    tooltip: context.l10n.sidebarNewSession,
                    onPressed: state.selectedProjectId == null || state.isBusy
                        ? null
                        : ref
                              .read(studioControllerProvider.notifier)
                              .createSession,
                  ),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: _SidebarActionButton(
                    icon: Icons.create_new_folder,
                    label: context.l10n.sidebarOpen,
                    tooltip: context.l10n.sidebarOpenProject,
                    onPressed: () => _openProject(ref),
                  ),
                ),
                const SizedBox(width: 8),
                IconButton(
                  tooltip: context.l10n.sidebarSettings,
                  icon: const Icon(Icons.settings),
                  onPressed: () => context.go('/settings'),
                ),
              ],
            ),
    );
  }

  Future<void> _openProject(WidgetRef ref) async {
    final path = await FilePicker.getDirectoryPath();
    if (path == null || path.isEmpty) {
      return;
    }
    await ref.read(studioControllerProvider.notifier).openProject(path);
  }
}

class _SidebarActionButton extends StatelessWidget {
  const _SidebarActionButton({
    required this.icon,
    required this.label,
    required this.tooltip,
    required this.onPressed,
  });

  final IconData icon;
  final String label;
  final String tooltip;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Tooltip(
      message: tooltip,
      child: OutlinedButton.icon(
        icon: Icon(icon, size: 18),
        label: Text(label),
        style: OutlinedButton.styleFrom(
          foregroundColor: colors.onSurfaceVariant,
          backgroundColor: colors.surfaceContainerLowest,
          side: BorderSide(color: context.studioLine),
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
        ),
        onPressed: onPressed,
      ),
    );
  }
}
