part of 'studio_shell.dart';

class AgentWorkspacePane extends ConsumerStatefulWidget {
  const AgentWorkspacePane({super.key});

  @override
  ConsumerState<AgentWorkspacePane> createState() => _AgentWorkspacePaneState();
}

class _AgentWorkspacePaneState extends ConsumerState<AgentWorkspacePane> {
  static const _todoPanelWidth = 304.0;
  static const _minimumTimelineWidth = 560.0;

  final _scaffoldKey = GlobalKey<ScaffoldState>();
  final Map<String, bool> _todoExpandedBySession = {};
  final Set<String> _todoAutoOpened = {};

  @override
  Widget build(BuildContext context) {
    final asyncLayout = ref.watch(selectedWorkspaceLayoutProvider);
    return asyncLayout.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (error, stackTrace) => Center(child: Text(error.toString())),
      data: (layout) {
        if (layout == null) {
          return const SizedBox.shrink();
        }
        return LayoutBuilder(
          builder: (context, constraints) {
            final todo = layout.todo;
            final sessionId = layout.sessionId;
            final todoInDrawer =
                constraints.maxWidth < _todoPanelWidth + _minimumTimelineWidth;
            final todoExpanded = _todoExpandedBySession[sessionId] ?? false;
            if (todo != null &&
                todo.items.any((item) => item.status != 'completed') &&
                _todoAutoOpened.add(sessionId)) {
              WidgetsBinding.instance.addPostFrameCallback((_) {
                if (!mounted ||
                    ref
                            .read(selectedWorkspaceLayoutProvider)
                            .value
                            ?.sessionId !=
                        sessionId) {
                  return;
                }
                if (todoInDrawer) {
                  _scaffoldKey.currentState?.openEndDrawer();
                } else {
                  setState(() => _todoExpandedBySession[sessionId] = true);
                }
              });
            }
            return Scaffold(
              key: _scaffoldKey,
              backgroundColor: context.studioPaper,
              endDrawerEnableOpenDragGesture: false,
              endDrawer: todoInDrawer && todo != null
                  ? Drawer(
                      width: 328,
                      backgroundColor: context.studioPaper2,
                      child: SessionTodoPanel(
                        key: const ValueKey('todo-drawer-panel'),
                        todo: todo,
                        inDrawer: true,
                        onClose: () =>
                            _scaffoldKey.currentState?.closeEndDrawer(),
                      ),
                    )
                  : null,
              body: Column(
                children: [
                  Expanded(
                    child: Row(
                      children: [
                        Expanded(
                          child: Stack(
                            children: [
                              Positioned.fill(
                                child: _AgentTimelineHost(sessionId: sessionId),
                              ),
                              if (layout.isLoading)
                                const Positioned.fill(
                                  child: ColoredBox(
                                    key: ValueKey('agent-workspace-loading'),
                                    color: Colors.transparent,
                                  ),
                                ),
                            ],
                          ),
                        ),
                        if (!todoInDrawer && todo != null && todoExpanded)
                          SizedBox(
                            width: _todoPanelWidth,
                            child: SessionTodoPanel(
                              key: const ValueKey('todo-side-panel'),
                              todo: todo,
                              onClose: () => setState(
                                () => _todoExpandedBySession[sessionId] = false,
                              ),
                            ),
                          ),
                      ],
                    ),
                  ),
                  _Footer(
                    showTodo: todo != null,
                    todoExpanded: todoExpanded,
                    onToggleTodo: todo == null
                        ? null
                        : () {
                            if (todoInDrawer) {
                              _scaffoldKey.currentState?.openEndDrawer();
                            } else {
                              setState(
                                () => _todoExpandedBySession[sessionId] =
                                    !todoExpanded,
                              );
                            }
                          },
                  ),
                ],
              ),
            );
          },
        );
      },
    );
  }
}

class _AgentTimelineHost extends ConsumerWidget {
  const _AgentTimelineHost({required this.sessionId});

  final String sessionId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final asyncTimeline = ref.watch(agentTimelineProvider(sessionId));
    return asyncTimeline.when(
      loading: () => const SizedBox.shrink(),
      error: (error, stackTrace) => Center(child: Text(error.toString())),
      data: (timeline) {
        if (timeline == null) {
          return const SizedBox.shrink();
        }
        return TimelineView(
          sessionId: sessionId,
          rows: timeline.isLoading ? const [] : timeline.rows,
          turn: timeline.turn,
          isLoadingOlder: timeline.isLoadingOlderHistory,
          onLoadOlder: timeline.hasOlderHistory
              ? () => unawaited(
                  ref
                      .read(studioControllerProvider.notifier)
                      .loadOlderHistory(sessionId),
                )
              : null,
        );
      },
    );
  }
}
