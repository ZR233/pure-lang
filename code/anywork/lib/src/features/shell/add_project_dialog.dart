part of 'studio_shell.dart';

Future<void> showAddProjectDialog(BuildContext context) => showDialog<void>(
  context: context,
  barrierDismissible: false,
  builder: (_) => const _AddProjectDialog(),
);

enum _AddProjectStep { location, connections, configuration, directory }

class _AddProjectDialog extends ConsumerStatefulWidget {
  const _AddProjectDialog();
  @override
  ConsumerState<_AddProjectDialog> createState() => _AddProjectDialogState();
}

class _AddProjectDialogState extends ConsumerState<_AddProjectDialog> {
  _AddProjectStep _step = _AddProjectStep.location;
  bool? _remote;
  bool _busy = false;
  bool _picking = false;
  String? _error;
  List<SshServer>? _servers;
  SshServer? _selected;
  SshServer? _directoryServer;
  SshServer? _saved;
  bool _configurationVisited = false;
  int _formGeneration = 0;
  final _search = TextEditingController();

  @override
  void dispose() {
    _search.dispose();
    super.dispose();
  }

  Future<void> _continue() async {
    if (_busy || _remote == null) return;
    if (_remote!) {
      setState(() => _step = _AddProjectStep.connections);
      await _loadServers();
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      setState(() => _picking = true);
      final path = await ref.read(projectDirectoryPickerProvider)(context);
      if (mounted) setState(() => _picking = false);
      if (!mounted || path == null || path.isEmpty) return;
      await ref.read(studioControllerProvider.notifier).openProject(path);
      if (mounted) Navigator.of(context).pop();
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _loadServers() async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final servers = await ref.read(studioApiProvider).listSshServers();
      if (!mounted) return;
      setState(() => _servers = servers);
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _connect(SshServer server) async {
    final connection = await ref
        .read(studioApiProvider)
        .testSshConnection(server.id);
    if (connection.state != 'ready') {
      throw StateError(connection.errorMessage ?? connection.state);
    }
    if (!mounted) return;
    setState(() {
      _selected = server;
      _directoryServer = server;
      _error = null;
      _step = _AddProjectStep.directory;
    });
  }

  Future<void> _connectSelected() async {
    if (_busy || _selected == null) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await _connect(_selected!);
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _save(SaveSshServerCommand command) async {
    final saved = await ref
        .read(studioApiProvider)
        .saveSshServer(
          SaveSshServerCommand(
            id: _saved?.id ?? command.id,
            name: command.name,
            host: command.host,
            port: command.port,
            username: command.username,
            authKind: command.authKind,
            identityFile: command.identityFile,
            password: command.password,
          ),
        );
    if (!mounted) return;
    _saved = saved;
    _selected = saved;
    _servers = [
      for (final server in _servers ?? <SshServer>[])
        if (server.id != saved.id) server,
      saved,
    ];
    try {
      await _connect(saved);
    } catch (error) {
      if (mounted) {
        throw StateError('${context.l10n.sidebarConnectionSaved}\n$error');
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Stack(
      alignment: Alignment.center,
      children: [
        if (_configurationVisited)
          Offstage(
            offstage: _step != _AddProjectStep.configuration,
            child: SshServerDialog(
              key: ValueKey(_formGeneration),
              server: _saved,
              onSave: _save,
              onBack: () => setState(() => _step = _AddProjectStep.connections),
            ),
          ),
        if (_directoryServer != null)
          Offstage(
            offstage: _step != _AddProjectStep.directory,
            child: RemoteDirectoryDialog(
              key: ValueKey(_directoryServer!.id),
              server: _directoryServer!,
              active: _step == _AddProjectStep.directory,
              onBack: () => setState(() => _step = _AddProjectStep.connections),
            ),
          ),
        if (_step == _AddProjectStep.location ||
            _step == _AddProjectStep.connections)
          _buildSelection(context),
      ],
    );
  }

  Widget _buildSelection(BuildContext context) {
    final location = _step == _AddProjectStep.location;
    return PopScope(
      canPop: !_busy,
      child: AlertDialog(
        key: const ValueKey('add-project-dialog'),
        scrollable: true,
        title: Row(
          children: [
            Expanded(
              child: Text(
                location
                    ? context.l10n.sidebarAddProject
                    : context.l10n.sidebarRemoteProject,
              ),
            ),
            IconButton(
              tooltip: context.l10n.settingsCancel,
              onPressed: _busy ? null : () => Navigator.pop(context),
              icon: const Icon(Icons.close),
            ),
          ],
        ),
        content: SizedBox(
          width: 520,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (location) ...[
                _locationCard(
                  false,
                  Icons.computer_outlined,
                  context.l10n.sidebarLocalProject,
                  context.l10n.sidebarLocalHint,
                ),
                const SizedBox(height: 12),
                _locationCard(
                  true,
                  Icons.dns_outlined,
                  context.l10n.sidebarRemoteProject,
                  context.l10n.sidebarRemoteHint,
                ),
              ] else ...[
                TextField(
                  key: const ValueKey('add-project-connection-search'),
                  controller: _search,
                  decoration: InputDecoration(
                    prefixIcon: const Icon(Icons.search),
                    hintText: context.l10n.sidebarSearchConnections,
                  ),
                  onChanged: (_) => setState(() {}),
                ),
                const SizedBox(height: 12),
                if (_servers?.isEmpty ?? false)
                  Text(context.l10n.sidebarNoConnections),
                for (final server in _servers ?? <SshServer>[])
                  if ('${server.name} ${server.username}@${server.host}'
                      .toLowerCase()
                      .contains(_search.text.toLowerCase()))
                    ListTile(
                      key: ValueKey('add-project-connection-${server.id}'),
                      selected: _selected?.id == server.id,
                      leading: Icon(
                        _selected?.id == server.id
                            ? Icons.radio_button_checked
                            : Icons.radio_button_unchecked,
                      ),
                      title: Text(server.name),
                      subtitle: Text(
                        '${server.username}@${server.host}:${server.port}',
                      ),
                      onTap: _busy
                          ? null
                          : () => setState(() => _selected = server),
                    ),
                const SizedBox(height: 12),
                OutlinedButton.icon(
                  key: const ValueKey('add-project-new-connection'),
                  onPressed: _busy
                      ? null
                      : () => setState(() {
                          if (_saved != null || !_configurationVisited) {
                            _formGeneration++;
                          }
                          _saved = null;
                          _configurationVisited = true;
                          _step = _AddProjectStep.configuration;
                        }),
                  icon: const Icon(Icons.add),
                  label: Text(context.l10n.sidebarNewConnection),
                ),
                if (_selected != null)
                  TextButton(
                    onPressed: _busy
                        ? null
                        : () => setState(() {
                            if (_saved?.id != _selected?.id ||
                                !_configurationVisited) {
                              _formGeneration++;
                            }
                            _saved = _selected;
                            _configurationVisited = true;
                            _step = _AddProjectStep.configuration;
                          }),
                    child: Text(context.l10n.settingsSshEdit),
                  ),
              ],
              if (_busy && !_picking)
                const Padding(
                  padding: EdgeInsets.only(top: 12),
                  child: LinearProgressIndicator(),
                ),
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: Text(
                    _error!,
                    style: TextStyle(color: context.colors.error),
                  ),
                ),
              if (!location && _servers == null && !_busy)
                TextButton(
                  onPressed: _loadServers,
                  child: Text(context.l10n.sidebarRetry),
                ),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: _busy
                ? null
                : () {
                    if (location) {
                      Navigator.pop(context);
                    } else {
                      setState(() {
                        _step = _AddProjectStep.location;
                        _error = null;
                      });
                    }
                  },
            child: Text(
              location ? context.l10n.settingsCancel : context.l10n.sidebarBack,
            ),
          ),
          FilledButton(
            key: const ValueKey('add-project-continue'),
            onPressed: _busy || (location ? _remote == null : _selected == null)
                ? null
                : location
                ? _continue
                : _connectSelected,
            child: Text(
              location
                  ? context.l10n.sidebarContinue
                  : context.l10n.sidebarConnect,
            ),
          ),
        ],
      ),
    );
  }

  Widget _locationCard(
    bool remote,
    IconData icon,
    String title,
    String subtitle,
  ) => Card(
    elevation: 0,
    margin: EdgeInsets.zero,
    shape: RoundedRectangleBorder(
      borderRadius: BorderRadius.circular(8),
      side: BorderSide(
        color: _remote == remote
            ? context.colors.primary
            : context.colors.outlineVariant,
      ),
    ),
    child: ListTile(
      key: ValueKey(remote ? 'add-project-remote' : 'add-project-local'),
      contentPadding: const EdgeInsets.all(16),
      leading: Icon(icon),
      title: Text(title),
      subtitle: Text(subtitle),
      selected: _remote == remote,
      onTap: _busy ? null : () => setState(() => _remote = remote),
    ),
  );
}
