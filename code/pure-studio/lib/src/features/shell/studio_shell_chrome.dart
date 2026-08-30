part of 'studio_shell.dart';

class _Header extends StatelessWidget {
  const _Header({required this.state});

  final HeaderView state;

  @override
  Widget build(BuildContext context) {
    final thread = state.selectedRootThread;
    final project = state.selectedProject;
    final projectLabel = project?.name.trim() ?? '';
    final workflowStage = state.runtime.workflow?.currentRun?.currentStageId;
    return SizedBox(
      height: 78,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16),
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(
              maxWidth: StudioLayout.conversationWidth,
            ),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  thread?.title ?? context.l10n.shellNoSession,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.titleMedium?.copyWith(
                    color: context.studioInk,
                    fontWeight: FontWeight.w700,
                  ),
                ),
                const SizedBox(height: 5),
                Row(
                  children: [
                    if (projectLabel.isNotEmpty)
                      Flexible(
                        child: Tooltip(
                          message: project?.path ?? projectLabel,
                          child: Text(
                            projectLabel,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: Theme.of(context).textTheme.bodySmall
                                ?.copyWith(color: context.studioInkSoft),
                          ),
                        ),
                      ),
                    if (projectLabel.isNotEmpty && workflowStage != null)
                      Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 7),
                        child: Text(
                          '·',
                          style: Theme.of(context).textTheme.bodySmall
                              ?.copyWith(color: context.studioInkSoft),
                        ),
                      ),
                    if (workflowStage != null)
                      Text(
                        workflowStage,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: Theme.of(context).textTheme.bodySmall?.copyWith(
                          color: StudioColors.clayDeep,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                    const Spacer(),
                    if (state.workspaceThreads.isNotEmpty)
                      Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          _AgentSwitcher(state: state),
                          const SizedBox(width: 8),
                          _SessionCostChip(cost: state.sessionCost),
                        ],
                      ),
                  ],
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _SessionCostChip extends StatelessWidget {
  const _SessionCostChip({required this.cost});

  final SessionCostView? cost;

  @override
  Widget build(BuildContext context) {
    final value = cost;
    final tooltip = value?.hasUnpricedUsage ?? false
        ? '${context.l10n.sessionAllAgentsCostTooltip} · ${context.l10n.statusUnpricedUsageLabel}'
        : context.l10n.sessionAllAgentsCostTooltip;
    return Tooltip(
      message: tooltip,
      child: Chip(
        key: StudioDriverKeys.sessionCost,
        avatar: const Icon(Icons.receipt_long_outlined, size: 16),
        label: Text(value?.label ?? '-'),
      ),
    );
  }
}

class _AgentSwitcher extends ConsumerStatefulWidget {
  const _AgentSwitcher({required this.state});

  final HeaderView state;

  @override
  ConsumerState<_AgentSwitcher> createState() => _AgentSwitcherState();
}

class _AgentSwitcherState extends ConsumerState<_AgentSwitcher> {
  final MenuController _menuController = MenuController();
  Timer? _hoverTimer;

  @override
  void dispose() {
    _hoverTimer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final threads = widget.state.workspaceThreads;
    final aggregateColor = _aggregateAgentColor(widget.state, threads);
    final viewport = MediaQuery.sizeOf(context);
    final availableWidth = (viewport.width - 24)
        .clamp(0.0, double.infinity)
        .toDouble();
    final minimumWidth = availableWidth < 240 ? availableWidth : 240.0;
    final menuWidth = (viewport.width * 0.36)
        .clamp(minimumWidth, availableWidth)
        .toDouble();
    // Reserve the compact header and overlay insets so a long menu can stay
    // below its anchor and scroll instead of being flipped over the header.
    final menuHeight = (viewport.height - 96)
        .clamp(0.0, double.infinity)
        .toDouble();
    final contentWidth = (menuWidth - 116).clamp(140.0, 244.0).toDouble();
    return MenuAnchor(
      controller: _menuController,
      style: MenuStyle(
        alignment: AlignmentDirectional.bottomEnd,
        minimumSize: WidgetStatePropertyAll(Size(menuWidth, 0)),
        maximumSize: WidgetStatePropertyAll(Size(menuWidth, menuHeight)),
      ),
      alignmentOffset: const Offset(0, 6),
      menuChildren: [
        for (final thread in threads)
          MenuItemButton(
            key: StudioDriverKeys.agentRow(thread.id),
            leadingIcon: Padding(
              padding: EdgeInsets.only(left: _agentDepth(thread, threads) * 12),
              child: Icon(
                _agentForThread(widget.state, thread.id)?.error != null
                    ? Icons.error_outline
                    : Icons.circle,
                size: _agentForThread(widget.state, thread.id)?.error != null
                    ? 16
                    : 9,
                color: _agentForThread(widget.state, thread.id)?.error != null
                    ? Theme.of(context).colorScheme.error
                    : _statusColor(widget.state, thread),
              ),
            ),
            trailingIcon: thread.id == widget.state.selectedThreadId
                ? const Icon(Icons.check, size: 18)
                : null,
            onPressed: () {
              _menuController.close();
              ref
                  .read(studioControllerProvider.notifier)
                  .selectAgentThread(thread.id);
            },
            child: SizedBox(
              width: contentWidth,
              child: Row(
                children: [
                  Expanded(
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          _agentDisplayName(context, thread),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                        if (thread.role.trim().isNotEmpty)
                          Text(
                            context.roleLabel(thread.role),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: Theme.of(context).textTheme.labelSmall
                                ?.copyWith(color: context.studioInkSoft),
                          ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 12),
                  Text(
                    _agentForThread(widget.state, thread.id)?.error ??
                        _agentShortStatus(thread),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: Theme.of(context).textTheme.labelSmall
                        ?.copyWith(color: context.studioInkSoft),
                  ),
                ],
              ),
            ),
          ),
      ],
      builder: (context, controller, child) {
        return MouseRegion(
          onEnter: (_) {
            _hoverTimer?.cancel();
            _hoverTimer = Timer(const Duration(milliseconds: 250), () {
              if (mounted && !_menuController.isOpen) {
                _menuController.open();
              }
            });
          },
          onExit: (_) => _hoverTimer?.cancel(),
          child: Focus(
            onFocusChange: (focused) {
              if (focused && !_menuController.isOpen) {
                _menuController.open();
              }
            },
            child: ActionChip(
              key: StudioDriverKeys.agentSwitcher,
              avatar: Icon(Icons.hub_outlined, size: 16, color: aggregateColor),
              label: Text(context.l10n.statusAgentsCount(threads.length)),
              tooltip: context.l10n.statusAgentsCount(threads.length),
              onPressed: () => _menuController.isOpen
                  ? _menuController.close()
                  : _menuController.open(),
            ),
          ),
        );
      },
    );
  }
}

StudioAgentView? _agentForThread(HeaderView state, String threadId) {
  for (final agent in state.agents) {
    if (agent.threadId == threadId) return agent;
  }
  return null;
}

int _agentDepth(StudioThread thread, List<StudioThread> threads) {
  var depth = 0;
  var parentId = thread.parentThreadId;
  final visited = <String>{thread.id};
  while (parentId != null && visited.add(parentId)) {
    final parent = threads
        .where((candidate) => candidate.id == parentId)
        .firstOrNull;
    if (parent == null) {
      break;
    }
    depth += 1;
    parentId = parent.parentThreadId;
  }
  return depth;
}

Color _aggregateAgentColor(HeaderView state, List<StudioThread> threads) {
  if (threads.any((thread) => _isFaultedAgentStatus(thread.status))) {
    return StudioColors.rose;
  }
  if (state.pendingInteractions.any(
    (interaction) => threads.any((thread) => thread.id == interaction.threadId),
  )) {
    return StudioColors.ochre;
  }
  if (threads.any((thread) => _isRunningAgentStatus(thread.status))) {
    return StudioColors.clay;
  }
  return StudioColors.sage;
}

Color _statusColor(HeaderView state, StudioThread thread) {
  if (_isFaultedAgentStatus(thread.status)) {
    return StudioColors.rose;
  }
  if (state.pendingInteractions.any(
    (interaction) => interaction.threadId == thread.id,
  )) {
    return StudioColors.ochre;
  }
  if (_isRunningAgentStatus(thread.status)) {
    return StudioColors.clay;
  }
  return StudioColors.sage;
}

bool _isRunningAgentStatus(ThreadStatusView status) => status.isActive;

bool _isFaultedAgentStatus(ThreadStatusView status) =>
    status == ThreadStatusView.faulted;

String _agentShortStatus(StudioThread thread) {
  return thread.status.name;
}

String _agentDisplayName(BuildContext context, StudioThread thread) {
  if (!thread.isRoot && thread.title.trim().isNotEmpty) {
    return thread.title.trim();
  }
  final role = thread.role.trim();
  if (role.isEmpty) {
    return thread.isRoot ? context.l10n.roleEmpty : thread.id;
  }
  return context.roleLabel(role);
}

String _threadSubtitle(
  BuildContext context,
  StudioThread thread,
  String? modeDisplayName,
) {
  final mode = modeDisplayName ?? context.compileModeLabel(thread.mode);
  final hour = thread.updatedAt.hour.toString().padLeft(2, '0');
  final minute = thread.updatedAt.minute.toString().padLeft(2, '0');
  return context.l10n.shellSessionUpdated(mode, '$hour:$minute');
}

class _Footer extends StatelessWidget {
  const _Footer({
    required this.showTodo,
    required this.todoExpanded,
    required this.onToggleTodo,
  });

  final bool showTodo;
  final bool todoExpanded;
  final VoidCallback? onToggleTodo;

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(color: context.studioPaper),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          _StatusBarHost(
            showTodo: showTodo,
            todoExpanded: todoExpanded,
            onToggleTodo: onToggleTodo,
          ),
          const _ComposerHost(),
        ],
      ),
    );
  }
}

class _StatusBarHost extends ConsumerWidget {
  const _StatusBarHost({
    required this.showTodo,
    required this.todoExpanded,
    required this.onToggleTodo,
  });

  final bool showTodo;
  final bool todoExpanded;
  final VoidCallback? onToggleTodo;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final asyncStatus = ref.watch(statusBarProvider);
    return asyncStatus.when(
      loading: () => const SizedBox.shrink(),
      error: (error, stackTrace) => const SizedBox.shrink(),
      data: (status) {
        if (status == null) {
          return const SizedBox.shrink();
        }
        return ThreadStatusBar(
          view: status,
          showTodo: showTodo,
          todoExpanded: todoExpanded,
          onToggleTodo: onToggleTodo,
        );
      },
    );
  }
}

class _ComposerHost extends ConsumerWidget {
  const _ComposerHost();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final asyncWorkspace = ref.watch(selectedWorkspaceControlsProvider);
    return asyncWorkspace.when(
      loading: () => const SizedBox.shrink(),
      error: (error, stackTrace) => const SizedBox.shrink(),
      data: (workspace) {
        if (workspace == null) {
          return const SizedBox.shrink();
        }
        return ComposerDock(workspace: workspace);
      },
    );
  }
}
