part of 'studio_shell.dart';

class AgentWorkspacePane extends ConsumerStatefulWidget {
  const AgentWorkspacePane({super.key});

  @override
  ConsumerState<AgentWorkspacePane> createState() => _AgentWorkspacePaneState();
}

class _AgentWorkspacePaneState extends ConsumerState<AgentWorkspacePane> {
  static const _todoPanelWidth = 304.0;
  static const _planPanelWidth = 424.0;
  static const _minimumTimelineWidth = 560.0;
  static const _minimumPlanTimelineWidth = 360.0;
  static const _maximumFooterFraction = 0.5;

  final _scaffoldKey = GlobalKey<ScaffoldState>();
  final Map<String, bool> _todoExpandedByThread = {};
  final Map<String, String> _expandedPlanByThread = {};
  final Map<String, String> _autoOpenedPlanByThread = {};
  final Set<String> _todoAutoOpened = {};

  @override
  Widget build(BuildContext context) {
    final asyncLayout = ref.watch(selectedWorkspaceLayoutProvider);
    final asyncStartPage = ref.watch(startPageProvider);
    return asyncLayout.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (error, stackTrace) => Center(child: Text(error.toString())),
      data: (layout) {
        if (layout == null) {
          return asyncStartPage.when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (error, stackTrace) => Center(child: Text(error.toString())),
            data: (startPage) => startPage.isStartPage
                ? _StudioStartPage(view: startPage)
                : const Center(child: CircularProgressIndicator()),
          );
        }
        return LayoutBuilder(
          builder: (context, constraints) {
            final todo = layout.todo;
            final threadId = layout.threadId;
            final plan = layout.planConfirmation;
            if (plan == null) {
              _expandedPlanByThread.remove(threadId);
            }
            final planOverlaysTimeline =
                constraints.maxWidth <
                _planPanelWidth + _minimumPlanTimelineWidth;
            final planExpanded =
                plan != null &&
                _expandedPlanByThread[threadId] == plan.interactionId;
            final planOverlayWidth = constraints.maxWidth < _planPanelWidth
                ? constraints.maxWidth
                : _planPanelWidth;
            final todoInDrawer =
                constraints.maxWidth < _todoPanelWidth + _minimumTimelineWidth;
            final todoExpanded = _todoExpandedByThread[threadId] ?? false;
            final footerMaxHeight = constraints.hasBoundedHeight
                ? constraints.maxHeight * _maximumFooterFraction
                : null;
            if (plan != null &&
                _autoOpenedPlanByThread[threadId] != plan.interactionId) {
              _autoOpenedPlanByThread[threadId] = plan.interactionId;
              WidgetsBinding.instance.addPostFrameCallback((_) {
                if (!mounted) return;
                final current = ref.read(selectedWorkspaceLayoutProvider).value;
                if (current?.threadId != threadId ||
                    current?.planConfirmation?.interactionId !=
                        plan.interactionId) {
                  return;
                }
                setState(
                  () => _expandedPlanByThread[threadId] = plan.interactionId,
                );
              });
            }
            if (plan == null &&
                todo != null &&
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
              endDrawer: !planExpanded && todoInDrawer && todo != null
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
                    child: Stack(
                      children: [
                        Positioned.fill(
                          child: Row(
                            children: [
                              Expanded(
                                child: Stack(
                                  children: [
                                    Positioned.fill(
                                      child: _AgentTimelineHost(
                                        threadId: threadId,
                                        planConfirmation: plan,
                                        planExpanded: planExpanded,
                                        onPlanToggle: plan == null
                                            ? null
                                            : () => _togglePlan(
                                                threadId,
                                                plan.interactionId,
                                              ),
                                      ),
                                    ),
                                    if (layout.isLoading)
                                      const Positioned.fill(
                                        child: ColoredBox(
                                          key: ValueKey(
                                            'agent-workspace-loading',
                                          ),
                                          color: Colors.transparent,
                                        ),
                                      ),
                                  ],
                                ),
                              ),
                              if (!planOverlaysTimeline &&
                                  plan != null &&
                                  planExpanded)
                                SizedBox(
                                  width: _planPanelWidth,
                                  child: PlanDetailPanel(
                                    plan: plan,
                                    onClose: () => _closePlan(threadId),
                                  ),
                                ),
                              if (!planExpanded &&
                                  !todoInDrawer &&
                                  todo != null &&
                                  todoExpanded)
                                SizedBox(
                                  width: _todoPanelWidth,
                                  child: TodoPanel(
                                    key: const ValueKey('todo-side-panel'),
                                    todo: todo,
                                    onClose: () => setState(
                                      () => _todoExpandedByThread[threadId] =
                                          false,
                                    ),
                                  ),
                                ),
                            ],
                          ),
                        ),
                        if (planOverlaysTimeline &&
                            plan != null &&
                            planExpanded)
                          Positioned(
                            top: 0,
                            right: 0,
                            bottom: 0,
                            width: planOverlayWidth,
                            child: PlanDetailPanel(
                              plan: plan,
                              overlay: true,
                              onClose: () => _closePlan(threadId),
                            ),
                          ),
                      ],
                    ),
                  ),
                  _AdaptiveFooter(
                    maxHeight: footerMaxHeight,
                    showTodo: !planExpanded && todo != null,
                    todoExpanded: !planExpanded && todoExpanded,
                    onToggleTodo: planExpanded || todo == null
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

  void _togglePlan(String threadId, String interactionId) {
    setState(() {
      if (_expandedPlanByThread[threadId] == interactionId) {
        _expandedPlanByThread.remove(threadId);
      } else {
        _expandedPlanByThread[threadId] = interactionId;
      }
    });
  }

  void _closePlan(String threadId) {
    setState(() => _expandedPlanByThread.remove(threadId));
  }
}

/// Keeps a large interaction dock scrollable when the desktop window is short.
class _AdaptiveFooter extends StatelessWidget {
  const _AdaptiveFooter({
    required this.maxHeight,
    required this.showTodo,
    required this.todoExpanded,
    required this.onToggleTodo,
  });

  final double? maxHeight;
  final bool showTodo;
  final bool todoExpanded;
  final VoidCallback? onToggleTodo;

  @override
  Widget build(BuildContext context) {
    final footer = _Footer(
      showTodo: showTodo,
      todoExpanded: todoExpanded,
      onToggleTodo: onToggleTodo,
    );
    if (maxHeight == null) {
      return footer;
    }
    return ConstrainedBox(
      constraints: BoxConstraints(maxHeight: maxHeight!),
      child: SingleChildScrollView(
        key: StudioDriverKeys.workspaceFooterScroll,
        primary: false,
        child: footer,
      ),
    );
  }
}

class _StudioStartPage extends StatelessWidget {
  const _StudioStartPage({required this.view});

  final StartPageView view;

  @override
  Widget build(BuildContext context) {
    final project = view.project;
    return Scaffold(
      key: StudioDriverKeys.startPage,
      backgroundColor: context.studioPaper,
      body: LayoutBuilder(
        builder: (context, constraints) {
          return SingleChildScrollView(
            child: ConstrainedBox(
              constraints: BoxConstraints(minHeight: constraints.maxHeight),
              child: Center(
                child: Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 24,
                    vertical: 32,
                  ),
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(
                      maxWidth: StudioLayout.conversationWidth,
                    ),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(
                          project == null
                              ? context.l10n.startPageOpenProjectTitle
                              : context.l10n.startPageWelcome,
                          textAlign: TextAlign.center,
                          style: Theme.of(context).textTheme.headlineSmall
                              ?.copyWith(
                                color: context.studioInk,
                                fontWeight: FontWeight.w600,
                              ),
                        ),
                        const SizedBox(height: 10),
                        Text(
                          project == null
                              ? context.l10n.startPageOpenProjectBody
                              : context.l10n.startPageProject(project.name),
                          textAlign: TextAlign.center,
                          style: Theme.of(context).textTheme.bodyMedium
                              ?.copyWith(color: context.studioInkSoft),
                        ),
                        const SizedBox(height: 28),
                        StartPageComposerDock(view: view),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          );
        },
      ),
    );
  }
}

class _AgentTimelineHost extends ConsumerWidget {
  const _AgentTimelineHost({
    required this.threadId,
    required this.planConfirmation,
    required this.planExpanded,
    required this.onPlanToggle,
  });

  final String threadId;
  final PlanConfirmationView? planConfirmation;
  final bool planExpanded;
  final VoidCallback? onPlanToggle;

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
          planConfirmation: planConfirmation,
          planExpanded: planExpanded,
          onPlanToggle: onPlanToggle,
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
