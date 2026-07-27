import 'dart:async';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_chrome.dart';
import '../update/studio_update_controller.dart';
import '../interaction/composer_dock.dart';
import '../status/session_status_bar.dart';
import '../timeline/timeline_view.dart';
import '../todo/session_todo_panel.dart';

part 'studio_sidebar.dart';
part 'studio_shell_chrome.dart';

class StudioShell extends ConsumerStatefulWidget {
  const StudioShell({super.key});

  @override
  ConsumerState<StudioShell> createState() => _StudioShellState();
}

class _StudioShellState extends ConsumerState<StudioShell> {
  static const _todoPanelWidth = 304.0;
  static const _minimumTimelineWidth = 560.0;

  final _scaffoldKey = GlobalKey<ScaffoldState>();
  final Map<String, bool> _todoExpandedBySession = {};
  final Set<String> _todoAutoOpened = {};

  @override
  Widget build(BuildContext context) {
    final asyncState = ref.watch(studioControllerProvider);
    return asyncState.when(
      loading: () =>
          const Scaffold(body: Center(child: CircularProgressIndicator())),
      error: (error, stackTrace) =>
          Scaffold(body: Center(child: Text(error.toString()))),
      data: (state) => LayoutBuilder(
        builder: (context, constraints) {
          final compact = constraints.maxWidth < StudioLayout.compactBreakpoint;
          final sidebarWidth = compact
              ? StudioLayout.compactRailWidth
              : StudioLayout.sidebarWidth;
          final workspaceWidth = constraints.maxWidth - sidebarWidth - 1;
          final todoInDrawer =
              workspaceWidth < _todoPanelWidth + _minimumTimelineWidth;
          final sessionId = state.selectedAgentSessionId;
          final todo = state.selectedTodoList;
          final todoExpanded =
              sessionId != null && (_todoExpandedBySession[sessionId] ?? false);
          if (sessionId != null &&
              todo != null &&
              todo.items.any((item) => item.status != 'completed') &&
              _todoAutoOpened.add(sessionId)) {
            WidgetsBinding.instance.addPostFrameCallback((_) {
              if (!mounted ||
                  state.selectedAgentSessionId != sessionId ||
                  state.selectedTodoList == null) {
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
                      onClose: () => Navigator.of(context).maybePop(),
                    ),
                  )
                : null,
            body: Row(
              children: [
                _Sidebar(state: state, compact: compact),
                VerticalDivider(width: 1, color: context.studioLine),
                Expanded(
                  child: DecoratedBox(
                    decoration: BoxDecoration(color: context.studioPaper),
                    child: Column(
                      children: [
                        _Header(state: state),
                        const Divider(height: 1),
                        Expanded(
                          child: Row(
                            children: [
                              Expanded(
                                child: TimelineView(
                                  sessionId: state.selectedTimelineSessionId,
                                  rows: state.selectedTimelineRows,
                                ),
                              ),
                              if (!todoInDrawer && todo != null && todoExpanded)
                                SizedBox(
                                  width: 304,
                                  child: SessionTodoPanel(
                                    key: const ValueKey('todo-side-panel'),
                                    todo: todo,
                                    onClose: () => setState(
                                      () => _todoExpandedBySession[sessionId] =
                                          false,
                                    ),
                                  ),
                                ),
                            ],
                          ),
                        ),
                        _Footer(
                          state: state,
                          showTodo: todo != null,
                          todoExpanded: todoExpanded,
                          onToggleTodo: todo == null
                              ? null
                              : () {
                                  if (todoInDrawer) {
                                    _scaffoldKey.currentState?.openEndDrawer();
                                  } else if (sessionId != null) {
                                    setState(
                                      () => _todoExpandedBySession[sessionId] =
                                          !todoExpanded,
                                    );
                                  }
                                },
                        ),
                      ],
                    ),
                  ),
                ),
              ],
            ),
          );
        },
      ),
    );
  }
}
