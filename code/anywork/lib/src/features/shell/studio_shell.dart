import 'dart:async';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widget_previews.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../data/frb/studio_api.dart' show FrbStudioApi;
import '../../shared/studio_loading.dart';
import '../../shared/recovery_check_status.dart';
import '../../app/theme/material3_theme.dart';
import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/app_localizations.dart';
import '../../l10n/studio_l10n.dart';
import '../../platform/vscode_launcher.dart';
import '../../shared/studio_chrome.dart';
import '../../shared/studio_driver_keys.dart';
import '../../shared/studio_driver_state.dart';
import '../update/studio_update_controller.dart';
import '../settings/settings_ssh_server_dialog.dart';
import '../settings/settings_remote_directory_dialog.dart';
import '../interaction/composer_dock.dart';
import '../status/thread_status_bar.dart';
import '../timeline/timeline_view.dart';
import '../todo/todo_panel.dart';

part 'studio_sidebar.dart';
part 'add_project_dialog.dart';
part 'sidebar_entries.dart';
part 'sidebar_tiles.dart';
part 'sidebar_actions.dart';
part 'runtime_banners.dart';
part 'studio_shell_chrome.dart';
part 'studio_startup.dart';
part 'agent_workspace_pane.dart';
part 'agent_workspace_preview.dart';

typedef ProjectDirectoryPicker = Future<String?> Function(BuildContext context);

final projectDirectoryPickerProvider = Provider<ProjectDirectoryPicker>((ref) {
  if (const bool.fromEnvironment('ANYWORK_DRIVER')) {
    return showDriverProjectPathDialog;
  }
  return (_) => FilePicker.getDirectoryPath();
});

Future<String?> showDriverProjectPathDialog(BuildContext context) {
  return showDialog<String>(
    context: context,
    builder: (_) => const _DriverProjectPathDialog(),
  );
}

class StudioShell extends ConsumerStatefulWidget {
  const StudioShell({super.key});

  @override
  ConsumerState<StudioShell> createState() => _StudioShellState();
}

class _StudioShellState extends ConsumerState<StudioShell> {
  final _scaffoldKey = GlobalKey<ScaffoldState>();
  final _sidebarKey = GlobalKey<_SidebarState>();
  final _startupClock = Stopwatch()..start();
  bool _reportedReady = false;
  double? _sidebarWidth;
  bool _sidebarHidden = false;

  void _focusSearch() {
    if (_sidebarKey.currentState == null) {
      _scaffoldKey.currentState?.openDrawer();
    }
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _sidebarKey.currentState?._searchFocus.requestFocus();
    });
  }

  void _newSession() {
    if (ref.read(studioControllerProvider).value?.selectedProjectId == null) {
      unawaited(showAddProjectDialog(context));
    } else {
      unawaited(ref.read(studioControllerProvider.notifier).beginNewThread());
    }
  }

  Future<void> _persistWidth() async {
    try {
      await _saveSidebarPreferences(ref, width: _sidebarWidth?.round());
      if (mounted) setState(() => _sidebarWidth = null);
    } catch (error) {
      if (mounted) {
        setState(() => _sidebarWidth = null);
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(error.toString())));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final savedWidth =
        ref.watch(
          studioControllerProvider.select(
            (state) => state.value?.general.sidebarWidth,
          ),
        ) ??
        336;
    final desiredWidth = _sidebarWidth ?? savedWidth.toDouble();
    final asyncChrome = ref.watch(shellChromeProvider);
    final asyncSidebar = ref.watch(sidebarProvider);
    final asyncHeader = ref.watch(studioHeaderProvider);
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.keyK, control: true):
            _focusSearch,
        const SingleActivator(LogicalKeyboardKey.keyK, meta: true):
            _focusSearch,
        const SingleActivator(LogicalKeyboardKey.keyN, control: true):
            _newSession,
        const SingleActivator(LogicalKeyboardKey.keyN, meta: true): _newSession,
      },
      child: asyncChrome.when(
        loading: () => const _StudioStartup(),
        error: (error, stackTrace) => _StudioFatalError(error: error),
        data: (chrome) {
          final sidebar = asyncSidebar.value;
          final header = asyncHeader.value;
          StudioDriverState.publishProject(header?.selectedProject);
          if (sidebar == null || header == null) {
            return const _StudioStartup();
          }
          if (!_reportedReady) {
            _reportedReady = true;
            WidgetsBinding.instance.addPostFrameCallback((_) {
              if (mounted) {
                debugPrint(
                  'startup_stage=first_usable_frame elapsed_ms=${_startupClock.elapsedMilliseconds}',
                );
              }
            });
          }
          return LayoutBuilder(
            builder: (context, constraints) {
              final compact =
                  constraints.maxWidth < StudioLayout.compactBreakpoint ||
                  MediaQuery.textScalerOf(context).scale(14) >= 21;
              final overlay = compact || _sidebarHidden;
              final maximumWidth = (constraints.maxWidth - 600).clamp(
                300.0,
                440.0,
              );
              final width =
                  (constraints.maxWidth < 1200 &&
                              _sidebarWidth == null &&
                              savedWidth == 336
                          ? 300.0
                          : desiredWidth)
                      .clamp(300.0, maximumWidth);
              return Scaffold(
                key: _scaffoldKey,
                drawer: overlay
                    ? Drawer(
                        width: (constraints.maxWidth - 24).clamp(0.0, 360.0),
                        child: _Sidebar(
                          key: _sidebarKey,
                          state: sidebar,
                          onNavigate: () =>
                              _scaffoldKey.currentState?.closeDrawer(),
                        ),
                      )
                    : null,
                backgroundColor: context.colors.surface,
                body: KeyedSubtree(
                  key: StudioDriverKeys.shell,
                  child: Row(
                    children: [
                      if (!overlay) ...[
                        SizedBox(
                          width: width,
                          child: _Sidebar(key: _sidebarKey, state: sidebar),
                        ),
                        Semantics(
                          label: context.l10n.sidebarResize,
                          child: Focus(
                            onKeyEvent: (_, event) {
                              if (event is! KeyDownEvent) {
                                return KeyEventResult.ignored;
                              }
                              final key = event.logicalKey;
                              if (key == LogicalKeyboardKey.home ||
                                  key == LogicalKeyboardKey.arrowLeft ||
                                  key == LogicalKeyboardKey.arrowRight) {
                                setState(
                                  () => _sidebarWidth =
                                      key == LogicalKeyboardKey.home
                                      ? 336
                                      : (desiredWidth +
                                                (key ==
                                                        LogicalKeyboardKey
                                                            .arrowLeft
                                                    ? -16
                                                    : 16))
                                            .clamp(300, maximumWidth),
                                );
                                unawaited(_persistWidth());
                                return KeyEventResult.handled;
                              }
                              return KeyEventResult.ignored;
                            },
                            child: MouseRegion(
                              cursor: SystemMouseCursors.resizeColumn,
                              child: GestureDetector(
                                onHorizontalDragUpdate: (event) => setState(
                                  () => _sidebarWidth = (width + event.delta.dx)
                                      .clamp(300, maximumWidth),
                                ),
                                onHorizontalDragEnd: (_) =>
                                    unawaited(_persistWidth()),
                                onDoubleTap: () {
                                  setState(() => _sidebarWidth = 336);
                                  unawaited(_persistWidth());
                                },
                                child: Container(
                                  width: 5,
                                  color: context.colors.outlineVariant
                                      .withValues(alpha: .5),
                                ),
                              ),
                            ),
                          ),
                        ),
                      ],
                      Expanded(
                        child: DecoratedBox(
                          decoration: BoxDecoration(
                            color: context.colors.surface,
                          ),
                          child: Column(
                            children: [
                              Row(
                                children: [
                                  IconButton(
                                    key: const ValueKey('sidebar-toggle'),
                                    tooltip: context.l10n.sidebarNavigation,
                                    onPressed: () {
                                      if (overlay) {
                                        _scaffoldKey.currentState?.openDrawer();
                                      } else {
                                        setState(() => _sidebarHidden = true);
                                      }
                                    },
                                    icon: Icon(
                                      overlay ? Icons.menu : Icons.chevron_left,
                                    ),
                                  ),
                                  if (_sidebarHidden && !compact)
                                    IconButton(
                                      tooltip: context.l10n.sidebarNavigation,
                                      onPressed: () => setState(
                                        () => _sidebarHidden = false,
                                      ),
                                      icon: const Icon(Icons.chevron_right),
                                    ),
                                  Expanded(child: _Header(state: header)),
                                ],
                              ),
                              if (chrome.persistenceState.needsAttention)
                                _PersistenceBanner(
                                  snapshot: chrome.persistenceState,
                                ),
                              if (chrome.applicationRecoveryIssues.isNotEmpty)
                                _ApplicationRecoveryBanner(
                                  issues: chrome.applicationRecoveryIssues,
                                ),
                              const RecoveryCheckStatus(),
                              const Divider(height: 1),
                              const Expanded(child: AgentWorkspacePane()),
                            ],
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              );
            },
          );
        },
      ),
    );
  }
}
