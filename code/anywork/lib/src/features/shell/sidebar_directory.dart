part of 'studio_shell.dart';

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
            modeDisplayName: state.modeDisplayNames[thread.mode.id],
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
                        ?.copyWith(color: context.colors.onSurfaceVariant),
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
          color: context.colors.onSurfaceVariant,
          fontFamily: 'Consolas',
          fontSize: 10,
          fontWeight: FontWeight.w600,
          letterSpacing: 0,
        ),
      ),
    );
  }
}
