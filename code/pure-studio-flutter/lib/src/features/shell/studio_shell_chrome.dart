part of 'studio_shell.dart';

class _Header extends StatelessWidget {
  const _Header({
    required this.state,
    required this.onOpenTodo,
    required this.showTodoButton,
  });

  final StudioState state;
  final VoidCallback? onOpenTodo;
  final bool showTodoButton;

  @override
  Widget build(BuildContext context) {
    final session = state.selectedRootSession;
    final projectId = session?.projectId ?? state.selectedProjectId;
    final project = state.projects
        .where((project) => project.id == projectId)
        .firstOrNull;
    final subtitle = _projectSubtitle(project);
    return SizedBox(
      height: 58,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16),
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(
              maxWidth: StudioLayout.conversationWidth,
            ),
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        session?.title ?? context.l10n.shellNoSession,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: Theme.of(context).textTheme.titleMedium
                            ?.copyWith(
                              color: context.studioInk,
                              fontWeight: FontWeight.w700,
                            ),
                      ),
                      if (subtitle.isNotEmpty) ...[
                        const SizedBox(height: 2),
                        Text(
                          subtitle,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.bodySmall
                              ?.copyWith(color: context.studioInkSoft),
                        ),
                      ],
                    ],
                  ),
                ),
                if (showTodoButton) ...[
                  const SizedBox(width: 8),
                  IconButton(
                    key: const ValueKey('todo-open-button'),
                    tooltip: context.l10n.timelineTodoListFallback,
                    icon: const Icon(Icons.checklist_outlined),
                    onPressed: onOpenTodo,
                  ),
                ],
                if (state.agentSessionsForSelectedRoot.isNotEmpty) ...[
                  const SizedBox(width: 4),
                  _AgentSwitcher(state: state),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _AgentSwitcher extends ConsumerStatefulWidget {
  const _AgentSwitcher({required this.state});

  final StudioState state;

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
    final sessions = widget.state.agentSessionsForSelectedRoot;
    final aggregateColor = _aggregateAgentColor(widget.state, sessions);
    return MenuAnchor(
      controller: _menuController,
      style: const MenuStyle(
        maximumSize: WidgetStatePropertyAll(Size(360, 560)),
      ),
      // The menu is wider than the compact chip. Pull it left so its trailing
      // edge follows the trigger instead of being clamped to the window edge.
      alignmentOffset: const Offset(-216, 6),
      menuChildren: [
        for (final session in sessions)
          MenuItemButton(
            key: ValueKey('agent-session-${session.id}'),
            leadingIcon: Padding(
              padding: EdgeInsets.only(
                left: _agentDepth(session, sessions) * 12,
              ),
              child: Icon(
                Icons.circle,
                size: 9,
                color: _agentStatusColor(widget.state, session),
              ),
            ),
            trailingIcon: session.id == widget.state.selectedAgentSessionId
                ? const Icon(Icons.check, size: 18)
                : null,
            onPressed: () {
              _menuController.close();
              ref
                  .read(studioControllerProvider.notifier)
                  .selectAgentSession(session.id);
            },
            child: SizedBox(
              width: 244,
              child: Row(
                children: [
                  Expanded(
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          _agentDisplayName(session),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                        if (session.ownerRole.trim().isNotEmpty)
                          Text(
                            session.ownerRole,
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
                    _agentShortStatus(session),
                    style: Theme.of(context).textTheme.labelSmall?.copyWith(
                      color: context.studioInkSoft,
                    ),
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
              key: const ValueKey('agent-switcher'),
              avatar: Icon(Icons.hub_outlined, size: 16, color: aggregateColor),
              label: Text(context.l10n.statusAgentsCount(sessions.length)),
              tooltip: context.l10n.statusAgentsCount(sessions.length),
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

int _agentDepth(StudioSession session, List<StudioSession> sessions) {
  var depth = 0;
  var parentId = session.parentSessionId;
  final visited = <String>{session.id};
  while (parentId != null && visited.add(parentId)) {
    final parent = sessions
        .where((candidate) => candidate.id == parentId)
        .firstOrNull;
    if (parent == null) {
      break;
    }
    depth += 1;
    parentId = parent.parentSessionId;
  }
  return depth;
}

Color _aggregateAgentColor(StudioState state, List<StudioSession> sessions) {
  if (sessions.any((session) => _isFaultedAgentStatus(session.agentStatus))) {
    return StudioColors.rose;
  }
  if (state.pendingInteractions.any(
    (interaction) =>
        sessions.any((session) => session.id == interaction.sessionId),
  )) {
    return StudioColors.ochre;
  }
  if (sessions.any((session) => _isRunningAgentStatus(session.agentStatus))) {
    return StudioColors.clay;
  }
  return StudioColors.sage;
}

Color _agentStatusColor(StudioState state, StudioSession session) {
  if (_isFaultedAgentStatus(session.agentStatus)) {
    return StudioColors.rose;
  }
  if (state.pendingInteractions.any(
    (interaction) => interaction.sessionId == session.id,
  )) {
    return StudioColors.ochre;
  }
  if (_isRunningAgentStatus(session.agentStatus)) {
    return StudioColors.clay;
  }
  return StudioColors.sage;
}

bool _isRunningAgentStatus(String status) => const {
  'queued',
  'running',
  'waiting',
  'waitingForDelivery',
}.contains(status);

bool _isFaultedAgentStatus(String status) =>
    const {'faulted', 'failed', 'errored', 'error'}.contains(status);

String _agentShortStatus(StudioSession session) {
  final status = session.agentStatus.trim();
  return status.isEmpty ? 'idle' : status;
}

String _agentDisplayName(StudioSession session) {
  if (!session.isRoot && session.title.trim().isNotEmpty) {
    return session.title.trim();
  }
  final role = session.ownerRole.trim();
  if (role.isEmpty) {
    return session.isRoot ? 'Agent' : session.id;
  }
  return '${role[0].toUpperCase()}${role.substring(1)}';
}

String _projectSubtitle(StudioProject? project) {
  if (project == null) {
    return '';
  }
  if (project.path.isEmpty) {
    return project.name;
  }
  return '${project.name} · ${project.path}';
}

String _sessionSubtitle(BuildContext context, StudioSession session) {
  final mode = context.compileModeLabel(session.mode);
  final hour = session.updatedAt.hour.toString().padLeft(2, '0');
  final minute = session.updatedAt.minute.toString().padLeft(2, '0');
  return context.l10n.shellSessionUpdated(mode, '$hour:$minute');
}

class _Footer extends StatelessWidget {
  const _Footer({required this.state});

  final StudioState state;

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(color: context.studioPaper),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          SessionStatusBar(state: state),
          ComposerDock(state: state),
        ],
      ),
    );
  }
}
