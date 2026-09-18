import 'package:flutter/material.dart';

import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';

class SshServerDialog extends StatefulWidget {
  const SshServerDialog({super.key, this.server, this.onSave, this.onBack});

  final SshServer? server;
  final Future<void> Function(SaveSshServerCommand)? onSave;
  final VoidCallback? onBack;

  @override
  State<SshServerDialog> createState() => _SshServerDialogState();
}

class _SshServerDialogState extends State<SshServerDialog> {
  late final TextEditingController _alias;
  late final TextEditingController _host;
  late final TextEditingController _port;
  late final TextEditingController _username;
  late final TextEditingController _identity;
  String? _validationError;
  bool _saving = false;

  @override
  void initState() {
    super.initState();
    final server = widget.server;
    _alias = TextEditingController(text: server?.alias ?? '');
    _host = TextEditingController(text: server?.hostName ?? '');
    _port = TextEditingController(text: '${server?.port ?? 22}');
    _username = TextEditingController(text: server?.username ?? '');
    _identity = TextEditingController(text: server?.identityFile ?? '');
  }

  @override
  void dispose() {
    _alias.dispose();
    _host.dispose();
    _port.dispose();
    _username.dispose();
    _identity.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final editing = widget.server != null;
    return PopScope(
      canPop: !_saving,
      child: SettingsFormDialog(
        key: StudioDriverKeys.sshServerDialog,
        title: Text(
          editing ? context.l10n.settingsSshEdit : context.l10n.settingsSshAdd,
        ),
        content: SizedBox(
          width: 480,
          child: SingleChildScrollView(
            child: SettingsFieldStack(
              children: [
                TextField(
                  key: StudioDriverKeys.sshServerAliasInput,
                  controller: _alias,
                  enabled: !editing,
                  decoration: InputDecoration(
                    labelText: context.l10n.settingsSshAlias,
                    helperText: context.l10n.settingsSshAliasHelper,
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
                TextField(
                  key: StudioDriverKeys.sshServerIdentityInput,
                  controller: _identity,
                  decoration: InputDecoration(
                    labelText: context.l10n.settingsSshIdentityFile,
                    helperText: context.l10n.settingsSshIdentityHelper,
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
            onPressed: _saving
                ? null
                : widget.onBack ?? () => Navigator.pop(context),
            child: Text(
              widget.onBack == null
                  ? context.l10n.settingsCancel
                  : context.l10n.sidebarBack,
            ),
          ),
          FilledButton(
            key: StudioDriverKeys.sshServerSave,
            onPressed: _saving
                ? null
                : () async {
                    final port = int.tryParse(_port.text);
                    final alias = _alias.text.trim();
                    if (alias.isEmpty) {
                      setState(
                        () => _validationError =
                            context.l10n.settingsSshAliasRequired,
                      );
                      return;
                    }
                    if (alias.contains(RegExp("[\\s*?\\[\\]!#\"'\\\\]")) ||
                        alias.startsWith('-')) {
                      setState(
                        () => _validationError =
                            context.l10n.settingsSshAliasInvalid,
                      );
                      return;
                    }
                    if (_host.text.trim().isEmpty) {
                      setState(
                        () => _validationError =
                            context.l10n.settingsSshHostRequired,
                      );
                      return;
                    }
                    if (_username.text.trim().isEmpty) {
                      setState(
                        () => _validationError =
                            context.l10n.settingsSshUsernameRequired,
                      );
                      return;
                    }
                    if (port == null || port <= 0 || port > 65535) {
                      setState(
                        () => _validationError =
                            context.l10n.settingsSshPortInvalid,
                      );
                      return;
                    }
                    setState(() => _validationError = null);
                    final command = SaveSshServerCommand(
                      alias: alias,
                      hostName: _host.text.trim(),
                      port: port,
                      username: _username.text.trim(),
                      identityFile: _identity.text.trim().isEmpty
                          ? null
                          : _identity.text.trim(),
                    );
                    if (widget.onSave == null) {
                      Navigator.pop(context, command);
                      return;
                    }
                    setState(() => _saving = true);
                    try {
                      await widget.onSave!(command);
                    } catch (error) {
                      if (mounted) {
                        setState(() => _validationError = error.toString());
                      }
                    } finally {
                      if (mounted) setState(() => _saving = false);
                    }
                  },
            child: Text(
              _saving
                  ? context.l10n.sidebarLoadingMore
                  : widget.onSave == null
                  ? context.l10n.settingsSshSave
                  : context.l10n.sidebarSaveConnect,
            ),
          ),
        ],
      ),
    );
  }
}
