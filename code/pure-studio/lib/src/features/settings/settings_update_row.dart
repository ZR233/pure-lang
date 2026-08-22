import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../update/studio_update_controller.dart';

class StudioUpdateSettingsRow extends ConsumerWidget {
  const StudioUpdateSettingsRow({super.key, required this.runtimeBusy});

  final bool runtimeBusy;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final updateState = ref.watch(studioUpdateControllerProvider);
    final currentVersion = ref.watch(studioVersionProvider);
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
                      _statusText(context, updateState, currentVersion),
                      style: context.text.bodySmall?.copyWith(
                        color: context.studioInkSoft,
                      ),
                    ),
                    if (runtimeBusy && _hasUpdate(updateState)) ...[
                      const SizedBox(height: 4),
                      Text(
                        context.l10n.settingsStudioUpdateBusy,
                        style: context.text.bodySmall?.copyWith(
                          color: theme.colorScheme.error,
                        ),
                      ),
                    ],
                    if (_error(updateState) != null) ...[
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
          if (updateState case DownloadingUpdaterStateSnapshot()) ...[
            const SizedBox(height: 10),
            LinearProgressIndicator(
              key: const ValueKey('studio-update-progress'),
              value: _progress(updateState),
            ),
          ],
        ],
      ),
    );
  }

  Widget? _action(
    BuildContext context,
    WidgetRef ref,
    UpdaterStateSnapshot state,
  ) {
    final controller = ref.read(studioUpdateControllerProvider.notifier);
    if (state is CheckingUpdaterStateSnapshot) {
      return const SizedBox.square(
        dimension: 22,
        child: CircularProgressIndicator(strokeWidth: 2),
      );
    }
    if (state is DownloadingUpdaterStateSnapshot ||
        state is VerifyingUpdaterStateSnapshot) {
      return Wrap(
        spacing: 8,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          const SizedBox.square(
            dimension: 22,
            child: CircularProgressIndicator(strokeWidth: 2),
          ),
          TextButton(
            key: const ValueKey('studio-update-cancel'),
            onPressed: controller.cancelInstall,
            child: Text(context.l10n.settingsCancel),
          ),
        ],
      );
    }
    if (state is InstallerLaunchedUpdaterStateSnapshot) {
      return null;
    }
    if (_hasUpdate(state)) {
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
    if (state is DisabledUpdaterStateSnapshot) return null;
    return OutlinedButton(
      key: const ValueKey('studio-update-check'),
      onPressed: controller.check,
      child: Text(context.l10n.settingsStudioUpdateCheck),
    );
  }

  String _statusText(
    BuildContext context,
    UpdaterStateSnapshot state,
    String current,
  ) {
    return switch (state) {
      DisabledUpdaterStateSnapshot() =>
        context.l10n.settingsStudioUpdateDisabled(current),
      IdleUpdaterStateSnapshot() => context.l10n.settingsStudioUpdateCurrent(
        current,
      ),
      CheckingUpdaterStateSnapshot() =>
        context.l10n.settingsStudioUpdateChecking(current),
      UpToDateUpdaterStateSnapshot() => context.l10n.settingsStudioUpdateLatest(
        current,
      ),
      AvailableUpdaterStateSnapshot(:final update) =>
        context.l10n.settingsStudioUpdateAvailable(current, update.version),
      DownloadingUpdaterStateSnapshot(:final update) =>
        context.l10n.settingsStudioUpdateDownloading(
          update.version,
          _progressPercent(state),
        ),
      VerifyingUpdaterStateSnapshot(:final update) =>
        context.l10n.settingsStudioUpdateVerifying(update.version),
      InstallerLaunchedUpdaterStateSnapshot(:final update) =>
        context.l10n.settingsStudioUpdateInstallerLaunched(update.version),
      InstallFailedUpdaterStateSnapshot(:final update) =>
        context.l10n.settingsStudioUpdateAvailable(current, update.version),
      CheckFailedUpdaterStateSnapshot() =>
        context.l10n.settingsStudioUpdateCurrent(current),
    };
  }

  String _errorText(BuildContext context, UpdaterStateSnapshot state) {
    final error = _error(state);
    if (error?.code == 'runtimeBusy') {
      return context.l10n.settingsStudioUpdateBusy;
    }
    return context.l10n.settingsStudioUpdateFailed(error?.message ?? '');
  }

  int _progressPercent(DownloadingUpdaterStateSnapshot state) {
    return ((_progress(state) ?? 0) * 100).round();
  }
}

double? _progress(DownloadingUpdaterStateSnapshot state) {
  if (state.total <= 0) return null;
  return (state.downloaded / state.total).clamp(0, 1);
}

bool _hasUpdate(UpdaterStateSnapshot state) =>
    state.update != null && state is! InstallerLaunchedUpdaterStateSnapshot;

UpdaterErrorView? _error(UpdaterStateSnapshot state) => switch (state) {
  CheckFailedUpdaterStateSnapshot(:final error) ||
  InstallFailedUpdaterStateSnapshot(:final error) => error,
  DisabledUpdaterStateSnapshot() ||
  IdleUpdaterStateSnapshot() ||
  CheckingUpdaterStateSnapshot() ||
  UpToDateUpdaterStateSnapshot() ||
  AvailableUpdaterStateSnapshot() ||
  DownloadingUpdaterStateSnapshot() ||
  VerifyingUpdaterStateSnapshot() ||
  InstallerLaunchedUpdaterStateSnapshot() => null,
};
