part of 'studio_shell.dart';

class _StudioFatalError extends ConsumerWidget {
  const _StudioFatalError({required this.error});

  final Object error;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Scaffold(
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
                Text(
                  context.l10n.runtimeFatalTitle,
                  style: Theme.of(context).textTheme.titleLarge,
                  textAlign: TextAlign.center,
                ),
                const SizedBox(height: 10),
                SelectableText(
                  error.toString(),
                  textAlign: TextAlign.center,
                  style: Theme.of(context).textTheme.bodySmall,
                ),
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
                child: Text(
                  context.l10n.recoveryGlobalWarning(issues.length),
                  style: context.text.bodySmall?.copyWith(
                    color: colors.onErrorContainer,
                  ),
                ),
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
            Icon(
              Icons.warning_amber_rounded,
              size: 18,
              color: colors.onTertiaryContainer,
            ),
            const SizedBox(width: 8),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    context.l10n.configRecoveryMessage,
                    style: context.text.bodySmall?.copyWith(
                      color: colors.onTertiaryContainer,
                    ),
                  ),
                  SelectableText(
                    context.l10n.configRecoveryBackupPath(notice.backupPath),
                    style: context.text.labelSmall?.copyWith(
                      color: colors.onTertiaryContainer,
                    ),
                  ),
                ],
              ),
            ),
            IconButton(
              key: const ValueKey('config-recovery-dismiss'),
              tooltip: context.l10n.configRecoveryDismissTooltip,
              onPressed: () => ref
                  .read(studioControllerProvider.notifier)
                  .dismissConfigRecoveryNotice(),
              icon: const Icon(Icons.close, size: 18),
              color: colors.onTertiaryContainer,
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
    return Tooltip(
      message: state.error?.message ?? message,
      child: ColoredBox(
        key: const ValueKey('persistence-state-banner'),
        color: colors.errorContainer,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 5),
          child: Row(
            children: [
              Icon(Icons.save_outlined, size: 18, color: colors.error),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  message,
                  style: context.text.bodySmall?.copyWith(
                    color: colors.onErrorContainer,
                  ),
                ),
              ),
              TextButton.icon(
                key: const ValueKey('persistence-retry'),
                onPressed: _retrying ? null : _retry,
                icon: _retrying
                    ? const SizedBox.square(
                        dimension: 14,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.refresh, size: 17),
                label: Text(context.l10n.persistenceRetry),
              ),
            ],
          ),
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

Future<void> _showRecoveryCleanupDialog(
  BuildContext context,
  WidgetRef ref,
  StudioRecoveryIssue issue,
) {
  return showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (context) => _RecoveryCleanupDialog(issue: issue),
  );
}

Future<void> _showProjectCleanupDialog(
  BuildContext context,
  WidgetRef ref,
  StudioProject project,
) {
  return showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (context) => _RecoveryCleanupDialog.project(project: project),
  );
}

class _RecoveryCleanupDialog extends ConsumerStatefulWidget {
  const _RecoveryCleanupDialog({required this.issue}) : project = null;

  const _RecoveryCleanupDialog.project({required this.project}) : issue = null;

  final StudioRecoveryIssue? issue;
  final StudioProject? project;

  bool get isProjectCleanup => project != null;

  @override
  ConsumerState<_RecoveryCleanupDialog> createState() =>
      _RecoveryCleanupDialogState();
}

class _RecoveryCleanupDialogState
    extends ConsumerState<_RecoveryCleanupDialog> {
  late Future<RecoveryCleanupPreview> _preview;
  bool _cleaning = false;
  String? _cleanupError;

  @override
  void initState() {
    super.initState();
    _preview = _loadPreview();
  }

  Future<RecoveryCleanupPreview> _loadPreview() {
    final controller = ref.read(studioControllerProvider.notifier);
    final project = widget.project;
    if (project != null) {
      return controller.previewProjectCleanup(project.id);
    }
    return controller.previewRecoveryIssueCleanup(widget.issue!.id);
  }

  void _refreshPreview() {
    setState(() {
      _cleanupError = null;
      _preview = _loadPreview();
    });
  }

  @override
  Widget build(BuildContext context) {
    final projectCleanup = widget.isProjectCleanup;
    return AlertDialog(
      title: Text(
        projectCleanup
            ? context.l10n.projectCleanupTitle
            : context.l10n.recoveryCleanupTitle,
      ),
      content: SizedBox(
        width: 600,
        child: FutureBuilder<RecoveryCleanupPreview>(
          future: _preview,
          builder: (context, snapshot) {
            if (snapshot.connectionState != ConnectionState.done) {
              return const SizedBox(
                height: 120,
                child: Center(child: CircularProgressIndicator()),
              );
            }
            if (snapshot.hasError) {
              return _RecoveryError(
                message: snapshot.error.toString(),
                onRetry: _refreshPreview,
              );
            }
            final preview = snapshot.requireData;
            return _RecoveryPreviewContent(
              preview: preview,
              body: projectCleanup
                  ? context.l10n.projectCleanupBody
                  : context.l10n.recoveryCleanupBody,
              showDetail: !projectCleanup,
              cleanupError: _cleanupError,
              onRefresh: _refreshPreview,
            );
          },
        ),
      ),
      actions: [
        TextButton(
          onPressed: _cleaning ? null : () => Navigator.of(context).pop(),
          child: Text(context.l10n.recoveryCleanupCancel),
        ),
        FutureBuilder<RecoveryCleanupPreview>(
          future: _preview,
          builder: (context, snapshot) {
            return FilledButton(
              key: ValueKey(
                projectCleanup
                    ? 'project-cleanup-confirm'
                    : 'recovery-cleanup-confirm',
              ),
              onPressed: !_cleaning && snapshot.hasData
                  ? () => _confirm(snapshot.requireData)
                  : null,
              child: _cleaning
                  ? const SizedBox.square(
                      dimension: 16,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : Text(
                      projectCleanup
                          ? context.l10n.projectCleanupConfirm
                          : context.l10n.recoveryCleanupConfirm,
                    ),
            );
          },
        ),
      ],
    );
  }

  Future<void> _confirm(RecoveryCleanupPreview preview) async {
    setState(() {
      _cleaning = true;
      _cleanupError = null;
    });
    try {
      final controller = ref.read(studioControllerProvider.notifier);
      final project = widget.project;
      if (project != null) {
        await controller.cleanupProject(project.id, preview.expectedRevision);
      } else {
        await controller.cleanupRecoveryIssue(
          widget.issue!.id,
          preview.expectedRevision,
        );
      }
      if (mounted) {
        Navigator.of(context).pop();
      }
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _cleaning = false;
        _cleanupError = context.l10n.recoveryCleanupFailed(error.toString());
      });
    }
  }
}

class _RecoveryPreviewContent extends StatelessWidget {
  const _RecoveryPreviewContent({
    required this.preview,
    required this.body,
    required this.showDetail,
    required this.cleanupError,
    required this.onRefresh,
  });

  final RecoveryCleanupPreview preview;
  final String body;
  final bool showDetail;
  final String? cleanupError;
  final VoidCallback onRefresh;

  @override
  Widget build(BuildContext context) {
    return ConstrainedBox(
      constraints: const BoxConstraints(maxHeight: 440),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(body),
          if (showDetail) ...[
            const SizedBox(height: 8),
            Text(
              preview.detail,
              style: context.text.bodySmall?.copyWith(
                color: context.studioInkSoft,
              ),
            ),
          ],
          const SizedBox(height: 12),
          if (preview.resources.isEmpty)
            Text(context.l10n.recoveryCleanupNoResources)
          else
            Flexible(
              child: ListView.separated(
                shrinkWrap: true,
                itemCount: preview.resources.length,
                separatorBuilder: (_, _) => const Divider(height: 1),
                itemBuilder: (context, index) =>
                    _RecoveryResourceRow(resource: preview.resources[index]),
              ),
            ),
          if (cleanupError != null) ...[
            const SizedBox(height: 12),
            Text(
              cleanupError!,
              key: const ValueKey('recovery-cleanup-error'),
              style: context.text.bodySmall?.copyWith(
                color: Theme.of(context).colorScheme.error,
              ),
            ),
            const SizedBox(height: 8),
            TextButton.icon(
              key: const ValueKey('recovery-cleanup-refresh'),
              onPressed: onRefresh,
              icon: const Icon(Icons.refresh),
              label: Text(context.l10n.recoveryCleanupRefreshPreview),
            ),
          ],
        ],
      ),
    );
  }
}

class _RecoveryResourceRow extends StatelessWidget {
  const _RecoveryResourceRow({required this.resource});

  final RecoveryCleanupResource resource;

  @override
  Widget build(BuildContext context) {
    final status = switch (resource.presence) {
      RecoveryResourcePresence.absent =>
        context.l10n.recoveryCleanupPresenceAbsent,
      RecoveryResourcePresence.complete =>
        context.l10n.recoveryCleanupPresenceComplete,
      RecoveryResourcePresence.partial =>
        context.l10n.recoveryCleanupPresencePartial,
    };
    final facts = <String>[
      status,
      if (resource.dirty) context.l10n.recoveryCleanupDirty,
      if (resource.aheadBy > 0)
        context.l10n.recoveryCleanupAhead(resource.aheadBy),
      if (resource.changedFileCount > 0)
        context.l10n.recoveryCleanupChangedFiles(resource.changedFileCount),
    ];
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            resource.path,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            style: context.text.bodyMedium?.copyWith(
              fontFamily: 'Consolas',
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 3),
          Text(
            resource.branch,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: context.text.bodySmall?.copyWith(fontFamily: 'Consolas'),
          ),
          const SizedBox(height: 3),
          Text(
            facts.join(' · '),
            style: context.text.bodySmall?.copyWith(
              color: resource.hasUnmergedWork
                  ? Theme.of(context).colorScheme.error
                  : context.studioInkSoft,
            ),
          ),
        ],
      ),
    );
  }
}

class _RecoveryError extends StatelessWidget {
  const _RecoveryError({required this.message, required this.onRetry});

  final String message;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(message),
        const SizedBox(height: 12),
        TextButton.icon(
          onPressed: onRetry,
          icon: const Icon(Icons.refresh),
          label: Text(context.l10n.runtimeFatalRetry),
        ),
      ],
    );
  }
}
