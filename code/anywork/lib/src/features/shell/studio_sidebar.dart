part of 'studio_shell.dart';

class _Sidebar extends ConsumerStatefulWidget {
  const _Sidebar({super.key, required this.state, this.onNavigate});
  final SidebarView state;
  final VoidCallback? onNavigate;
  @override
  ConsumerState<_Sidebar> createState() => _SidebarState();
}

class _SidebarState extends ConsumerState<_Sidebar> {
  final _projectKeys = <String, GlobalKey>{};
  final _search = TextEditingController();
  final _searchFocus = FocusNode();
  final _expanded = <String>{};
  final _pages = <String, ThreadDirectoryPage>{};
  final _loading = <String>{};
  final _errors = <String, String>{};
  DirectoryFilter _filter = DirectoryFilter.all;
  bool _archived = false;
  Timer? _debounce;
  int _generation = 0;

  bool get _filtered =>
      _search.text.trim().isNotEmpty ||
      _filter != DirectoryFilter.all ||
      _archived;

  @override
  void initState() {
    super.initState();
    if (widget.state.selectedProjectId case final id?) _expanded.add(id);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _refresh();
    });
  }

  @override
  void didUpdateWidget(_Sidebar oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.state.selectedProjectId != widget.state.selectedProjectId) {
      if (widget.state.selectedProjectId case final id?) {
        _expanded.add(id);
        WidgetsBinding.instance.addPostFrameCallback((_) {
          final target = _projectKeys[id]?.currentContext;
          if (mounted && target != null) {
            Scrollable.ensureVisible(target, alignment: .1);
          }
        });
      }
    }
    final live = {
      for (final thread in widget.state.rootThreads) thread.id: thread,
    };
    final removed = oldWidget.state.rootThreads
        .map((thread) => thread.id)
        .toSet()
        .difference(live.keys.toSet());
    if (!_filtered) {
      for (final entry in _pages.entries.toList()) {
        final rows = <String, StudioThread>{
          for (final thread in entry.value.threads)
            if (!removed.contains(thread.id))
              thread.id: live[thread.id] ?? thread,
          for (final thread in live.values)
            if (thread.projectId == entry.key && !thread.archived)
              thread.id: thread,
        };
        _pages[entry.key] = ThreadDirectoryPage(
          threads: rows.values.toList(),
          nextCursor: entry.value.nextCursor,
        );
      }
    }
    final changed = oldWidget.state.rootThreads.any((before) {
      final after = live[before.id];
      return after != null &&
          (after.title != before.title ||
              after.status != before.status ||
              after.archived != before.archived);
    });
    // Merely incorporating a queried page must not reset pagination or issue
    // another query. Live metadata is overlaid above without discarding history.
    if (removed.isNotEmpty ||
        (_filtered && changed) ||
        (_archived && oldWidget.state != widget.state) ||
        (oldWidget.state.selectedProjectId != widget.state.selectedProjectId &&
            !_pages.containsKey(widget.state.selectedProjectId))) {
      _scheduleRefresh();
    }
  }

  @override
  void dispose() {
    _debounce?.cancel();
    _search.dispose();
    _searchFocus.dispose();
    super.dispose();
  }

  void _scheduleRefresh() {
    _debounce?.cancel();
    _generation++;
    _debounce = Timer(const Duration(milliseconds: 200), _refresh);
  }

  void _refresh() {
    _generation++;
    _loading.clear();
    for (final project in widget.state.projects) {
      if (_filtered || _expanded.contains(project.id)) {
        unawaited(_load(project.id));
      }
    }
  }

  Future<void> _load(String projectId, {bool more = false}) async {
    if (_loading.contains(projectId)) return;
    final generation = _generation;
    final previous = _pages[projectId];
    setState(() {
      _loading.add(projectId);
      _errors.remove(projectId);
    });
    try {
      final page = await ref
          .read(studioApiProvider)
          .queryThreads(
            DirectoryQuery(
              projectId: projectId,
              search: _search.text,
              filter: _filter,
              archived: _archived,
            ),
            cursor: more ? previous?.nextCursor : null,
            limit: more ? 20 : 8,
          );
      if (!mounted || generation != _generation) return;
      ref
          .read(studioControllerProvider.notifier)
          .includeDirectoryThreads(page.threads);
      final rows = <String, StudioThread>{
        if (more)
          for (final thread in previous?.threads ?? <StudioThread>[])
            thread.id: thread,
        for (final thread in page.threads) thread.id: thread,
      };
      setState(
        () => _pages[projectId] = ThreadDirectoryPage(
          threads: rows.values.toList(),
          nextCursor: page.nextCursor,
        ),
      );
    } catch (error) {
      if (mounted && generation == _generation) {
        setState(() => _errors[projectId] = error.toString());
      }
    } finally {
      if (mounted && generation == _generation) {
        setState(() => _loading.remove(projectId));
      }
    }
  }

  void _changeQuery() {
    setState(() {
      _pages.clear();
      _errors.clear();
    });
    _scheduleRefresh();
  }

  @override
  Widget build(BuildContext context) {
    final general =
        ref.watch(
          studioControllerProvider.select((state) => state.value?.general),
        ) ??
        const GeneralSettingsView();
    final projects =
        [
          ...widget.state.projects.where((project) {
            if (!_filtered ||
                _pages[project.id] == null ||
                _errors.containsKey(project.id)) {
              return true;
            }
            if (_pages[project.id]!.threads.isNotEmpty) {
              return true;
            }
            return !_archived &&
                _filter == DirectoryFilter.all &&
                '${project.name} ${project.path}'.toLowerCase().contains(
                  _search.text.trim().toLowerCase(),
                );
          }),
        ]..sort((a, b) {
          final pin =
              (general.pinnedProjectIds.contains(b.id) ? 1 : 0) -
              (general.pinnedProjectIds.contains(a.id) ? 1 : 0);
          return pin == 0
              ? widget.state.projects
                    .indexOf(a)
                    .compareTo(widget.state.projects.indexOf(b))
              : pin;
        });
    return Material(
      key: StudioDriverKeys.sidebar,
      color: context.colors.surfaceContainer,
      child: SafeArea(
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(14, 16, 14, 8),
              child: TextField(
                key: const ValueKey('sidebar-search'),
                controller: _search,
                focusNode: _searchFocus,
                decoration: InputDecoration(
                  hintText: context.l10n.sidebarSearch,
                  prefixIcon: const Icon(Icons.search, size: 18),
                  suffixIcon: _search.text.isEmpty
                      ? null
                      : IconButton(
                          tooltip: context.l10n.settingsCancel,
                          icon: const Icon(Icons.close, size: 16),
                          onPressed: () {
                            _search.clear();
                            _changeQuery();
                          },
                        ),
                  isDense: true,
                  filled: true,
                  fillColor: context.colors.surface,
                ),
                onChanged: (_) => _changeQuery(),
              ),
            ),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 14),
              child: Align(
                alignment: Alignment.centerLeft,
                child: Wrap(
                  spacing: 6,
                  children: [
                    for (final filter in DirectoryFilter.values)
                      ChoiceChip(
                        label: Text(switch (filter) {
                          DirectoryFilter.all => context.l10n.sidebarAll,
                          DirectoryFilter.running =>
                            context.l10n.sidebarRunning,
                          DirectoryFilter.attention =>
                            context.l10n.sidebarAttention,
                        }),
                        selected: _filter == filter,
                        showCheckmark: false,
                        onSelected: (_) {
                          _filter = filter;
                          _changeQuery();
                        },
                      ),
                  ],
                ),
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(18, 8, 12, 8),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      _archived
                          ? context.l10n.sidebarArchived
                          : context.l10n.sidebarProjects,
                      style: context.text.labelLarge,
                    ),
                  ),
                  TextButton.icon(
                    key: StudioDriverKeys.openProject,
                    onPressed: () => showAddProjectDialog(context),
                    icon: const Icon(Icons.add, size: 18),
                    label: Text(context.l10n.sidebarAddProject),
                  ),
                  if (widget.onNavigate != null)
                    IconButton(
                      tooltip: context.l10n.settingsCancel,
                      onPressed: widget.onNavigate,
                      icon: const Icon(Icons.close, size: 18),
                    ),
                ],
              ),
            ),
            Expanded(
              child: SingleChildScrollView(
                key: const ValueKey('sidebar-project-tree'),
                padding: const EdgeInsets.symmetric(horizontal: 10),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    if (projects.isEmpty)
                      Padding(
                        padding: const EdgeInsets.all(16),
                        child: Text(context.l10n.sidebarNoResults),
                      ),
                    for (final project in projects) ...[
                      KeyedSubtree(
                        key: _projectKeys.putIfAbsent(
                          project.id,
                          GlobalKey.new,
                        ),
                        child: _ProjectTile(
                          project: project,
                          selected:
                              project.id == widget.state.selectedProjectId,
                          expanded: _filtered || _expanded.contains(project.id),
                          onToggle: () {
                            setState(() {
                              if (!_expanded.remove(project.id)) {
                                _expanded.add(project.id);
                              }
                            });
                            if (_expanded.contains(project.id)) {
                              unawaited(_load(project.id));
                            }
                          },
                          onNavigate: widget.onNavigate,
                          recoveryIssue:
                              widget.state.projectRecoveryIssues[project.id],
                        ),
                      ),
                      if (_filtered || _expanded.contains(project.id))
                        ..._projectThreads(project, general),
                      const SizedBox(height: 10),
                    ],
                  ],
                ),
              ),
            ),
            _SidebarActions(
              archived: _archived,
              onToggleArchived: () {
                _archived = !_archived;
                _changeQuery();
              },
            ),
          ],
        ),
      ),
    );
  }

  List<Widget> _projectThreads(
    StudioProject project,
    GeneralSettingsView general,
  ) {
    final page = _pages[project.id];
    final rows =
        [
          ...(page?.threads ??
              (_filtered
                  ? <StudioThread>[]
                  : widget.state.rootThreads.where(
                      (t) => t.projectId == project.id && !t.archived,
                    ))),
        ]..sort((a, b) {
          final pin =
              (general.pinnedThreadIds.contains(b.id) ? 1 : 0) -
              (general.pinnedThreadIds.contains(a.id) ? 1 : 0);
          if (pin != 0) return pin;
          final date = b.updatedAt.compareTo(a.updatedAt);
          return date != 0 ? date : b.id.compareTo(a.id);
        });
    return [
      for (final thread in rows)
        Padding(
          padding: const EdgeInsets.only(left: 16),
          child: DecoratedBox(
            decoration: BoxDecoration(
              border: Border(
                left: BorderSide(color: context.colors.outlineVariant),
              ),
            ),
            child: Padding(
              padding: const EdgeInsets.only(left: 6),
              child: _archived
                  ? ListTile(
                      title: Text(
                        thread.title,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                      ),
                      trailing: IconButton(
                        key: ValueKey('thread-restore-${thread.id}'),
                        tooltip: context.l10n.sidebarRestore,
                        icon: const Icon(Icons.unarchive_outlined),
                        onPressed: () async {
                          try {
                            await ref
                                .read(studioControllerProvider.notifier)
                                .restoreThread(thread.id);
                            if (mounted) _refresh();
                          } catch (error) {
                            if (mounted) {
                              setState(
                                () => _errors[project.id] = error.toString(),
                              );
                            }
                          }
                        },
                      ),
                    )
                  : _ThreadTile(
                      thread: thread,
                      modeDisplayName:
                          widget.state.modeDisplayNames[thread.mode.id],
                      selected: thread.id == widget.state.selectedRootThreadId,
                      onNavigate: widget.onNavigate,
                      recoveryIssue:
                          widget.state.threadRecoveryIssues[thread.id],
                      canArchive:
                          !thread.status.isActive &&
                          (thread.id != widget.state.selectedRootThreadId ||
                              !widget.state.isBusy),
                    ),
            ),
          ),
        ),
      if (_loading.contains(project.id))
        const Padding(
          padding: EdgeInsets.all(8),
          child: LinearProgressIndicator(minHeight: 2),
        ),
      if (_errors[project.id] case final error?)
        Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            children: [
              Text(error, style: TextStyle(color: context.colors.error)),
              TextButton(
                onPressed: () => _load(project.id),
                child: Text(context.l10n.sidebarRetry),
              ),
            ],
          ),
        ),
      if (rows.isEmpty &&
          !_loading.contains(project.id) &&
          !_errors.containsKey(project.id))
        Padding(
          padding: const EdgeInsets.fromLTRB(36, 8, 12, 12),
          child: Text(
            _filtered
                ? context.l10n.sidebarNoResults
                : context.l10n.sidebarEmptyProject,
            style: context.text.bodySmall,
          ),
        ),
      if (page?.hasMore ?? false)
        TextButton(
          onPressed: _loading.contains(project.id)
              ? null
              : () => _load(project.id, more: true),
          child: Text(context.l10n.sidebarEarlier),
        ),
    ];
  }
}

Future<void> _saveSidebarPreferences(
  WidgetRef ref, {
  int? width,
  String? threadId,
  String? projectId,
}) async {
  final general = ref.read(studioControllerProvider).requireValue.general;
  final threads = [...general.pinnedThreadIds];
  final projects = [...general.pinnedProjectIds];
  if (threadId != null && !threads.remove(threadId)) threads.add(threadId);
  if (projectId != null && !projects.remove(projectId)) projects.add(projectId);
  await ref
      .read(studioControllerProvider.notifier)
      .saveGeneralSettings(
        GeneralSettingsCommand(
          followActiveTurn: general.followActiveTurn,
          compactTimeline: general.compactTimeline,
          sidebarWidth: width ?? general.sidebarWidth,
          pinnedThreadIds: threads,
          pinnedProjectIds: projects,
        ),
      );
}
