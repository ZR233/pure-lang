import 'package:flutter/material.dart';

import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';

class SshServerDialog extends StatefulWidget {
  const SshServerDialog({super.key, this.server});

  final SshServer? server;

  @override
  State<SshServerDialog> createState() => _SshServerDialogState();
}

class _SshServerDialogState extends State<SshServerDialog> {
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
    return SettingsFormDialog(
      key: StudioDriverKeys.sshServerDialog,
      title: Text(
        widget.server == null
            ? context.l10n.settingsSshAdd
            : context.l10n.settingsSshEdit,
      ),
      content: SizedBox(
        width: 480,
        child: SingleChildScrollView(
          child: SettingsFieldStack(
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
