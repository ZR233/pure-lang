import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';
import 'settings_remote_directory_dialog.dart';
import 'settings_ssh_server_dialog.dart';
import 'settings_ssh_server_row.dart';

class SshTab extends ConsumerStatefulWidget {
  const SshTab({super.key});

  @override
  ConsumerState<SshTab> createState() => _SshTabState();
}

class _SshTabState extends ConsumerState<SshTab> {
  List<SshServer>? _servers;
  final Map<String, SshConnectionView> _connections = {};
  final Map<String, SshServerOperation> _operations = {};
  final Map<String, String> _serverErrors = {};
  String? _error;

  @override
  void initState() {
    super.initState();
    unawaited(_reload());
  }

  Future<void> _reload() async {
    try {
      final servers = await ref.read(studioApiProvider).listSshServers();
      if (mounted) setState(() => _servers = servers);
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    }
  }

  @override
  Widget build(BuildContext context) {
    final servers = _servers;
    return SettingsPane(
      header: SettingsHeader(
        title: context.l10n.settingsSshTitle,
        subtitle: context.l10n.settingsSshSubtitle,
        trailing: FilledButton.tonalIcon(
          key: StudioDriverKeys.sshAddServer,
          onPressed: () => _editServer(),
          icon: const Icon(Icons.add, size: 18),
          label: Text(context.l10n.settingsSshAdd),
        ),
      ),
      children: [
        const SizedBox(height: 16),
        if (_error case final error?) ...[
          SettingsInlineError(message: error),
          const SizedBox(height: 12),
        ],
        if (servers == null)
          const LinearProgressIndicator()
        else if (servers.isEmpty)
          SettingsEmptyMessage(
            icon: Icons.dns_outlined,
            title: context.l10n.settingsSshEmpty,
            body: context.l10n.settingsSshManagedByCore,
          )
        else
          for (final server in servers) ...[
            SshServerRow(
              server: server,
              connection: _connections[server.id],
              operation: _operations[server.id],
              error: _serverErrors[server.id],
              onTest: () => _test(server),
              onReconnect: () => _reconnect(server),
              onOpen: () => _openWorkspace(server),
              onEdit: () => _runServerOperation(
                server,
                SshServerOperation.edit,
                () => _editServer(server),
              ),
              onDelete: () => _deleteServer(server),
            ),
            const SizedBox(height: 10),
          ],
      ],
    );
  }

  Future<void> _runServerOperation(
    SshServer server,
    SshServerOperation operation,
    Future<void> Function() action,
  ) async {
    // Reserve synchronously: callbacks from the current frame can still be invoked twice.
    if (_operations.containsKey(server.id)) return;
    setState(() {
      _operations[server.id] = operation;
      _serverErrors.remove(server.id);
    });
    try {
      await action();
    } catch (error) {
      if (mounted) setState(() => _serverErrors[server.id] = error.toString());
    } finally {
      if (mounted) setState(() => _operations.remove(server.id));
    }
  }

  Future<void> _test(SshServer server) =>
      _runServerOperation(server, SshServerOperation.test, () async {
        setState(() => _connections.remove(server.id));
        final snapshot = await ref
            .read(studioApiProvider)
            .testSshConnection(server.id);
        if (mounted) setState(() => _connections[server.id] = snapshot);
      });

  Future<void> _reconnect(SshServer server) =>
      _runServerOperation(server, SshServerOperation.reconnect, () async {
        setState(() => _connections.remove(server.id));
        final snapshot = await ref
            .read(studioApiProvider)
            .reconnectSshServer(server.id);
        if (mounted) setState(() => _connections[server.id] = snapshot);
      });

  Future<void> _editServer([SshServer? server]) async {
    final command = await showDialog<SaveSshServerCommand>(
      context: context,
      builder: (context) => SshServerDialog(server: server),
    );
    if (command == null || !mounted) return;
    setState(() => _error = null);
    try {
      await ref.read(studioApiProvider).saveSshServer(command);
      if (server != null && mounted) {
        setState(() => _connections.remove(server.id));
      }
      await _reload();
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    }
  }

  Future<void> _deleteServer(SshServer server) =>
      _runServerOperation(server, SshServerOperation.delete, () async {
        final confirmed = await showDialog<bool>(
          context: context,
          builder: (context) => AlertDialog(
            title: Text(context.l10n.settingsSshDeleteTitle),
            content: Text(context.l10n.settingsSshDeleteBody(server.name)),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context, false),
                child: Text(context.l10n.settingsCancel),
              ),
              FilledButton(
                onPressed: () => Navigator.pop(context, true),
                child: Text(context.l10n.settingsSshDelete),
              ),
            ],
          ),
        );
        if (confirmed != true || !mounted) return;
        await ref.read(studioApiProvider).deleteSshServer(server.id);
        if (mounted) setState(() => _connections.remove(server.id));
        await _reload();
      });

  Future<void> _openWorkspace(SshServer server) =>
      _runServerOperation(server, SshServerOperation.open, () async {
        await showDialog<String>(
          context: context,
          builder: (context) => RemoteDirectoryDialog(server: server),
        );
      });
}
