import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';

class SshTab extends ConsumerStatefulWidget {
  const SshTab({super.key});

  @override
  ConsumerState<SshTab> createState() => _SshTabState();
}

class _SshTabState extends ConsumerState<SshTab> {
  List<SshServer>? _servers;
  final Map<String, SshConnectionView> _connections = {};
  String? _busyServerId;
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
      children: [
        SettingsHeader(
          title: context.l10n.settingsSshTitle,
          subtitle: context.l10n.settingsSshSubtitle,
          trailing: FilledButton.tonalIcon(
            key: StudioDriverKeys.sshAddServer,
            onPressed: () => _editServer(),
            icon: const Icon(Icons.add, size: 18),
            label: Text(context.l10n.settingsSshAdd),
          ),
        ),
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
            _SshServerCard(
              server: server,
              connection: _connections[server.id],
              busy: _busyServerId == server.id,
              onTest: () => _test(server),
              onReconnect: () => _reconnect(server),
              onOpen: () => _openWorkspace(server),
              onEdit: () => _editServer(server),
              onDelete: () => _deleteServer(server),
            ),
            const SizedBox(height: 10),
          ],
      ],
    );
  }

  Future<void> _test(SshServer server) async {
    setState(() {
      _busyServerId = server.id;
      _error = null;
    });
    try {
      final snapshot = await ref
          .read(studioApiProvider)
          .testSshConnection(server.id);
      if (mounted) setState(() => _connections[server.id] = snapshot);
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    } finally {
      if (mounted) setState(() => _busyServerId = null);
    }
  }

  Future<void> _reconnect(SshServer server) async {
    setState(() {
      _busyServerId = server.id;
      _error = null;
    });
    try {
      final snapshot = await ref
          .read(studioApiProvider)
          .reconnectSshServer(server.id);
      if (mounted) setState(() => _connections[server.id] = snapshot);
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    } finally {
      if (mounted) setState(() => _busyServerId = null);
    }
  }

  Future<void> _editServer([SshServer? server]) async {
    final command = await showDialog<SaveSshServerCommand>(
      context: context,
      builder: (context) => _SshServerDialog(server: server),
    );
    if (command == null) return;
    setState(() => _error = null);
    try {
      await ref.read(studioApiProvider).saveSshServer(command);
      await _reload();
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    }
  }

  Future<void> _deleteServer(SshServer server) async {
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
    if (confirmed != true) return;
    try {
      await ref.read(studioApiProvider).deleteSshServer(server.id);
      await _reload();
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    }
  }

  Future<void> _openWorkspace(SshServer server) async {
    final path = await showDialog<String>(
      context: context,
      builder: (context) => _RemoteDirectoryDialog(server: server),
    );
    if (path == null || !mounted) return;
    setState(() => _busyServerId = server.id);
    try {
      await ref
          .read(studioControllerProvider.notifier)
          .openRemoteProject(server.id, path);
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    } finally {
      if (mounted) setState(() => _busyServerId = null);
    }
  }
}

class _SshServerCard extends StatelessWidget {
  const _SshServerCard({
    required this.server,
    required this.connection,
    required this.busy,
    required this.onTest,
    required this.onReconnect,
    required this.onOpen,
    required this.onEdit,
    required this.onDelete,
  });

  final SshServer server;
  final SshConnectionView? connection;
  final bool busy;
  final VoidCallback onTest;
  final VoidCallback onReconnect;
  final VoidCallback onOpen;
  final VoidCallback onEdit;
  final VoidCallback onDelete;

  @override
  Widget build(BuildContext context) {
    final ready = connection?.state == 'ready';
    return SettingsSectionPanel(
      title: server.name,
      trailing: _ConnectionChip(connection: connection),
      children: [
        Text(
          '${server.username}@${server.host}:${server.port}',
          style: context.text.bodyMedium?.copyWith(
            color: context.studioInk,
            fontFamily: 'monospace',
          ),
        ),
        const SizedBox(height: 6),
        Text(
          ready
              ? '${connection!.architecture} · helper ${connection!.helperVersion}'
              : context.l10n.settingsSshManagedByCore,
          style: context.text.bodySmall?.copyWith(color: context.studioInkSoft),
        ),
        const SizedBox(height: 14),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            FilledButton.tonalIcon(
              key: StudioDriverKeys.sshTest(server.id),
              onPressed: busy ? null : onTest,
              icon: busy
                  ? const SizedBox.square(
                      dimension: 15,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.cable_outlined, size: 17),
              label: Text(context.l10n.settingsSshTest),
            ),
            OutlinedButton.icon(
              key: StudioDriverKeys.sshOpen(server.id),
              onPressed: busy ? null : onOpen,
              icon: const Icon(Icons.folder_open_outlined, size: 17),
              label: Text(context.l10n.settingsSshOpenProject),
            ),
            if (ready)
              OutlinedButton.icon(
                key: StudioDriverKeys.sshReconnect(server.id),
                onPressed: busy ? null : onReconnect,
                icon: const Icon(Icons.refresh, size: 17),
                label: Text(context.l10n.settingsSshReconnect),
              ),
            TextButton(
              onPressed: onEdit,
              child: Text(context.l10n.settingsSshEdit),
            ),
            TextButton(
              onPressed: onDelete,
              child: Text(context.l10n.settingsSshDelete),
            ),
          ],
        ),
      ],
    );
  }
}

class _ConnectionChip extends StatelessWidget {
  const _ConnectionChip({required this.connection});

  final SshConnectionView? connection;

  @override
  Widget build(BuildContext context) {
    final state = connection?.state ?? 'disconnected';
    final ready = state == 'ready';
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 5),
      decoration: BoxDecoration(
        color: ready
            ? Colors.green.withValues(alpha: 0.1)
            : context.studioPaper2,
        borderRadius: BorderRadius.circular(99),
        border: Border.all(color: context.studioLine),
      ),
      child: Text(
        ready ? context.l10n.settingsSshReady : state,
        style: context.text.labelSmall?.copyWith(
          color: ready ? Colors.green.shade700 : context.studioInkSoft,
        ),
      ),
    );
  }
}

class _SshServerDialog extends StatefulWidget {
  const _SshServerDialog({this.server});

  final SshServer? server;

  @override
  State<_SshServerDialog> createState() => _SshServerDialogState();
}

class _SshServerDialogState extends State<_SshServerDialog> {
  late final TextEditingController _name;
  late final TextEditingController _host;
  late final TextEditingController _port;
  late final TextEditingController _username;
  late final TextEditingController _identity;
  late final TextEditingController _password;
  late SshAuthKind _authKind;
  String? _validationError;

  @override
  void initState() {
    super.initState();
    final server = widget.server;
    _name = TextEditingController(text: server?.name ?? '');
    _host = TextEditingController(text: server?.host ?? '');
    _port = TextEditingController(text: '${server?.port ?? 22}');
    _username = TextEditingController(text: server?.username ?? '');
    _identity = TextEditingController(text: server?.identityFile ?? '');
    _password = TextEditingController();
    _authKind = server?.authKind ?? SshAuthKind.agentOrKey;
  }

  @override
  void dispose() {
    _name.dispose();
    _host.dispose();
    _port.dispose();
    _username.dispose();
    _identity.dispose();
    _password.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      key: StudioDriverKeys.sshServerDialog,
      title: Text(
        widget.server == null
            ? context.l10n.settingsSshAdd
            : context.l10n.settingsSshEdit,
      ),
      content: SizedBox(
        width: 480,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                key: StudioDriverKeys.sshServerNameInput,
                controller: _name,
                decoration: InputDecoration(
                  labelText: context.l10n.settingsSshName,
                ),
              ),
              TextField(
                key: StudioDriverKeys.sshServerHostInput,
                controller: _host,
                decoration: InputDecoration(
                  labelText: context.l10n.settingsSshHost,
                ),
              ),
              Row(
                children: [
                  Expanded(
                    child: TextField(
                      key: StudioDriverKeys.sshServerUsernameInput,
                      controller: _username,
                      decoration: InputDecoration(
                        labelText: context.l10n.settingsSshUsername,
                      ),
                    ),
                  ),
                  const SizedBox(width: 12),
                  SizedBox(
                    width: 100,
                    child: TextField(
                      key: StudioDriverKeys.sshServerPortInput,
                      controller: _port,
                      keyboardType: TextInputType.number,
                      decoration: InputDecoration(
                        labelText: context.l10n.settingsSshPort,
                      ),
                    ),
                  ),
                ],
              ),
              DropdownButtonFormField<SshAuthKind>(
                key: StudioDriverKeys.sshServerAuthInput,
                initialValue: _authKind,
                decoration: InputDecoration(
                  labelText: context.l10n.settingsSshAuth,
                ),
                items: [
                  DropdownMenuItem(
                    value: SshAuthKind.agentOrKey,
                    child: Text(context.l10n.settingsSshAuthAgentOrKey),
                  ),
                  DropdownMenuItem(
                    value: SshAuthKind.password,
                    child: Text(context.l10n.settingsSshAuthPassword),
                  ),
                ],
                onChanged: (value) {
                  if (value != null) setState(() => _authKind = value);
                },
              ),
              if (_authKind == SshAuthKind.agentOrKey)
                TextField(
                  key: StudioDriverKeys.sshServerIdentityInput,
                  controller: _identity,
                  decoration: InputDecoration(
                    labelText: context.l10n.settingsSshIdentityFile,
                  ),
                )
              else
                TextField(
                  key: StudioDriverKeys.sshServerPasswordInput,
                  controller: _password,
                  obscureText: true,
                  autocorrect: false,
                  enableSuggestions: false,
                  decoration: InputDecoration(
                    labelText: context.l10n.settingsSshPassword,
                    helperText: context.l10n.settingsSshPasswordLease,
                  ),
                ),
              if (_validationError case final error?)
                Align(
                  alignment: Alignment.centerLeft,
                  child: Padding(
                    padding: const EdgeInsets.only(top: 12),
                    child: Text(
                      error,
                      key: StudioDriverKeys.sshServerValidationError,
                      style: TextStyle(
                        color: Theme.of(context).colorScheme.error,
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: Text(context.l10n.settingsCancel),
        ),
        FilledButton(
          key: StudioDriverKeys.sshServerSave,
          onPressed: () {
            final port = int.tryParse(_port.text);
            if (_name.text.trim().isEmpty) {
              setState(
                () => _validationError = context.l10n.settingsSshNameRequired,
              );
              return;
            }
            if (_host.text.trim().isEmpty) {
              setState(
                () => _validationError = context.l10n.settingsSshHostRequired,
              );
              return;
            }
            if (_username.text.trim().isEmpty) {
              setState(
                () =>
                    _validationError = context.l10n.settingsSshUsernameRequired,
              );
              return;
            }
            if (port == null || port <= 0 || port > 65535) {
              setState(
                () => _validationError = context.l10n.settingsSshPortInvalid,
              );
              return;
            }
            setState(() => _validationError = null);
            Navigator.pop(
              context,
              SaveSshServerCommand(
                id: widget.server?.id,
                name: _name.text.trim(),
                host: _host.text.trim(),
                port: port,
                username: _username.text.trim(),
                authKind: _authKind,
                identityFile:
                    _authKind == SshAuthKind.agentOrKey &&
                        _identity.text.trim().isNotEmpty
                    ? _identity.text.trim()
                    : null,
                password:
                    _authKind == SshAuthKind.password &&
                        _password.text.isNotEmpty
                    ? _password.text
                    : null,
              ),
            );
          },
          child: Text(context.l10n.settingsSshSave),
        ),
      ],
    );
  }
}

class _RemoteDirectoryDialog extends ConsumerStatefulWidget {
  const _RemoteDirectoryDialog({required this.server});

  final SshServer server;

  @override
  ConsumerState<_RemoteDirectoryDialog> createState() =>
      _RemoteDirectoryDialogState();
}

class _RemoteDirectoryDialogState
    extends ConsumerState<_RemoteDirectoryDialog> {
  RemoteDirectoryListing? _listing;
  String? _error;

  @override
  void initState() {
    super.initState();
    unawaited(_load(null));
  }

  Future<void> _load(String? path) async {
    setState(() {
      _listing = null;
      _error = null;
    });
    try {
      final listing = await ref
          .read(studioApiProvider)
          .browseRemoteDirectories(widget.server.id, path: path);
      if (mounted) setState(() => _listing = listing);
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    }
  }

  @override
  Widget build(BuildContext context) {
    final listing = _listing;
    return AlertDialog(
      key: StudioDriverKeys.sshDirectoryDialog,
      title: Text(context.l10n.settingsSshChooseDirectory),
      content: SizedBox(
        width: 560,
        height: 420,
        child: Column(
          children: [
            Row(
              children: [
                IconButton(
                  onPressed: listing?.parent == null
                      ? null
                      : () => _load(listing!.parent),
                  icon: const Icon(Icons.arrow_upward),
                ),
                Expanded(
                  child: SelectableText(
                    listing?.path ?? '…',
                    key: listing == null
                        ? null
                        : StudioDriverKeys.sshDirectoryCurrent(listing.path),
                  ),
                ),
              ],
            ),
            const Divider(),
            Expanded(
              child: _error != null
                  ? SettingsInlineError(message: _error!)
                  : listing == null
                  ? const Center(child: CircularProgressIndicator())
                  : ListView(
                      key: StudioDriverKeys.sshDirectoryList,
                      children: [
                        for (final entry in listing.entries)
                          ListTile(
                            key: StudioDriverKeys.sshDirectoryEntry(entry.path),
                            leading: const Icon(Icons.folder_outlined),
                            title: Text(entry.name),
                            onTap: () => _load(entry.path),
                          ),
                      ],
                    ),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: Text(context.l10n.settingsCancel),
        ),
        FilledButton.icon(
          key: StudioDriverKeys.sshOpenCurrentDirectory,
          onPressed: listing == null
              ? null
              : () => Navigator.pop(context, listing.path),
          icon: const Icon(Icons.folder_open_outlined, size: 17),
          label: Text(context.l10n.settingsSshOpenThisDirectory),
        ),
      ],
    );
  }
}
