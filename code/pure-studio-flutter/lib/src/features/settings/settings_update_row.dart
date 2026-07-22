part of 'settings_page.dart';

class _StudioUpdateSettingsRow extends ConsumerWidget {
  const _StudioUpdateSettingsRow({required this.runtimeBusy});

  final bool runtimeBusy;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final updateState = ref.watch(studioUpdateControllerProvider);
    final theme = Theme.of(context);
    final action = _action(context, ref, updateState);
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Icon(
                Icons.system_update_alt,
                size: 20,
                color: theme.colorScheme.onSurfaceVariant,
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      context.l10n.settingsStudioUpdateTitle,
                      style: context.text.bodyMedium?.copyWith(
                        color: context.studioInk,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      _statusText(context, updateState),
                      style: context.text.bodySmall?.copyWith(
                        color: context.studioInkSoft,
                      ),
                    ),
                    if (runtimeBusy && updateState.hasUpdate) ...[
                      const SizedBox(height: 4),
                      Text(
                        context.l10n.settingsStudioUpdateBusy,
                        style: context.text.bodySmall?.copyWith(
                          color: theme.colorScheme.error,
                        ),
                      ),
                    ],
                    if (updateState.phase == StudioUpdatePhase.failed &&
                        updateState.errorMessage != null) ...[
                      const SizedBox(height: 4),
                      Text(
                        _errorText(context, updateState),
                        key: const ValueKey('studio-update-error'),
                        style: context.text.bodySmall?.copyWith(
                          color: theme.colorScheme.error,
                        ),
                      ),
                    ],
                  ],
                ),
              ),
              if (action != null) ...[const SizedBox(width: 16), action],
            ],
          ),
          if (updateState.phase == StudioUpdatePhase.downloading) ...[
            const SizedBox(height: 10),
            LinearProgressIndicator(
              key: const ValueKey('studio-update-progress'),
              value: updateState.progress,
            ),
          ],
        ],
      ),
    );
  }

  Widget? _action(
    BuildContext context,
    WidgetRef ref,
    StudioUpdateState state,
  ) {
    final controller = ref.read(studioUpdateControllerProvider.notifier);
    if (state.phase == StudioUpdatePhase.checking ||
        state.phase == StudioUpdatePhase.verifying) {
      return const SizedBox.square(
        dimension: 22,
        child: CircularProgressIndicator(strokeWidth: 2),
      );
    }
    if (state.phase == StudioUpdatePhase.downloading ||
        state.phase == StudioUpdatePhase.installerLaunched) {
      return null;
    }
    if (state.hasUpdate) {
      return Wrap(
        spacing: 8,
        runSpacing: 8,
        alignment: WrapAlignment.end,
        children: [
          TextButton(
            key: const ValueKey('studio-update-release-notes'),
            onPressed: controller.openReleaseNotes,
            child: Text(context.l10n.settingsStudioUpdateReleaseNotes),
          ),
          FilledButton(
            key: const ValueKey('studio-update-install'),
            onPressed: runtimeBusy ? null : controller.install,
            child: Text(context.l10n.settingsStudioUpdateInstall),
          ),
        ],
      );
    }
    if (state.phase == StudioUpdatePhase.disabled) return null;
    return OutlinedButton(
      key: const ValueKey('studio-update-check'),
      onPressed: controller.check,
      child: Text(context.l10n.settingsStudioUpdateCheck),
    );
  }

  String _statusText(BuildContext context, StudioUpdateState state) {
    final current = state.currentVersion;
    return switch (state.phase) {
      StudioUpdatePhase.disabled => context.l10n.settingsStudioUpdateDisabled(
        current,
      ),
      StudioUpdatePhase.idle => context.l10n.settingsStudioUpdateCurrent(
        current,
      ),
      StudioUpdatePhase.checking => context.l10n.settingsStudioUpdateChecking(
        current,
      ),
      StudioUpdatePhase.upToDate => context.l10n.settingsStudioUpdateLatest(
        current,
      ),
      StudioUpdatePhase.available => context.l10n.settingsStudioUpdateAvailable(
        current,
        state.update!.version,
      ),
      StudioUpdatePhase.downloading =>
        context.l10n.settingsStudioUpdateDownloading(
          state.update!.version,
          _progressPercent(state),
        ),
      StudioUpdatePhase.verifying => context.l10n.settingsStudioUpdateVerifying(
        state.update!.version,
      ),
      StudioUpdatePhase.installerLaunched =>
        context.l10n.settingsStudioUpdateInstallerLaunched(
          state.update!.version,
        ),
      StudioUpdatePhase.failed when state.update != null =>
        context.l10n.settingsStudioUpdateAvailable(
          current,
          state.update!.version,
        ),
      StudioUpdatePhase.failed => context.l10n.settingsStudioUpdateCurrent(
        current,
      ),
    };
  }

  String _errorText(BuildContext context, StudioUpdateState state) {
    if (state.errorCode == 'runtimeBusy') {
      return context.l10n.settingsStudioUpdateBusy;
    }
    return context.l10n.settingsStudioUpdateFailed(state.errorMessage ?? '');
  }

  int _progressPercent(StudioUpdateState state) {
    return ((state.progress ?? 0) * 100).round();
  }
}
