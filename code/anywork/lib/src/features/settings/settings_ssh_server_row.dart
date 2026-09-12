import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';

enum SshServerOperation { test, reconnect, open, edit, delete }

class SshServerRow extends StatelessWidget {
  const SshServerRow({
    super.key,
    required this.server,
    required this.connection,
    required this.operation,
    this.error,
    required this.onTest,
    required this.onReconnect,
    required this.onOpen,
    required this.onEdit,
    required this.onDelete,
  });

  final SshServer server;
  final SshConnectionView? connection;
  final SshServerOperation? operation;
  final String? error;
  final VoidCallback onTest;
  final VoidCallback onReconnect;
  final VoidCallback onOpen;
  final VoidCallback onEdit;
  final VoidCallback onDelete;

  @override
  Widget build(BuildContext context) {
    final ready = connection?.state == 'ready';
    final busy = operation != null;
    return SettingsResourceRow(
      title: server.name,
      icon: Icons.dns_outlined,
      status: _ConnectionChip(
        key: ready ? StudioDriverKeys.sshReady(server.id) : null,
        connection: connection,
      ),
      children: [
        Text(
          '${server.username}@${server.host}:${server.port}',
          style: context.text.bodyMedium?.copyWith(
            color: context.colors.onSurface,
            fontFamily: 'monospace',
          ),
        ),
        const SizedBox(height: 6),
        Text(
          ready
              ? '${connection!.architecture} · helper ${connection!.helperVersion}'
              : context.l10n.settingsSshManagedByCore,
          style: context.text.bodySmall?.copyWith(
            color: context.colors.onSurfaceVariant,
          ),
        ),
        const SizedBox(height: 6),
        Text(
          context.l10n.settingsSshReconnectHint,
          style: context.text.bodySmall,
        ),
        if ((error ?? connection?.errorMessage) case final message?)
          SettingsInlineError(message: message),
        const SizedBox(height: 14),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            TextButton.icon(
              key: StudioDriverKeys.sshTest(server.id),
              onPressed: busy ? null : onTest,
              icon: operation == SshServerOperation.test
                  ? const SizedBox.square(
                      dimension: 15,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.cable_outlined, size: 17),
              label: Text(context.l10n.settingsSshTest),
            ),
            TextButton.icon(
              key: StudioDriverKeys.sshOpen(server.id),
              onPressed: busy ? null : onOpen,
              icon: const Icon(Icons.folder_open_outlined, size: 17),
              label: Text(context.l10n.settingsSshOpenProject),
            ),
            TextButton.icon(
              key: StudioDriverKeys.sshReconnect(server.id),
              onPressed: busy ? null : onReconnect,
              icon: operation == SshServerOperation.reconnect
                  ? const SizedBox.square(
                      dimension: 15,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.refresh, size: 17),
              label: Text(context.l10n.settingsSshReconnect),
            ),
            TextButton(
              onPressed: busy ? null : onEdit,
              child: Text(context.l10n.settingsSshEdit),
            ),
            TextButton(
              onPressed: busy ? null : onDelete,
              child: Text(context.l10n.settingsSshDelete),
            ),
          ],
        ),
      ],
    );
  }
}

class _ConnectionChip extends StatelessWidget {
  const _ConnectionChip({super.key, required this.connection});

  final SshConnectionView? connection;

  @override
  Widget build(BuildContext context) {
    final state = connection?.state ?? 'disconnected';
    final ready = state == 'ready';
    return SettingsMiniMeta(
      icon: ready ? Icons.check_circle_outline : Icons.circle_outlined,
      label: ready ? context.l10n.settingsSshReady : state,
    );
  }
}
