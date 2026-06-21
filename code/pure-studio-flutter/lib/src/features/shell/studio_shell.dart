import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../interaction/composer_dock.dart';
import '../status/session_status_bar.dart';
import '../timeline/timeline_view.dart';

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
            backgroundColor: Theme.of(context).colorScheme.surface,
            body: Row(
              children: [
                _Sidebar(state: state, compact: compact),
                const VerticalDivider(width: 1),
                Expanded(
                  child: DecoratedBox(
                    decoration: BoxDecoration(
                      color: Theme.of(context).colorScheme.surface,
                    ),
                    child: Column(
                      children: [
                        _Header(state: state),
                        const Divider(height: 1),
                        Expanded(
                          child: TimelineView(
                            sessionId: state.selectedSessionId,
                            messages: state.selectedMessages,
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

class _Sidebar extends ConsumerWidget {
  const _Sidebar({required this.state, required this.compact});

  final StudioState state;
  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final width = compact ? 68.0 : 268.0;
    final colors = Theme.of(context).colorScheme;
    return SizedBox(
      width: width,
      child: Material(
        color: colors.surfaceContainerLow,
        child: Column(
          children: [
            SizedBox(
              height: 54,
              child: Center(
                child: compact
                    ? Icon(Icons.auto_awesome_motion, color: colors.primary)
                    : Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 12),
                        child: Row(
                          children: [
                            Icon(
                              Icons.auto_awesome_motion,
                              color: colors.primary,
                              size: 20,
                            ),
                            const SizedBox(width: 8),
                            Expanded(
                              child: Text(
                                'Pure Studio',
                                overflow: TextOverflow.ellipsis,
                                style: Theme.of(context).textTheme.titleSmall,
                              ),
                            ),
                          ],
                        ),
                      ),
              ),
            ),
            Expanded(
              child: ListView(
                padding: const EdgeInsets.symmetric(horizontal: 8),
                children: [
                  for (final project in state.projects)
                    _ProjectTile(
                      project: project,
                      compact: compact,
                      selected: project.id == state.selectedProjectId,
                      canArchive:
                          !state.isBusy ||
                          project.id != state.selectedProjectId,
                    ),
                  const SizedBox(height: 8),
                  if (!compact)
                    Padding(
                      padding: const EdgeInsets.symmetric(
                        horizontal: 8,
                        vertical: 6,
                      ),
                      child: Text(
                        'Sessions',
                        style: Theme.of(context).textTheme.labelSmall,
                      ),
                    ),
                  for (final session in state.sessions)
                    _SessionTile(
                      session: session,
                      selected: session.id == state.selectedSessionId,
                      compact: compact,
                      canArchive: !state.isBusy,
                    ),
                ],
              ),
            ),
            const Divider(height: 1),
            _SidebarActions(state: state, compact: compact),
          ],
        ),
      ),
    );
  }
}

class _ProjectTile extends ConsumerWidget {
  const _ProjectTile({
    required this.project,
    required this.compact,
    required this.selected,
    required this.canArchive,
  });

  final StudioProject project;
  final bool compact;
  final bool selected;
  final bool canArchive;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final colors = Theme.of(context).colorScheme;
    final controller = ref.read(studioControllerProvider.notifier);
    if (compact) {
      return Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Tooltip(
            message: project.path,
            child: IconButton(
              isSelected: selected,
              tooltip: project.name,
              icon: const Icon(Icons.folder_open),
              selectedIcon: Icon(Icons.folder, color: colors.onSurface),
              onPressed: () => controller.selectProject(project.id),
            ),
          ),
          IconButton(
            tooltip: 'Close project',
            icon: const Icon(Icons.close, size: 18),
            onPressed: canArchive
                ? () => controller.archiveProject(project.id)
                : null,
          ),
        ],
      );
    }
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: ListTile(
        dense: true,
        selected: selected,
        tileColor: selected
            ? colors.surfaceContainerHighest.withValues(alpha: 0.62)
            : null,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
        leading: Icon(
          selected ? Icons.folder : Icons.folder_open,
          color: selected ? colors.onSurface : null,
        ),
        title: Text(project.name, overflow: TextOverflow.ellipsis),
        subtitle: Text(
          project.path,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: Theme.of(
            context,
          ).textTheme.bodySmall?.copyWith(color: colors.onSurfaceVariant),
        ),
        trailing: IconButton(
          tooltip: 'Close project',
          style: IconButton.styleFrom(
            minimumSize: const Size.square(34),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          ),
          icon: const Icon(Icons.close, size: 18),
          onPressed: canArchive
              ? () => controller.archiveProject(project.id)
              : null,
        ),
        onTap: () => controller.selectProject(project.id),
      ),
    );
  }
}

class _SessionTile extends ConsumerWidget {
  const _SessionTile({
    required this.session,
    required this.selected,
    required this.compact,
    required this.canArchive,
  });

  final StudioSession session;
  final bool selected;
  final bool compact;
  final bool canArchive;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final modeIcon = session.mode == CompileMode.plan
        ? Icons.route
        : Icons.flash_on;
    final colors = Theme.of(context).colorScheme;
    final color = selected
        ? colors.surfaceContainerHighest.withValues(alpha: 0.62)
        : null;
    if (compact) {
      return Tooltip(
        message: session.title,
        child: IconButton(
          isSelected: selected,
          icon: Icon(modeIcon),
          onPressed: () => ref
              .read(studioControllerProvider.notifier)
              .selectSession(session.id),
        ),
      );
    }
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 2),
      child: ListTile(
        dense: true,
        selected: selected,
        tileColor: color,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(8)),
        leading: Icon(
          modeIcon,
          size: 18,
          color: selected ? colors.onSurface : colors.onSurfaceVariant,
        ),
        title: Text(
          session.title,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
        subtitle: Text(
          _sessionSubtitle(session),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: Theme.of(
            context,
          ).textTheme.bodySmall?.copyWith(color: colors.onSurfaceVariant),
        ),
        trailing: IconButton(
          tooltip: 'Archive session',
          style: IconButton.styleFrom(
            minimumSize: const Size.square(34),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          ),
          icon: const Icon(Icons.archive_outlined, size: 20),
          onPressed: canArchive
              ? () => ref
                    .read(studioControllerProvider.notifier)
                    .archiveSession(session.id)
              : null,
        ),
        onTap: () => ref
            .read(studioControllerProvider.notifier)
            .selectSession(session.id),
      ),
    );
  }
}

class _SidebarActions extends ConsumerWidget {
  const _SidebarActions({required this.state, required this.compact});

  final StudioState state;
  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Padding(
      padding: const EdgeInsets.all(8),
      child: compact
          ? Column(
              children: [
                IconButton(
                  tooltip: 'New session',
                  icon: const Icon(Icons.add_comment_outlined),
                  onPressed: state.selectedProjectId == null || state.isBusy
                      ? null
                      : ref
                            .read(studioControllerProvider.notifier)
                            .createSession,
                ),
                IconButton(
                  tooltip: 'Open project',
                  icon: const Icon(Icons.create_new_folder),
                  onPressed: () => _openProject(ref),
                ),
                IconButton(
                  tooltip: 'Settings',
                  icon: const Icon(Icons.settings),
                  onPressed: () => context.go('/settings'),
                ),
              ],
            )
          : Row(
              children: [
                Expanded(
                  child: _SidebarActionButton(
                    icon: Icons.add_comment_outlined,
                    label: 'New',
                    tooltip: 'New session',
                    onPressed: state.selectedProjectId == null || state.isBusy
                        ? null
                        : ref
                              .read(studioControllerProvider.notifier)
                              .createSession,
                  ),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: _SidebarActionButton(
                    icon: Icons.create_new_folder,
                    label: 'Open',
                    tooltip: 'Open project',
                    onPressed: () => _openProject(ref),
                  ),
                ),
                const SizedBox(width: 8),
                IconButton(
                  tooltip: 'Settings',
                  icon: const Icon(Icons.settings),
                  onPressed: () => context.go('/settings'),
                ),
              ],
            ),
    );
  }

  Future<void> _openProject(WidgetRef ref) async {
    final path = await FilePicker.getDirectoryPath();
    if (path == null || path.isEmpty) {
      return;
    }
    await ref.read(studioControllerProvider.notifier).openProject(path);
  }
}

class _SidebarActionButton extends StatelessWidget {
  const _SidebarActionButton({
    required this.icon,
    required this.label,
    required this.tooltip,
    required this.onPressed,
  });

  final IconData icon;
  final String label;
  final String tooltip;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Tooltip(
      message: tooltip,
      child: OutlinedButton.icon(
        icon: Icon(icon, size: 18),
        label: Text(label),
        style: OutlinedButton.styleFrom(
          foregroundColor: colors.onSurfaceVariant,
          side: BorderSide(color: colors.outlineVariant),
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
        ),
        onPressed: onPressed,
      ),
    );
  }
}

class _Header extends StatelessWidget {
  const _Header({required this.state});

  final StudioState state;

  @override
  Widget build(BuildContext context) {
    final session = state.sessions
        .where((session) => session.id == state.selectedSessionId)
        .firstOrNull;
    final projectId = session?.projectId ?? state.selectedProjectId;
    final project = state.projects
        .where((project) => project.id == projectId)
        .firstOrNull;
    final subtitle = _projectSubtitle(project);
    final colors = Theme.of(context).colorScheme;
    return SizedBox(
      height: 54,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 20),
        child: Row(
          children: [
            Expanded(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    session?.title ?? 'No session',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  if (subtitle.isNotEmpty) ...[
                    const SizedBox(height: 2),
                    Text(
                      subtitle,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                        color: colors.onSurfaceVariant,
                      ),
                    ),
                  ],
                ],
              ),
            ),
            if (state.isBusy) ...[
              Text(
                state.turnPhase.name,
                style: Theme.of(context).textTheme.labelSmall?.copyWith(
                  color: colors.onSurfaceVariant,
                ),
              ),
              const SizedBox(width: 10),
              const SizedBox.square(
                dimension: 16,
                child: CircularProgressIndicator(strokeWidth: 1.8),
              ),
            ],
          ],
        ),
      ),
    );
  }
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

String _sessionSubtitle(StudioSession session) {
  final mode = switch (session.mode) {
    CompileMode.auto => 'Auto',
    CompileMode.plan => 'Plan',
  };
  final hour = session.updatedAt.hour.toString().padLeft(2, '0');
  final minute = session.updatedAt.minute.toString().padLeft(2, '0');
  return '$mode · updated $hour:$minute';
}

class _Footer extends StatelessWidget {
  const _Footer({required this.state});

  final StudioState state;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: colors.surface,
        border: Border(
          top: BorderSide(color: colors.outlineVariant.withValues(alpha: 0.7)),
        ),
      ),
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
