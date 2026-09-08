import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';

class McpTab extends ConsumerStatefulWidget {
  const McpTab({super.key, required this.settingsServers, required this.state});

  final List<McpServerSettingsView> settingsServers;
  final McpStateSnapshot state;

  @override
  ConsumerState<McpTab> createState() => _McpTabState();
}

class _McpTabState extends ConsumerState<McpTab> {
  final Map<String, bool> _enabledByServer = {};
  final Map<String, String> _endpointByServer = {};
  Timer? _saveTimer;
  String? _error;

  @override
  void didUpdateWidget(covariant McpTab oldWidget) {
    super.didUpdateWidget(oldWidget);
    final serversById = {
      for (final server in widget.settingsServers) server.id: server,
    };
    _enabledByServer.removeWhere(
      (id, enabled) =>
          serversById[id] == null || serversById[id]!.enabled == enabled,
    );
    _endpointByServer.removeWhere(
      (id, endpoint) =>
          serversById[id] == null || serversById[id]!.endpoint == endpoint,
    );
  }

  @override
  void dispose() {
    _saveTimer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return SettingsPane(
      header: SettingsHeader(
        title: context.l10n.settingsMcpTitle,
        subtitle: context.l10n.settingsMcpSubtitle,
        trailing: Wrap(
          spacing: 8,
          children: [
            TextButton.icon(
              key: StudioDriverKeys.mcpRefresh,
              onPressed: () => unawaited(_run(_refresh)),
              icon: const Icon(Icons.refresh),
              label: Text(context.l10n.settingsMcpRefresh),
            ),
            TextButton.icon(
              key: StudioDriverKeys.mcpResetAll,
              onPressed: widget.state.servers.isEmpty
                  ? null
                  : () => unawaited(_confirmResetAll()),
              icon: const Icon(Icons.restart_alt),
              label: Text(context.l10n.settingsMcpResetAll),
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
                _McpSettingsRow(
                  server: server,
                  enabled: _enabledByServer[server.id] ?? server.enabled,
                  onReconnect: () => unawaited(
                    _run(
                      () => ref
                          .read(studioControllerProvider.notifier)
                          .resetMcpServer(server.id),
                    ),
                  ),
                  onEnabledChanged: (value) {
                    setState(() => _enabledByServer[server.id] = value);
                    unawaited(_save());
                  },
                  onEndpointChanged: server.hasLockedIdentity
                      ? null
                      : (value) => setState(() {
                          _endpointByServer[server.id] = value;
                          _scheduleSave();
                        }),
                ),
            ],
          ),
        if (widget.state.servers.isEmpty)
          SettingsEmptyMessage(
            icon: Icons.hub_outlined,
            title: context.l10n.settingsMcpEmptyTitle,
            body: context.l10n.settingsMcpEmptyMessage,
          ),
        if (_error != null) SettingsInlineError(message: _error!),
      ],
    );
  }

  Future<void> _refresh() {
    return ref.read(studioControllerProvider.notifier).refreshMcpState();
  }

  Future<void> _run(Future<void> Function() operation) async {
    try {
      setState(() => _error = null);
      await operation();
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    }
  }

  Future<void> _confirmResetAll() async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(context.l10n.settingsMcpResetConfirmTitle),
        content: Text(context.l10n.settingsMcpResetConfirmBody),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(false),
            child: Text(context.l10n.settingsCancel),
          ),
          FilledButton(
            key: StudioDriverKeys.mcpResetAllConfirm,
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(context.l10n.settingsMcpResetConfirmAction),
          ),
        ],
      ),
    );
    if (confirmed == true && mounted) {
      await _run(
        () => ref.read(studioControllerProvider.notifier).resetAllMcp(),
      );
    }
  }

  void _scheduleSave() {
    _saveTimer?.cancel();
    _saveTimer = Timer(const Duration(milliseconds: 650), () {
      unawaited(_save());
    });
  }

  Future<void> _save() async {
    try {
      setState(() => _error = null);
      await ref
          .read(studioControllerProvider.notifier)
          .saveMcpSettings(
            McpSettingsCommand(
              servers: [
                for (final server in widget.settingsServers)
                  McpServerCommand(
                    id: server.id,
                    enabled: _enabledByServer[server.id] ?? server.enabled,
                    transport: server.transport,
                    endpoint: server.hasLockedIdentity
                        ? server.endpoint
                        : _endpointByServer[server.id] ?? server.endpoint,
                  ),
              ],
            ),
          );
    } catch (error) {
      if (mounted) {
        setState(() => _error = error.toString());
      }
    }
  }
}

class _McpSettingsRow extends StatelessWidget {
  const _McpSettingsRow({
    required this.server,
    required this.enabled,
    required this.onEnabledChanged,
    required this.onEndpointChanged,
    required this.onReconnect,
  });

  final McpServerSettingsView server;
  final bool enabled;
  final ValueChanged<bool> onEnabledChanged;
  final ValueChanged<String>? onEndpointChanged;
  final VoidCallback onReconnect;

  @override
  Widget build(BuildContext context) {
    final unavailable = switch (server.state) {
      McpUnavailableState() => server.state as McpUnavailableState,
      McpDisabledState() ||
      McpMissingCredentialState() ||
      McpCheckingState() ||
      McpAvailableState() => null,
    };
    return SettingsResourceRow(
      key: StudioDriverKeys.mcpServerRow(server.id),
      title: server.id,
      icon: Icons.hub_outlined,
      actions: [
        Switch(value: enabled, onChanged: onEnabledChanged),
        const SizedBox(width: 8),
        TextButton.icon(
          key: StudioDriverKeys.mcpResetServer(server.id),
          onPressed: server.enabled ? onReconnect : null,
          icon: const Icon(Icons.sync),
          label: Text(context.l10n.settingsMcpReconnect),
        ),
      ],
      children: [
        Wrap(
          spacing: 8,
          runSpacing: 6,
          children: [
            SettingsInfoPill(icon: Icons.hub_outlined, label: server.transport),
            SettingsInfoPill(
              icon: Icons.circle_outlined,
              label: _mcpAvailabilityLabel(context, server.state),
            ),
          ],
        ),
        if (unavailable != null) ...[
          const SizedBox(height: 8),
          Text(
            unavailable.message,
            key: StudioDriverKeys.mcpServerError(server.id),
            style: context.text.bodySmall?.copyWith(
              color: context.colors.error,
            ),
          ),
        ],
        const SizedBox(height: 9),
        TextFormField(
          key: ValueKey(
            '${server.id}:${server.endpoint}:${server.mutationPolicy}',
          ),
          initialValue: server.endpoint,
          readOnly: server.hasLockedIdentity,
          decoration: InputDecoration(
            labelText: context.l10n.settingsEndpoint,
            isDense: true,
          ),
          onChanged: onEndpointChanged,
        ),

        const Divider(height: 24),
      ],
    );
  }
}

String _mcpAvailabilityLabel(BuildContext context, McpServerState state) =>
    switch (state) {
      McpDisabledState() => context.l10n.settingsStateDisabled,
      McpMissingCredentialState() =>
        context.l10n.settingsMcpStateMissingCredential,
      McpCheckingState() => context.l10n.settingsStateChecking,
      McpAvailableState() => context.l10n.settingsStateAvailable,
      McpUnavailableState() => context.l10n.settingsStateUnavailable,
    };
