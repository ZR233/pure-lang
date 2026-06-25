import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_chrome.dart';
import '../interaction/composer_dock.dart';
import '../status/session_status_bar.dart';
import '../timeline/timeline_view.dart';

part 'studio_sidebar.dart';
part 'studio_shell_chrome.dart';

class StudioShell extends ConsumerWidget {
  const StudioShell({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final asyncState = ref.watch(studioControllerProvider);
    return asyncState.when(
      loading: () =>
          const Scaffold(body: Center(child: CircularProgressIndicator())),
      error: (error, stackTrace) =>
          Scaffold(body: Center(child: Text(error.toString()))),
      data: (state) => LayoutBuilder(
        builder: (context, constraints) {
          final compact = constraints.maxWidth < 900;
          return Scaffold(
            backgroundColor: context.studioPaper,
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
                          child: TimelineView(
                            sessionId: state.selectedSessionId,
                            rows: state.selectedTimelineRows,
                          ),
                        ),
                        _Footer(state: state),
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
