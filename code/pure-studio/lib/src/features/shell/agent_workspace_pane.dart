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
  final Map<String, bool> _todoExpandedByThread = {};
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
            final threadId = layout.threadId;
            final todoInDrawer =
                constraints.maxWidth < _todoPanelWidth + _minimumTimelineWidth;
            final todoExpanded = _todoExpandedByThread[threadId] ?? false;
            if (todo != null &&
                todo.items.any((item) => item.status != 'completed') &&
                _todoAutoOpened.add(threadId)) {
              WidgetsBinding.instance.addPostFrameCallback((_) {
                if (!mounted ||
                    ref.read(selectedWorkspaceLayoutProvider).value?.threadId !=
                        threadId) {
                  return;
                }
                if (todoInDrawer) {
                  _scaffoldKey.currentState?.openEndDrawer();
                } else {
                  setState(() => _todoExpandedByThread[threadId] = true);
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
                      child: TodoPanel(
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
                                child: _AgentTimelineHost(threadId: threadId),
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
                            child: TodoPanel(
                              key: const ValueKey('todo-side-panel'),
                              todo: todo,
                              onClose: () => setState(
                                () => _todoExpandedByThread[threadId] = false,
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
                                () => _todoExpandedByThread[threadId] =
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
  const _AgentTimelineHost({required this.threadId});

  final String threadId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final asyncTimeline = ref.watch(agentTimelineProvider(threadId));
    return asyncTimeline.when(
      loading: () => const SizedBox.shrink(),
      error: (error, stackTrace) => Center(child: Text(error.toString())),
      data: (timeline) {
        if (timeline == null) {
          return const SizedBox.shrink();
        }
        return TimelineView(
          threadId: threadId,
          rows: timeline.isLoading ? const [] : timeline.rows,
          turn: timeline.turn,
          isLoadingOlder: timeline.isLoadingOlderHistory,
          onLoadOlder: timeline.hasOlderHistory
              ? () => unawaited(
                  ref
                      .read(studioControllerProvider.notifier)
                      .loadOlderHistory(threadId),
                )
              : null,
        );
      },
    );
  }
}
