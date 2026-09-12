import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';

class LspTab extends ConsumerStatefulWidget {
  const LspTab({super.key, required this.projectId, required this.state});

  final String? projectId;
  final LspStateSnapshot state;

  @override
  ConsumerState<LspTab> createState() => _LspTabState();
}

class _LspTabState extends ConsumerState<LspTab> {
  String? _error;

  @override
  Widget build(BuildContext context) {
    return SettingsPane(
      header: SettingsHeader(
        title: context.l10n.settingsLspTitle,
        subtitle: context.l10n.settingsLspSubtitle,
        trailing: Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            TextButton.icon(
              key: StudioDriverKeys.lspRefresh,
              onPressed: () => unawaited(_run(_refresh)),
              icon: const Icon(Icons.refresh),
              label: Text(context.l10n.settingsLspRefresh),
            ),
            TextButton.icon(
              key: StudioDriverKeys.lspProbe,
              onPressed: widget.projectId == null
                  ? null
                  : () => unawaited(_run(_probe)),
              icon: const Icon(Icons.monitor_heart_outlined),
              label: Text(context.l10n.settingsLspProbe),
            ),
            TextButton.icon(
              key: StudioDriverKeys.lspResetWorkspace,
              onPressed:
                  widget.projectId == null || widget.state.servers.isEmpty
                  ? null
                  : () => unawaited(_run(_resetWorkspace)),
              icon: const Icon(Icons.restart_alt),
              label: Text(context.l10n.settingsLspResetWorkspace),
            ),
          ],
        ),
      ),
      children: [
        const SizedBox(height: 16),
        if (widget.state.servers.isNotEmpty)
          SettingsGroup(
            children: [
              for (final server in widget.state.servers)
                _LspSettingsRow(
                  server: server,
                  onRepair: switch (server.state) {
                    LspUnavailableState(code: 'lspComponentMissing') =>
                      () => unawaited(_run(() => _repair(server.id))),
                    LspCheckingState() ||
                    LspAvailableState() ||
                    LspUnavailableState() ||
                    LspDisabledState() => null,
                  },
                  onReset: widget.projectId == null
                      ? null
                      : () => unawaited(_run(() => _resetServer(server.id))),
                ),
            ],
          )
        else
          SettingsEmptyMessage(
            icon: Icons.code_outlined,
            title: context.l10n.settingsLspEmptyTitle,
            body: context.l10n.settingsLspEmptyMessage,
          ),
        if (_error != null) ...[
          const SizedBox(height: 12),
          SettingsInlineError(message: _error!),
        ],
      ],
    );
  }

  Future<void> _refresh() {
    return ref.read(studioControllerProvider.notifier).refreshLspState();
  }

  Future<void> _probe() {
    return ref.read(studioControllerProvider.notifier).probeLspServer();
  }

  Future<void> _repair(String serverId) {
    return ref
        .read(studioControllerProvider.notifier)
        .repairLspServer(serverId);
  }

  Future<void> _resetServer(String serverId) {
    return ref.read(studioControllerProvider.notifier).resetLspServer(serverId);
  }

  Future<void> _resetWorkspace() {
    return ref.read(studioControllerProvider.notifier).resetLspWorkspace();
  }

  Future<void> _run(Future<void> Function() operation) async {
    try {
      setState(() => _error = null);
      await operation();
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    }
  }
}

class _LspSettingsRow extends StatelessWidget {
  const _LspSettingsRow({
    required this.server,
    required this.onRepair,
    required this.onReset,
  });

  final LspServerStateView server;
  final VoidCallback? onRepair;
  final VoidCallback? onReset;

  @override
  Widget build(BuildContext context) {
    return SettingsResourceRow(
      title: server.displayName,
      icon: Icons.code_outlined,
      actions: [
        if (onRepair != null)
          TextButton.icon(
            key: StudioDriverKeys.lspRepairServer(server.id),
            onPressed: onRepair,
            icon: const Icon(Icons.build_outlined),
            label: Text(context.l10n.settingsLspRepair),
          ),
        const SizedBox(width: 8),
        TextButton.icon(
          key: StudioDriverKeys.lspResetServer(server.id),
          onPressed: onReset,
          icon: const Icon(Icons.restart_alt),
          label: Text(context.l10n.settingsLspReset),
        ),
      ],
      children: [
        const SizedBox(height: 8),
        Wrap(
          spacing: 8,
          runSpacing: 6,
          children: [
            SettingsInfoPill(
              icon: Icons.circle_outlined,
              label: _lspAvailabilityLabel(context, server.state),
            ),
            if (server.state case LspAvailableState(:final diagnosticCount))
              SettingsInfoPill(
                icon: Icons.rule_outlined,
                label: diagnosticCount.toString(),
              ),
            if (_activeLspActivity(server) case final activity?)
              SettingsInfoPill(
                icon: activity is LspIndexingActivity
                    ? Icons.manage_search_outlined
                    : Icons.sync_outlined,
                label: _lspActivityPillLabel(context, activity),
              ),
          ],
        ),
        if (_lspActivityDetail(server) case final activityDetail?) ...[
          const SizedBox(height: 8),
          Text(activityDetail, style: context.text.bodySmall),
        ],
        if (_lspInformationalMessage(server.state) case final message?) ...[
          const SizedBox(height: 8),
          Text(message, style: context.text.bodySmall),
        ],
        if (server.state case LspUnavailableState(message: final error)) ...[
          const SizedBox(height: 8),
          Text(
            error,
            style: context.text.bodySmall?.copyWith(
              color: context.colors.error,
            ),
          ),
        ],

        const Divider(height: 24),
      ],
    );
  }
}

String _lspAvailabilityLabel(BuildContext context, LspServerState state) =>
    switch (state) {
      LspCheckingState() => context.l10n.settingsStateChecking,
      LspAvailableState() => context.l10n.settingsStateAvailable,
      LspUnavailableState() => context.l10n.settingsStateUnavailable,
      LspDisabledState() => context.l10n.settingsStateDisabled,
    };

String? _lspInformationalMessage(LspServerState state) => switch (state) {
  LspCheckingState(:final message) ||
  LspDisabledState(:final message) => message,
  LspAvailableState() || LspUnavailableState() => null,
};

LspActivity? _activeLspActivity(LspServerStateView server) {
  return switch (server.state) {
    LspAvailableState(activity: final activity)
        when activity is! LspIdleActivity =>
      activity,
    LspAvailableState() ||
    LspCheckingState() ||
    LspUnavailableState() ||
    LspDisabledState() => null,
  };
}

String _lspActivityPillLabel(BuildContext context, LspActivity activity) {
  final label = switch (activity) {
    LspIndexingActivity() => context.l10n.settingsLspActivityIndexing,
    LspBusyActivity() => context.l10n.settingsLspActivityBusy,
    LspIdleActivity() => context.l10n.settingsLspActivityIdle,
  };
  final percentage = switch (activity) {
    LspBusyActivity(:final percentage) ||
    LspIndexingActivity(:final percentage) => percentage,
    LspIdleActivity() => null,
  };
  return percentage == null
      ? label
      : '$label · ${context.l10n.statusLspActivityPercentage(percentage)}';
}

String? _lspActivityDetail(LspServerStateView server) {
  final activity = _activeLspActivity(server);
  final parts = [
    if (activity
        case LspBusyActivity(title: final title?) ||
            LspIndexingActivity(title: final title?))
      if (title.isNotEmpty) title,
    if (activity
        case LspBusyActivity(message: final message?) ||
            LspIndexingActivity(message: final message?))
      if (message.isNotEmpty) message,
  ];
  return parts.isEmpty ? null : parts.join(' · ');
}
