part of 'studio_shell.dart';

class _Sidebar extends ConsumerWidget {
  const _Sidebar({required this.state, required this.compact});

  final StudioState state;
  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final width = compact
        ? StudioLayout.compactRailWidth
        : StudioLayout.sidebarWidth;
    return SizedBox(
      key: const ValueKey('studio-sidebar'),
      width: width,
      child: Material(
        color: context.studioPaper2,
        child: Column(
          children: [
            SizedBox(
              height: 52,
              child: Center(
                child: compact
                    ? const StudioIconBadge(
                        icon: Icons.auto_awesome_motion,
                        backgroundColor: StudioColors.clay,
                        foregroundColor: Colors.white,
                        size: 34,
                      )
                    : Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 20),
                        child: Row(
                          children: [
                            const StudioIconBadge(
                              icon: Icons.auto_awesome_motion,
                              backgroundColor: StudioColors.clay,
                              foregroundColor: Colors.white,
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
                padding: EdgeInsets.symmetric(horizontal: compact ? 8 : 14),
                children: [
                  if (!compact) ...[
                    _SidebarSectionLabel(label: context.l10n.sidebarProjects),
                    const SizedBox(height: 4),
                  ],
                  for (final project in state.projects)
                    _ProjectTile(
                      project: project,
                      compact: compact,
                      selected: project.id == state.selectedProjectId,
                      canArchive:
                          !state.isBusy ||
                          project.id != state.selectedProjectId,
                    ),
                  const SizedBox(height: 12),
                  if (!compact) ...[
                    _SidebarSectionLabel(label: context.l10n.sidebarSessions),
                    const SizedBox(height: 4),
                  ],
                  for (final session in state.rootSessions)
                    _SessionTile(
                      session: session,
                      selected: session.id == state.selectedRootSession?.id,
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

class _SidebarSectionLabel extends StatelessWidget {
  const _SidebarSectionLabel({required this.label});

  final String label;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8),
      child: Text(
        label.toUpperCase(),
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: context.text.labelSmall?.copyWith(
          color: context.studioInkSoft.withValues(alpha: 0.72),
          fontFamily: 'Consolas',
          fontSize: 10,
          fontWeight: FontWeight.w600,
          letterSpacing: 0,
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
      return _CompactSidebarTile(
        selected: selected,
        tooltip: project.path.isEmpty ? project.name : project.path,
        icon: selected ? Icons.folder : Icons.folder_open,
        onTap: () => controller.selectProject(project.id),
        actionTooltip: context.l10n.sidebarCloseProject,
        actionIcon: Icons.close,
        onAction: canArchive
            ? () => controller.archiveProject(project.id)
            : null,
      );
    }
    return _SidebarTile(
      selected: selected,
      icon: selected ? Icons.folder : Icons.folder_open,
      title: project.name,
      subtitle: project.path,
      dense: true,
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
    final modeIcon = session.mode == StudioMode.task
        ? Icons.route
        : Icons.flash_on;
    final colors = Theme.of(context).colorScheme;
    if (compact) {
      return _CompactSidebarTile(
        selected: selected,
        tooltip: session.title,
        icon: modeIcon,
        onTap: () => ref
            .read(studioControllerProvider.notifier)
            .selectSession(session.id),
        actionTooltip: context.l10n.sidebarArchiveSession,
        actionIcon: Icons.archive_outlined,
        onAction: canArchive
            ? () => ref
                  .read(studioControllerProvider.notifier)
                  .archiveSession(session.id)
            : null,
      );
    }
    return _SidebarTile(
      selected: selected,
      icon: modeIcon,
      title: session.title,
      subtitle: _sessionSubtitle(context, session),
      dense: true,
      iconColor: selected ? StudioColors.clayDeep : colors.onSurfaceVariant,
      markerColor: session.mode == StudioMode.task
          ? StudioColors.clay
          : StudioColors.sage,
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

class _CompactSidebarTile extends StatefulWidget {
  const _CompactSidebarTile({
    required this.selected,
    required this.tooltip,
    required this.icon,
    required this.onTap,
    required this.actionTooltip,
    required this.actionIcon,
    required this.onAction,
  });

  final bool selected;
  final String tooltip;
  final IconData icon;
  final VoidCallback onTap;
  final String actionTooltip;
  final IconData actionIcon;
  final VoidCallback? onAction;

  @override
  State<_CompactSidebarTile> createState() => _CompactSidebarTileState();
}

class _CompactSidebarTileState extends State<_CompactSidebarTile> {
  bool _hovering = false;

  @override
  Widget build(BuildContext context) {
    final actionVisible = widget.selected || _hovering;
    return MouseRegion(
      onEnter: (_) => setState(() => _hovering = true),
      onExit: (_) => setState(() => _hovering = false),
      child: SizedBox(
        width: 44,
        height: 44,
        child: Stack(
          children: [
            Positioned.fill(
              child: Tooltip(
                message: widget.tooltip,
                child: IconButton(
                  isSelected: widget.selected,
                  icon: Icon(widget.icon),
                  onPressed: widget.onTap,
                ),
              ),
            ),
            Positioned(
              right: 0,
              bottom: 0,
              child: IgnorePointer(
                ignoring: !actionVisible,
                child: AnimatedOpacity(
                  opacity: actionVisible ? 1 : 0,
                  duration: const Duration(milliseconds: 120),
                  child: IconButton(
                    tooltip: widget.actionTooltip,
                    style: IconButton.styleFrom(
                      minimumSize: const Size.square(20),
                      maximumSize: const Size.square(20),
                      padding: EdgeInsets.zero,
                      backgroundColor: context.studioPaper,
                    ),
                    icon: Icon(widget.actionIcon, size: 12),
                    onPressed: widget.onAction,
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _SidebarTile extends StatefulWidget {
  const _SidebarTile({
    required this.selected,
    required this.icon,
    required this.iconColor,
    required this.title,
    required this.subtitle,
    required this.dense,
    required this.onTap,
    required this.trailing,
    this.markerColor,
  });

  final bool selected;
  final IconData icon;
  final Color iconColor;
  final String title;
  final String subtitle;
  final bool dense;
  final VoidCallback onTap;
  final Widget trailing;
  final Color? markerColor;

  @override
  State<_SidebarTile> createState() => _SidebarTileState();
}

class _SidebarTileState extends State<_SidebarTile> {
  bool _hovering = false;

  @override
  Widget build(BuildContext context) {
    final foreground = widget.selected
        ? StudioColors.clayDeep
        : context.studioInk;
    final trailingVisible = widget.selected || _hovering;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: MouseRegion(
        onEnter: (_) => setState(() => _hovering = true),
        onExit: (_) => setState(() => _hovering = false),
        child: Material(
          color: widget.selected ? context.studioPaper : Colors.transparent,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(StudioRadii.md),
            side: BorderSide(
              color: widget.selected ? context.studioLine2 : Colors.transparent,
            ),
          ),
          clipBehavior: Clip.antiAlias,
          child: InkWell(
            onTap: widget.onTap,
            hoverColor: context.studioPaper.withValues(alpha: 0.72),
            child: Padding(
              padding: EdgeInsets.fromLTRB(
                10,
                widget.dense ? 6 : 8,
                4,
                widget.dense ? 6 : 8,
              ),
              child: Row(
                children: [
                  if (widget.markerColor == null)
                    Icon(widget.icon, size: 17, color: widget.iconColor)
                  else
                    SizedBox(
                      width: 17,
                      child: Center(
                        child: DecoratedBox(
                          decoration: BoxDecoration(
                            color: widget.markerColor,
                            borderRadius: BorderRadius.circular(
                              StudioRadii.pill,
                            ),
                          ),
                          child: const SizedBox.square(dimension: 5),
                        ),
                      ),
                    ),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          widget.title,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: context.text.labelLarge?.copyWith(
                            color: foreground,
                            fontWeight: widget.selected
                                ? FontWeight.w600
                                : FontWeight.w500,
                          ),
                        ),
                        if (widget.subtitle.isNotEmpty) ...[
                          const SizedBox(height: 1),
                          Text(
                            widget.subtitle,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: context.text.bodySmall?.copyWith(
                              color: widget.selected
                                  ? context.studioInkSoft
                                  : context.studioInkSoft.withValues(
                                      alpha: 0.72,
                                    ),
                            ),
                          ),
                        ],
                      ],
                    ),
                  ),
                  AnimatedOpacity(
                    opacity: trailingVisible ? 1 : 0,
                    duration: const Duration(milliseconds: 140),
                    alwaysIncludeSemantics: true,
                    child: IconTheme.merge(
                      data: IconThemeData(
                        color: trailingVisible
                            ? context.studioInkSoft
                            : context.studioInkSoft.withValues(alpha: 0.48),
                      ),
                      child: widget.trailing,
                    ),
                  ),
                ],
              ),
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
    final horizontalPadding = compact ? 8.0 : 14.0;
    final hasUpdate = ref.watch(
      studioUpdateControllerProvider.select((state) => state.hasUpdate),
    );
    return Padding(
      padding: EdgeInsets.fromLTRB(
        horizontalPadding,
        11,
        horizontalPadding,
        12,
      ),
      child: compact
          ? Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                _SidebarActionButton(
                  tooltip: context.l10n.sidebarNewSession,
                  icon: Icons.add_comment_outlined,
                  onPressed: state.selectedProjectId == null || state.isBusy
                      ? null
                      : ref
                            .read(studioControllerProvider.notifier)
                            .createSession,
                ),
                const SizedBox(height: 4),
                _SidebarActionButton(
                  tooltip: context.l10n.sidebarOpenProject,
                  icon: Icons.create_new_folder,
                  onPressed: () => _openProject(ref),
                ),
                const SizedBox(height: 4),
                _SidebarActionButton(
                  tooltip: context.l10n.sidebarSettings,
                  icon: Icons.settings,
                  showIndicator: hasUpdate,
                  onPressed: () => context.go('/settings'),
                ),
              ],
            )
          : Row(
              children: [
                _SidebarActionButton(
                  icon: Icons.add_comment_outlined,
                  tooltip: context.l10n.sidebarNewSession,
                  onPressed: state.selectedProjectId == null || state.isBusy
                      ? null
                      : ref
                            .read(studioControllerProvider.notifier)
                            .createSession,
                ),
                const SizedBox(width: 4),
                _SidebarActionButton(
                  icon: Icons.create_new_folder,
                  tooltip: context.l10n.sidebarOpenProject,
                  onPressed: () => _openProject(ref),
                ),
                const Spacer(),
                _SidebarActionButton(
                  tooltip: context.l10n.sidebarSettings,
                  icon: Icons.settings,
                  showIndicator: hasUpdate,
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
    required this.tooltip,
    required this.onPressed,
    this.showIndicator = false,
  });

  final IconData icon;
  final String tooltip;
  final VoidCallback? onPressed;
  final bool showIndicator;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return IconButton(
      tooltip: tooltip,
      icon: Stack(
        clipBehavior: Clip.none,
        children: [
          Icon(icon),
          if (showIndicator)
            Positioned(
              key: const ValueKey('studio-update-indicator'),
              right: -3,
              top: -3,
              child: DecoratedBox(
                decoration: BoxDecoration(
                  color: StudioColors.clay,
                  shape: BoxShape.circle,
                  border: Border.all(color: context.studioPaper, width: 1.5),
                ),
                child: const SizedBox.square(dimension: 8),
              ),
            ),
        ],
      ),
      style: IconButton.styleFrom(
        fixedSize: const Size.square(40),
        iconSize: 18,
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        foregroundColor: colors.onSurfaceVariant,
        disabledForegroundColor: colors.onSurfaceVariant.withValues(
          alpha: 0.38,
        ),
        hoverColor: context.studioPaper.withValues(alpha: 0.76),
        focusColor: context.studioPaper.withValues(alpha: 0.76),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(StudioRadii.sm),
        ),
      ),
      onPressed: onPressed,
    );
  }
}
