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

  /// 挂起的定向刷新项目集合。它和 [_fullPending] 一起表达待执行请求：
  /// 空集合不等于"没有请求"，因此全量请求不会被随后到达的定向请求覆盖。
  final _pendingProjects = <String>{};

  /// 是否挂起一次全量刷新（当前过滤/展开下的所有项目）。全量请求一旦挂起
  /// 就不能被定向请求降级，避免搜索/过滤刚清空分页却只重查了受影响项目。
  bool _fullPending = false;

  /// 全侧栏统一的输入模式：最近一次指针输入为触摸/触笔时视为触控，
  /// 所有行的快捷操作常显并放大命中区；出现鼠标 hover 或按键指针即回到
  /// 悬停模式。不猜测操作系统或硬件，只跟随真实指针事件（Listener 随
  /// 侧栏树卸载自动销毁，无手动监听需要清理）。
  bool _touchInput = false;
  Timer? _debounce;

  /// 目录查询代数：只在搜索/过滤/归档条件变化时递增，用于丢弃旧条件的在途
  /// 结果。单纯的定点/全量刷新不改代数，避免误丢弃无关项目的在途加载。
  int _queryGeneration = 0;

  bool get _filtered =>
      _search.text.trim().isNotEmpty ||
      _filter != DirectoryFilter.all ||
      _archived;

  /// 当前 canonical live 根会话（按 id）。唯一事实源仍是 controller 投影，
  /// 不引入第二份 canonical 缓存，也不比较时间戳。
  Map<String, StudioThread> get _liveById => {
    for (final thread in widget.state.rootThreads) thread.id: thread,
  };

  @override
  void initState() {
    super.initState();
    if (widget.state.selectedProjectId case final id?) _expanded.add(id);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _refreshNow();
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
    final live = _liveById;
    final removed = oldWidget.state.rootThreads
        .map((thread) => thread.id)
        .toSet()
        .difference(live.keys.toSet());
    // 目录事件把 canonical live 元数据按身份投影进**所有**已有分页行（含
    // 搜索/过滤/归档视图）：只更新已加载窗口内已存在的 result id，保序、保
    // nextCursor、不重置滚动/过滤/选择。元数据（如运行状态）变化即便不改变
    // 成员资格、不触发新查询，也会就地刷新，不再留下恒定旧值。
    for (final entry in _pages.entries.toList()) {
      final rows = <String, StudioThread>{
        for (final thread in entry.value.threads)
          if (!removed.contains(thread.id))
            thread.id: live[thread.id] ?? thread,
      };
      // 只有未过滤视图允许把 live 中本项目未归档行补进页面；过滤/归档视图
      // 的成员资格必须来自查询结果，不能本地追加行而污染结果集。
      if (!_filtered) {
        for (final thread in live.values) {
          if (thread.projectId == entry.key && !thread.archived) {
            rows[thread.id] = thread;
          }
        }
      }
      _pages[entry.key] = ThreadDirectoryPage(
        threads: rows.values.toList(),
        nextCursor: entry.value.nextCursor,
      );
    }
    // 只有事件确实改变当前搜索/过滤/归档成员资格时，才对受影响项目做定向重查询；
    // 单纯加载元数据更新不重置分页、不额外发起查询。
    final affected = _affectedProjects(oldWidget.state);
    if (affected.isNotEmpty) {
      _scheduleRefresh(projects: affected);
    }
  }

  /// 事件影响的、需要定向重查询的项目集合；空集表示已有分页无需查询。
  Set<String> _affectedProjects(SidebarView previous) {
    final previousById = {
      for (final thread in previous.rootThreads) thread.id: thread,
    };
    final liveById = {
      for (final thread in widget.state.rootThreads) thread.id: thread,
    };
    final affected = <String>{};
    // 会话被移除（归档/关闭）影响其所属项目页的成员资格。
    for (final thread in previous.rootThreads) {
      if (!liveById.containsKey(thread.id)) affected.add(thread.projectId);
    }
    for (final thread in widget.state.rootThreads) {
      final before = previousById[thread.id];
      if (before == null) {
        // 新增会话只影响过滤/归档视图；普通视图由 live 覆盖即时呈现。
        if (_filtered) affected.add(thread.projectId);
        continue;
      }
      final membershipChanged =
          before.archived != thread.archived ||
          before.projectId != thread.projectId ||
          (_filter != DirectoryFilter.all && before.status != thread.status) ||
          (_search.text.trim().isNotEmpty && before.title != thread.title);
      if (membershipChanged) {
        affected
          ..add(thread.projectId)
          ..add(before.projectId);
      }
    }
    if (previous.selectedProjectId != widget.state.selectedProjectId) {
      final selected = widget.state.selectedProjectId;
      if (selected != null && !_pages.containsKey(selected)) {
        affected.add(selected);
      }
    }
    return affected;
  }

  @override
  void dispose() {
    _debounce?.cancel();
    _search.dispose();
    _searchFocus.dispose();
    super.dispose();
  }

  /// 延迟一次刷新；[projects] 为受影响项目集合（null 表示全部展开/过滤项目）。
  /// 同一防抖窗口内：全量请求吸收定向请求，定向请求相互合并；全量优先，
  /// 不会被随后到达的定向请求降级。
  void _scheduleRefresh({Set<String>? projects}) {
    _debounce?.cancel();
    if (projects == null) {
      _fullPending = true;
    } else if (!_fullPending) {
      _pendingProjects.addAll(projects);
    }
    _debounce = Timer(const Duration(milliseconds: 200), _flushPending);
  }

  /// 立即执行一次全量刷新（初始化与归档恢复后使用）。
  void _refreshNow() {
    _debounce?.cancel();
    _fullPending = true;
    _flushPending();
  }

  /// 原子摘取挂起的刷新请求并执行：全量优先，否则只加载定向集合中仍处于
  /// 过滤或展开状态的项目。摘取与清空同时完成，执行期间到达的新请求进入
  /// 下一次调度，既不误丢弃也不遗漏；既有分页、滚动与过滤不被清空。
  void _flushPending() {
    _debounce = null;
    final full = _fullPending;
    final target = Set<String>.of(_pendingProjects);
    _fullPending = false;
    _pendingProjects.clear();
    for (final project in widget.state.projects) {
      if (!full && !target.contains(project.id)) continue;
      if (_filtered || _expanded.contains(project.id)) {
        unawaited(_load(project.id));
      }
    }
  }

  Future<void> _load(String projectId, {bool more = false}) async {
    if (_loading.contains(projectId)) return;
    final generation = _queryGeneration;
    final previous = _pages[projectId];
    setState(() {
      _loading.add(projectId);
      _errors.remove(projectId);
    });
    try {
      final page = await ref
          .read(studioControllerProvider.notifier)
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
      if (!mounted || generation != _queryGeneration) return;
      ref
          .read(studioControllerProvider.notifier)
          .includeDirectoryThreads(page.threads);
      // 查询结果是查询时的快照，可能早于已到达的 canonical 事件；按身份把
      // 最新 live 元数据投影到查询行上，避免迟到结果把最新 canonical 元数据
      // 遮回旧值（成员资格与顺序仍来自查询本身，不改变分页窗口）。
      final live = _liveById;
      final rows = <String, StudioThread>{
        if (more)
          for (final thread in previous?.threads ?? <StudioThread>[])
            thread.id: live[thread.id] ?? thread,
        for (final thread in page.threads) thread.id: live[thread.id] ?? thread,
      };
      setState(
        () => _pages[projectId] = ThreadDirectoryPage(
          threads: rows.values.toList(),
          nextCursor: page.nextCursor,
        ),
      );
    } catch (error) {
      if (mounted && generation == _queryGeneration) {
        setState(() => _errors[projectId] = error.toString());
      }
    } finally {
      if (mounted && generation == _queryGeneration) {
        setState(() => _loading.remove(projectId));
      }
    }
  }

  void _changeQuery() {
    // 条件变化：递增代数让旧条件的在途结果作废，并清空分页/错误/加载态。
    _queryGeneration++;
    setState(() {
      _pages.clear();
      _errors.clear();
      _loading.clear();
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
    return Listener(
      onPointerDown: (event) {
        final touch =
            event.kind == PointerDeviceKind.touch ||
            event.kind == PointerDeviceKind.stylus;
        if (touch != _touchInput) {
          setState(() => _touchInput = touch);
        }
      },
      onPointerHover: (_) {
        if (_touchInput) {
          setState(() => _touchInput = false);
        }
      },
      child: Material(
        key: StudioDriverKeys.sidebar,
        color: context.colors.surfaceContainer,
        child: SafeArea(
          child: Column(
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 16, 16, 8),
                child: TextField(
                  key: const ValueKey('sidebar-search'),
                  controller: _search,
                  focusNode: _searchFocus,
                  decoration: InputDecoration(
                    hintText: context.l10n.sidebarSearch,
                    prefixIcon: const Icon(Icons.search, size: 18),
                    suffixIcon: _search.text.isEmpty
                        ? null
                        : _tileIconButton(
                            tooltip: context.l10n.settingsCancel,
                            icon: Icons.close,
                            iconSize: 16,
                            minTarget: _touchInput ? 44 : 32,
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
                padding: const EdgeInsets.symmetric(horizontal: 16),
                child: Align(
                  alignment: Alignment.centerLeft,
                  child: Wrap(
                    spacing: 8,
                    runSpacing: 4,
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
                padding: const EdgeInsets.fromLTRB(20, 8, 16, 8),
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
                      _tileIconButton(
                        tooltip: context.l10n.settingsCancel,
                        icon: Icons.close,
                        iconSize: 18,
                        minTarget: _touchInput ? 44 : 32,
                        onPressed: widget.onNavigate,
                      ),
                  ],
                ),
              ),
              Expanded(
                child: SingleChildScrollView(
                  key: const ValueKey('sidebar-project-tree'),
                  padding: const EdgeInsets.symmetric(horizontal: 14),
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
                            expanded:
                                _filtered || _expanded.contains(project.id),
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
                            touchMode: _touchInput,
                          ),
                        ),
                        if (_filtered || _expanded.contains(project.id))
                          ..._projectThreads(project, general),
                        const SizedBox(height: 8),
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
          return StudioThread.compareDirectoryOrder(a, b);
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
                  ? _SidebarTile(
                      selected: false,
                      title: thread.title,
                      onTap: null,
                      touchMode: _touchInput,
                      trailing: _tileIconButton(
                        key: ValueKey('thread-restore-${thread.id}'),
                        tooltip: context.l10n.sidebarRestore,
                        icon: Icons.unarchive_outlined,
                        iconSize: 17,
                        minTarget: _touchInput ? 44 : 32,
                        onPressed: () async {
                          try {
                            await ref
                                .read(studioControllerProvider.notifier)
                                .restoreThread(thread.id);
                            if (mounted) _refreshNow();
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
                      project: project,
                      touchMode: _touchInput,
                      selected: thread.id == widget.state.selectedRootThreadId,
                      onNavigate: widget.onNavigate,
                      recoveryIssue:
                          widget.state.threadRecoveryIssues[thread.id],
                    ),
            ),
          ),
        ),
      if (_loading.contains(project.id))
        const Padding(
          padding: EdgeInsets.fromLTRB(36, 6, 8, 6),
          child: LinearProgressIndicator(minHeight: 2),
        ),
      if (_errors[project.id] case final error?)
        Padding(
          padding: const EdgeInsets.fromLTRB(36, 8, 12, 8),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
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
        Padding(
          padding: const EdgeInsets.only(left: 36, right: 8),
          child: Align(
            alignment: Alignment.centerLeft,
            child: TextButton(
              onPressed: _loading.contains(project.id)
                  ? null
                  : () => _load(project.id, more: true),
              child: Text(context.l10n.sidebarEarlier),
            ),
          ),
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
  final controller = ref.read(studioControllerProvider.notifier);
  if (width != null && width != general.sidebarWidth) {
    await controller.applySettingsField(GeneralSidebarWidthCommand(width));
  }
  if (threadId != null) {
    await controller.applySettingsField(GeneralPinnedThreadIdsCommand(threads));
  }
  if (projectId != null) {
    await controller.applySettingsField(
      GeneralPinnedProjectIdsCommand(projects),
    );
  }
}
