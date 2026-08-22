part of 'studio_shell.dart';

class _Sidebar extends ConsumerWidget {
  const _Sidebar({required this.state, required this.compact});

  final SidebarView state;
  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final width = compact
        ? StudioLayout.compactRailWidth
        : StudioLayout.sidebarWidth;
    return SizedBox(
      key: StudioDriverKeys.sidebar,
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
              child: _SidebarDirectoryList(state: state, compact: compact),
            ),
            Divider(height: 1, color: context.studioLine),
            _SidebarActions(state: state, compact: compact),
          ],
        ),
      ),
    );
  }
}

/// 侧栏目录分页列表：项目区固定在顶部，会话区懒构建并触底加载下一页。
class _SidebarDirectoryList extends ConsumerStatefulWidget {
  const _SidebarDirectoryList({required this.state, required this.compact});

  final SidebarView state;
  final bool compact;

  @override
  ConsumerState<_SidebarDirectoryList> createState() =>
      _SidebarDirectoryListState();
}

class _SidebarDirectoryListState extends ConsumerState<_SidebarDirectoryList> {
  final _scrollController = ScrollController();
  var _loadMoreRequested = false;

  @override
  void initState() {
    super.initState();
    _scrollController.addListener(_handleScroll);
  }

  @override
  void dispose() {
    _scrollController.removeListener(_handleScroll);
    _scrollController.dispose();
    super.dispose();
  }

  void _handleScroll() {
    if (_loadMoreRequested || !widget.state.directoryHasMore) return;
    final position = _scrollController.position;
    if (position.extentAfter > 240) return;
    _loadMoreRequested = true;
    ref
        .read(studioControllerProvider.notifier)
        .loadMoreThreads()
        .whenComplete(() => _loadMoreRequested = false);
  }

  @override
  Widget build(BuildContext context) {
    final state = widget.state;
    final compact = widget.compact;
    final projects = state.projects;
    final threadCount = state.rootThreads.length;
    final showFooter = state.directoryHasMore || state.directoryIsLoading;
    // 非紧凑：[0]=项目标签，[1..P]=项目，[P+1]=会话标签；紧凑：[0..P-1]=项目，[P]=间隔。
    final headerCount = projects.length + (compact ? 1 : 2);
    final itemCount = headerCount + threadCount + (showFooter ? 1 : 0);
    return ListView.builder(
      controller: _scrollController,
      padding: EdgeInsets.symmetric(horizontal: compact ? 8 : 14),
      itemCount: itemCount,
      itemBuilder: (context, index) {
        if (index < headerCount) {
          if (compact) {
            if (index < projects.length) {
              return _projectTile(context, state, compact, index);
            }
            return const SizedBox(height: 12);
          }
          if (index == 0) {
            return Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                _SidebarSectionLabel(label: context.l10n.sidebarProjects),
                const SizedBox(height: 4),
              ],
            );
          }
          if (index <= projects.length) {
            return _projectTile(context, state, compact, index - 1);
          }
          return Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const SizedBox(height: 12),
              _SidebarSectionLabel(label: context.l10n.sidebarSessions),
              const SizedBox(height: 4),
            ],
          );
        }
        final threadIndex = index - headerCount;
        if (threadIndex < threadCount) {
          final thread = state.rootThreads[threadIndex];
          return _ThreadTile(
            thread: thread,
            selected: thread.id == state.selectedRootThreadId,
            compact: compact,
            recoveryIssue: state.threadRecoveryIssues[thread.id],
            canArchive:
                (thread.id != state.selectedRootThreadId || !state.isBusy) &&
                !thread.status.isActive,
          );
        }
        return _DirectoryLoadFooter(state: state);
      },
    );
  }

  Widget _projectTile(
    BuildContext context,
    SidebarView state,
    bool compact,
    int projectIndex,
  ) {
    final project = state.projects[projectIndex];
    return _ProjectTile(
      project: project,
      compact: compact,
      selected: project.id == state.selectedProjectId,
      recoveryIssue: state.projectRecoveryIssues[project.id],
    );
  }
}

class _DirectoryLoadFooter extends ConsumerWidget {
  const _DirectoryLoadFooter({required this.state});

  final SidebarView state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final error = ref.watch(directoryLoadErrorProvider);
    return Padding(
      key: const ValueKey('sidebar-directory-footer'),
      padding: const EdgeInsets.symmetric(vertical: 10),
      child: Center(
        child: error == null
            ? Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const SizedBox(
                    width: 14,
                    height: 14,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  ),
                  const SizedBox(width: 8),
                  Text(
                    context.l10n.sidebarLoadingMore,
                    style: Theme.of(context).textTheme.bodySmall
                        ?.copyWith(color: context.studioInkSoft),
                  ),
                ],
              )
            : Text(
                context.l10n.sidebarLoadError,
                style: Theme.of(context).textTheme.bodySmall
                    ?.copyWith(color: Theme.of(context).colorScheme.error),
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
          tooltip:
              issue?.detail ??
              (project.path.isEmpty ? project.name : project.path),
          icon: issue != null
              ? Icons.error_outline
              : selected
              ? Icons.folder
              : Icons.folder_open,
          iconColor: issue == null ? null : colors.error,
          onTap: issue == null
              ? () => controller.selectProject(project.id)
              : null,
          actionTooltip: issue == null
              ? context.l10n.sidebarCloseProject
              : _recoveryActionTooltip(context, issue),
          actionIcon: issue == null ? Icons.close : _recoveryActionIcon(issue),
          actionKey: issue == null ? null : _recoveryActionKey(issue),
          onAction: issue != null
              ? _recoveryAction(context, ref, issue)
              : () => _showProjectCleanupDialog(context, ref, project),
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
      subtitle: project.path,
      dense: true,
      iconColor: issue != null
          ? colors.error
          : selected
          ? StudioColors.clayDeep
          : colors.onSurfaceVariant,
      onTap: issue == null ? () => controller.selectProject(project.id) : null,
      trailing: IconButton(
        key: issue == null
            ? ValueKey('project-cleanup-${project.id}')
            : _recoveryActionKey(issue),
        tooltip: issue == null
            ? context.l10n.sidebarCloseProject
            : _recoveryActionTooltip(context, issue),
        style: IconButton.styleFrom(
          minimumSize: const Size.square(30),
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        ),
        icon: Icon(
          issue == null ? Icons.close : _recoveryActionIcon(issue),
          size: 17,
        ),
        onPressed: issue != null
            ? _recoveryAction(context, ref, issue)
            : () => _showProjectCleanupDialog(context, ref, project),
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
    required this.selected,
    required this.compact,
    required this.recoveryIssue,
    required this.canArchive,
  });

  final StudioThread thread;
  final bool selected;
  final bool compact;
  final StudioRecoveryIssue? recoveryIssue;
  final bool canArchive;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final modeIcon = thread.mode == StudioMode.task
        ? Icons.route
        : Icons.flash_on;
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
          actionKey: issue == null
              ? StudioDriverKeys.archiveThread(thread.id)
              : _recoveryActionKey(issue),
          actionTooltip: issue == null
              ? context.l10n.sidebarArchiveSession
              : _recoveryActionTooltip(context, issue),
          actionIcon: issue == null
              ? Icons.archive_outlined
              : _recoveryActionIcon(issue),
          onAction: issue != null
              ? _recoveryAction(context, ref, issue)
              : canArchive
              ? () => unawaited(
                  _archiveThreadFromSidebar(context, ref, thread.id),
                )
              : null,
        ),
      );
    }
    final tile = _SidebarTile(
      selected: selected,
      icon: issue == null ? modeIcon : Icons.error_outline,
      title: thread.title,
      subtitle: _threadSubtitle(context, thread),
      dense: true,
      iconColor: issue != null
          ? colors.error
          : selected
          ? StudioColors.clayDeep
          : colors.onSurfaceVariant,
      markerColor: issue != null
          ? null
          : thread.mode == StudioMode.task
          ? StudioColors.clay
          : StudioColors.sage,
      onTap: issue == null
          ? () => ref
                .read(studioControllerProvider.notifier)
                .selectThread(thread.id)
          : null,
      trailing: IconButton(
        key: issue == null
            ? StudioDriverKeys.archiveThread(thread.id)
            : _recoveryActionKey(issue),
        tooltip: issue == null
            ? context.l10n.sidebarArchiveSession
            : _recoveryActionTooltip(context, issue),
        style: IconButton.styleFrom(
          minimumSize: const Size.square(30),
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        ),
        icon: Icon(
          issue == null ? Icons.archive_outlined : _recoveryActionIcon(issue),
          size: 18,
        ),
        onPressed: issue != null
            ? _recoveryAction(context, ref, issue)
            : canArchive
            ? () =>
                  unawaited(_archiveThreadFromSidebar(context, ref, thread.id))
            : null,
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

Key? _recoveryActionKey(StudioRecoveryIssue issue) {
  if (issue.canRetry) {
    return StudioDriverKeys.retryRecoveryIssue(issue.id);
  }
  if (issue.availableActions.contains(RecoveryIssueAction.removeProject)) {
    final projectId = issue.projectId;
    if (projectId != null) {
      return ValueKey('project-cleanup-$projectId');
    }
  }
  return issue.canCleanup ? ValueKey('recovery-cleanup-${issue.id}') : null;
}

String _recoveryActionTooltip(BuildContext context, StudioRecoveryIssue issue) {
  return issue.canRetry
      ? context.l10n.recoveryRetryTooltip
      : context.l10n.recoveryCleanupTooltip;
}

IconData _recoveryActionIcon(StudioRecoveryIssue issue) {
  return issue.canRetry ? Icons.refresh : Icons.delete_sweep_outlined;
}

VoidCallback? _recoveryAction(
  BuildContext context,
  WidgetRef ref,
  StudioRecoveryIssue issue,
) {
  if (issue.canRetry) {
    return () => unawaited(_retryRecoveryIssue(context, ref, issue));
  }
  if (issue.canCleanup) {
    return () => _showRecoveryCleanupDialog(context, ref, issue);
  }
  return null;
}

Future<void> _retryRecoveryIssue(
  BuildContext context,
  WidgetRef ref,
  StudioRecoveryIssue issue,
) async {
  try {
    await ref
        .read(studioControllerProvider.notifier)
        .retryRecoveryIssue(issue.id);
  } catch (error) {
    if (!context.mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(context.l10n.recoveryRetryFailed(error.toString())),
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
    this.iconColor,
    this.actionKey,
  });

  final bool selected;
  final String tooltip;
  final IconData icon;
  final Color? iconColor;
  final VoidCallback? onTap;
  final String actionTooltip;
  final IconData actionIcon;
  final VoidCallback? onAction;
  final Key? actionKey;

  @override
  State<_CompactSidebarTile> createState() => _CompactSidebarTileState();
}

class _CompactSidebarTileState extends State<_CompactSidebarTile> {
  bool _hovering = false;

  @override
  Widget build(BuildContext context) {
    final actionVisible =
        widget.onAction != null && (widget.selected || _hovering);
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
                  icon: Icon(widget.icon, color: widget.iconColor),
                  onPressed: widget.onTap,
                ),
              ),
            ),
            if (widget.onAction != null)
              Positioned(
                right: 0,
                bottom: 0,
                child: IgnorePointer(
                  ignoring: !actionVisible,
                  child: AnimatedOpacity(
                    opacity: actionVisible ? 1 : 0,
                    duration: const Duration(milliseconds: 120),
                    child: IconButton(
                      key: widget.actionKey,
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
  final VoidCallback? onTap;
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

  final SidebarView state;
  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final horizontalPadding = compact ? 8.0 : 14.0;
    final selectedProjectId = state.selectedProjectId;
    final canCreateThread =
        selectedProjectId != null &&
        !state.projectRecoveryIssues.containsKey(selectedProjectId);
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
                  key: StudioDriverKeys.newSession,
                  tooltip: context.l10n.sidebarNewSession,
                  icon: Icons.add_comment_outlined,
                  onPressed: canCreateThread
                      ? ref
                            .read(studioControllerProvider.notifier)
                            .beginNewThread
                      : null,
                ),
                const SizedBox(height: 4),
                _SidebarActionButton(
                  key: StudioDriverKeys.openProject,
                  tooltip: context.l10n.sidebarOpenProject,
                  icon: Icons.create_new_folder,
                  onPressed: () => _openProject(context, ref),
                ),
                const SizedBox(height: 4),
                _SidebarActionButton(
                  key: StudioDriverKeys.settingsOpen,
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
                  key: StudioDriverKeys.newSession,
                  icon: Icons.add_comment_outlined,
                  tooltip: context.l10n.sidebarNewSession,
                  onPressed: canCreateThread
                      ? ref
                            .read(studioControllerProvider.notifier)
                            .beginNewThread
                      : null,
                ),
                const SizedBox(width: 4),
                _SidebarActionButton(
                  key: StudioDriverKeys.openProject,
                  icon: Icons.create_new_folder,
                  tooltip: context.l10n.sidebarOpenProject,
                  onPressed: () => _openProject(context, ref),
                ),
                const Spacer(),
                _SidebarActionButton(
                  key: StudioDriverKeys.settingsOpen,
                  tooltip: context.l10n.sidebarSettings,
                  icon: Icons.settings,
                  showIndicator: hasUpdate,
                  onPressed: () => context.go('/settings'),
                ),
              ],
            ),
    );
  }

  Future<void> _openProject(BuildContext context, WidgetRef ref) async {
    final path = await ref.read(projectDirectoryPickerProvider)(context);
    if (path == null || path.isEmpty) {
      return;
    }
    await ref.read(studioControllerProvider.notifier).openProject(path);
  }
}

class _DriverProjectPathDialog extends StatefulWidget {
  const _DriverProjectPathDialog();

  @override
  State<_DriverProjectPathDialog> createState() =>
      _DriverProjectPathDialogState();
}

class _DriverProjectPathDialogState extends State<_DriverProjectPathDialog> {
  final _controller = TextEditingController();

  String get _path => _controller.text.trim();

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      key: StudioDriverKeys.projectPathDialog,
      title: Text(context.l10n.sidebarOpenProject),
      content: TextField(
        key: StudioDriverKeys.projectPathInput,
        controller: _controller,
        autofocus: true,
        decoration: InputDecoration(
          labelText: context.l10n.agentDetailPathLabel,
        ),
        onChanged: (_) => setState(() {}),
        onSubmitted: (_) => _submit(),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(context.l10n.settingsCancel),
        ),
        FilledButton(
          key: StudioDriverKeys.projectPathSubmit,
          onPressed: _path.isEmpty ? null : _submit,
          child: Text(context.l10n.sidebarOpen),
        ),
      ],
    );
  }

  void _submit() {
    final path = _path;
    if (path.isNotEmpty) {
      Navigator.of(context).pop(path);
    }
  }
}

class _SidebarActionButton extends StatelessWidget {
  const _SidebarActionButton({
    required this.icon,
    required this.tooltip,
    required this.onPressed,
    this.showIndicator = false,
    super.key,
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
