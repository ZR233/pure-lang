part of 'studio_shell.dart';

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
      height: 58,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 24),
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
                    style: Theme.of(context).textTheme.titleMedium?.copyWith(
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
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                        color: context.studioInkSoft,
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
        color: context.studioPaper,
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
