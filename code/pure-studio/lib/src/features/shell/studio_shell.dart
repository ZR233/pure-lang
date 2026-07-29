import 'dart:async';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/widget_previews.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../app/theme/material3_theme.dart';
import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/app_localizations.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_chrome.dart';
import '../../shared/studio_driver_keys.dart';
import '../update/studio_update_controller.dart';
import '../interaction/composer_dock.dart';
import '../status/session_status_bar.dart';
import '../timeline/timeline_view.dart';
import '../todo/session_todo_panel.dart';

part 'studio_sidebar.dart';
part 'recovery_cleanup_dialog.dart';
part 'studio_shell_chrome.dart';
part 'agent_workspace_pane.dart';
part 'agent_workspace_preview.dart';

class StudioShell extends ConsumerStatefulWidget {
  const StudioShell({super.key});

  @override
  ConsumerState<StudioShell> createState() => _StudioShellState();
}

class _StudioShellState extends ConsumerState<StudioShell> {
  @override
  Widget build(BuildContext context) {
    final asyncChrome = ref.watch(shellChromeProvider);
    final asyncSidebar = ref.watch(sidebarProvider);
    final asyncHeader = ref.watch(studioHeaderProvider);
    return asyncChrome.when(
      loading: () =>
          const Scaffold(body: Center(child: CircularProgressIndicator())),
      error: (error, stackTrace) => _StudioFatalError(error: error),
      data: (chrome) {
        final sidebar = asyncSidebar.value;
        final header = asyncHeader.value;
        if (sidebar == null || header == null) {
          return const Scaffold(
            body: Center(child: CircularProgressIndicator()),
          );
        }
        return LayoutBuilder(
          builder: (context, constraints) {
            final compact =
                constraints.maxWidth < StudioLayout.compactBreakpoint;
            return Scaffold(
              key: StudioDriverKeys.shell,
              backgroundColor: context.studioPaper,
              body: Row(
                children: [
                  _Sidebar(state: sidebar, compact: compact),
                  VerticalDivider(width: 1, color: context.studioLine),
                  Expanded(
                    child: DecoratedBox(
                      decoration: BoxDecoration(color: context.studioPaper),
                      child: Column(
                        children: [
                          _Header(state: header),
                          if (chrome.applicationRecoveryIssues.isNotEmpty)
                            _ApplicationRecoveryBanner(
                              issues: chrome.applicationRecoveryIssues,
                            ),
                          const Divider(height: 1),
                          const Expanded(child: AgentWorkspacePane()),
                        ],
                      ),
                    ),
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
