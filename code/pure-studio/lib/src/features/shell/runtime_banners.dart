part of 'studio_shell.dart';

class _StudioFatalError extends ConsumerWidget {
  const _StudioFatalError({required this.error});

  final Object error;

  @override
  Widget build(BuildContext context, WidgetRef ref) => Scaffold(
    body: Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 560),
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(
                Icons.error_outline,
                size: 44,
                color: Theme.of(context).colorScheme.error,
              ),
              const SizedBox(height: 16),
              Text(context.l10n.runtimeFatalTitle),
              const SizedBox(height: 10),
              SelectableText(error.toString(), textAlign: TextAlign.center),
              const SizedBox(height: 20),
              FilledButton.icon(
                key: const ValueKey('runtime-fatal-retry'),
                onPressed: () => ref
                    .read(studioControllerProvider.notifier)
                    .retryInitialization(),
                icon: const Icon(Icons.refresh),
                label: Text(context.l10n.runtimeFatalRetry),
              ),
            ],
          ),
        ),
      ),
    ),
  );
}

class _ApplicationRecoveryBanner extends StatelessWidget {
  const _ApplicationRecoveryBanner({required this.issues});

  final List<StudioRecoveryIssue> issues;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Tooltip(
      message: issues.map((issue) => issue.detail).join('\n'),
      child: ColoredBox(
        color: colors.errorContainer,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 7),
          child: Row(
            children: [
              Icon(Icons.warning_amber_rounded, size: 18, color: colors.error),
              const SizedBox(width: 8),
              Expanded(
                child: Text(context.l10n.recoveryGlobalWarning(issues.length)),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _ConfigRecoveryBanner extends ConsumerWidget {
  const _ConfigRecoveryBanner({required this.notice});

  final ConfigRecoveryNotice notice;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final colors = Theme.of(context).colorScheme;
    return ColoredBox(
      key: const ValueKey('config-recovery-banner'),
      color: colors.tertiaryContainer,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 7),
        child: Row(
          children: [
            const Icon(Icons.warning_amber_rounded, size: 18),
            const SizedBox(width: 8),
            Expanded(
              child: SelectableText(
                '${context.l10n.configRecoveryMessage}\n'
                '${context.l10n.configRecoveryBackupPath(notice.backupPath)}',
              ),
            ),
            IconButton(
              key: const ValueKey('config-recovery-dismiss'),
              onPressed: () => ref
                  .read(studioControllerProvider.notifier)
                  .dismissConfigRecoveryNotice(),
              icon: const Icon(Icons.close, size: 18),
            ),
          ],
        ),
      ),
    );
  }
}

class _PersistenceBanner extends ConsumerStatefulWidget {
  const _PersistenceBanner({required this.snapshot});

  final PersistenceStateSnapshot snapshot;

  @override
  ConsumerState<_PersistenceBanner> createState() => _PersistenceBannerState();
}

class _PersistenceBannerState extends ConsumerState<_PersistenceBanner> {
  bool _retrying = false;

  @override
  Widget build(BuildContext context) {
    final state = widget.snapshot.state;
    final colors = Theme.of(context).colorScheme;
    final message = switch (state) {
      DegradedPersistenceState(:final pendingCommits) =>
        context.l10n.persistenceDegraded(pendingCommits),
      RecoveringPersistenceState(:final pendingCommits) =>
        context.l10n.persistenceRecovering(pendingCommits),
      BlockedPersistenceState(:final pendingCommits) =>
        context.l10n.persistenceBlocked(pendingCommits),
      ReadyPersistenceState() || FlushingPersistenceState() => '',
    };
    return ColoredBox(
      key: const ValueKey('persistence-state-banner'),
      color: colors.errorContainer,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 5),
        child: Row(
          children: [
            const Icon(Icons.save_outlined, size: 18),
            const SizedBox(width: 8),
            Expanded(child: Text(message)),
            TextButton.icon(
              key: const ValueKey('persistence-retry'),
              onPressed: _retrying ? null : _retry,
              icon: const Icon(Icons.refresh, size: 17),
              label: Text(context.l10n.persistenceRetry),
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _retry() async {
    setState(() => _retrying = true);
    try {
      await ref.read(studioControllerProvider.notifier).retryPersistence();
    } finally {
      if (mounted) setState(() => _retrying = false);
    }
  }
}
