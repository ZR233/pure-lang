part of 'settings_page.dart';

class _InstructionsTab extends ConsumerStatefulWidget {
  const _InstructionsTab({required this.settings});

  final InstructionsSettingsView settings;

  @override
  ConsumerState<_InstructionsTab> createState() => _InstructionsTabState();
}

class _InstructionsTabState extends ConsumerState<_InstructionsTab> {
  late final TextEditingController _baseController;
  late final TextEditingController _developerController;
  late final TextEditingController _userController;
  Timer? _saveTimer;
  bool _saving = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _baseController = TextEditingController(text: widget.settings.baseOverride);
    _developerController = TextEditingController(
      text: widget.settings.developer,
    );
    _userController = TextEditingController(text: widget.settings.user);
  }

  @override
  void didUpdateWidget(covariant _InstructionsTab oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_saving) {
      return;
    }
    if (oldWidget.settings.baseOverride != widget.settings.baseOverride &&
        _baseController.text != widget.settings.baseOverride) {
      _baseController.text = widget.settings.baseOverride;
    }
    if (oldWidget.settings.developer != widget.settings.developer &&
        _developerController.text != widget.settings.developer) {
      _developerController.text = widget.settings.developer;
    }
    if (oldWidget.settings.user != widget.settings.user &&
        _userController.text != widget.settings.user) {
      _userController.text = widget.settings.user;
    }
  }

  @override
  void dispose() {
    _saveTimer?.cancel();
    _baseController.dispose();
    _developerController.dispose();
    _userController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return _SettingsPane(
      children: [
        TextField(
          controller: _baseController,
          maxLines: 8,
          decoration: const InputDecoration(
            labelText: 'Base instructions',
            prefixIcon: Icon(Icons.notes_outlined),
          ),
          onChanged: (_) => _scheduleSave(),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: _developerController,
          maxLines: 8,
          decoration: const InputDecoration(
            labelText: 'Developer instructions',
            prefixIcon: Icon(Icons.code),
          ),
          onChanged: (_) => _scheduleSave(),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: _userController,
          maxLines: 8,
          decoration: const InputDecoration(
            labelText: 'User context',
            prefixIcon: Icon(Icons.person_outline),
          ),
          onChanged: (_) => _scheduleSave(),
        ),
        if (_saving || _error != null) ...[
          const SizedBox(height: 12),
          if (_saving) const LinearProgressIndicator(),
          if (_error != null) _InlineError(message: _error!),
        ],
      ],
    );
  }

  void _scheduleSave() {
    _saveTimer?.cancel();
    _saveTimer = Timer(const Duration(milliseconds: 650), () {
      unawaited(_save());
    });
  }

  Future<void> _save() async {
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await ref
          .read(studioControllerProvider.notifier)
          .saveInstructionsSettings({
            'baseOverride': _baseController.text,
            'developer': _developerController.text,
            'user': _userController.text,
            'projectDocMaxBytes': widget.settings.projectDocMaxBytes,
            'projectDocFallbackFilenames':
                widget.settings.projectDocFallbackFilenames,
          });
    } catch (error) {
      if (mounted) {
        setState(() => _error = error.toString());
      }
    } finally {
      if (mounted) {
        setState(() => _saving = false);
      }
    }
  }
}

class _SkillsTab extends ConsumerStatefulWidget {
  const _SkillsTab({
    required this.skills,
    required this.settings,
    required this.projectId,
  });

  final List<String> skills;
  final SkillsSettingsView settings;
  final String? projectId;

  @override
  ConsumerState<_SkillsTab> createState() => _SkillsTabState();
}

class _SkillsTabState extends ConsumerState<_SkillsTab> {
  String _query = '';
  final Set<String> _discoveredSkills = {};
  bool _discovering = false;
  String? _discoverError;
  String? _saveError;

  @override
  void initState() {
    super.initState();
    _discoveredSkills.addAll(widget.skills);
  }

  @override
  void didUpdateWidget(covariant _SkillsTab oldWidget) {
    super.didUpdateWidget(oldWidget);
    _discoveredSkills.addAll(widget.skills);
  }

  @override
  Widget build(BuildContext context) {
    final skills = {...widget.skills, ..._discoveredSkills}.toList()..sort();
    final disabledSkills = widget.settings.disabled.toSet();
    final filteredSkills = skills
        .where((skill) => skill.toLowerCase().contains(_query.toLowerCase()))
        .toList();
    return _SettingsPane(
      children: [
        SearchBar(
          leading: const Icon(Icons.search),
          hintText: 'Filter skills',
          onChanged: (value) => setState(() => _query = value),
        ),
        const SizedBox(height: 14),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            for (final skill in filteredSkills)
              FilterChip(
                avatar: const Icon(Icons.extension_outlined, size: 18),
                label: Text(skill, overflow: TextOverflow.ellipsis),
                selected: !disabledSkills.contains(skill),
                onSelected: (selected) {
                  final disabled = {...disabledSkills};
                  if (selected) {
                    disabled.remove(skill);
                  } else {
                    disabled.add(skill);
                  }
                  unawaited(_saveDisabled(disabled));
                },
              ),
          ],
        ),
        if (filteredSkills.isEmpty) ...[
          const SizedBox(height: 12),
          _EmptySettingsMessage(
            icon: Icons.extension_outlined,
            title: widget.projectId == null
                ? 'Open a project to discover skills'
                : 'No skills match this filter',
            body: widget.projectId == null
                ? 'Skills are discovered from the selected workspace and configured user/system sources.'
                : 'Clear the search or run discovery again.',
          ),
        ],
        if (_discoverError != null) ...[
          const SizedBox(height: 12),
          _InlineError(message: _discoverError!),
        ],
        if (_saveError != null) ...[
          const SizedBox(height: 12),
          _InlineError(message: _saveError!),
        ],
        const SizedBox(height: 16),
        Align(
          alignment: Alignment.centerLeft,
          child: FilledButton.icon(
            icon: _discovering
                ? const SizedBox.square(
                    dimension: 18,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.travel_explore),
            label: Text(_discovering ? 'Discovering' : 'Discover'),
            onPressed: widget.projectId == null || _discovering
                ? null
                : _discoverSkills,
          ),
        ),
      ],
    );
  }

  Future<void> _saveDisabled(Set<String> disabled) async {
    try {
      setState(() => _saveError = null);
      await ref.read(studioControllerProvider.notifier).saveSkillsSettings({
        'enabled': widget.settings.enabled,
        'autoLearn': widget.settings.autoLearn,
        'systemEnabled': widget.settings.systemEnabled,
        'projectDir': widget.settings.projectDir,
        'userDir': widget.settings.userDir,
        'externalDirs': widget.settings.externalDirs,
        'disabled': disabled.toList()..sort(),
        'autoLearnMinToolCalls': widget.settings.autoLearnMinToolCalls,
      });
    } catch (error) {
      if (mounted) {
        setState(() => _saveError = error.toString());
      }
    }
  }

  Future<void> _discoverSkills() async {
    setState(() {
      _discovering = true;
      _discoverError = null;
    });
    try {
      final skills = await ref
          .read(studioControllerProvider.notifier)
          .listDiscoveredSkills();
      if (!mounted) {
        return;
      }
      setState(() => _discoveredSkills.addAll(skills));
    } catch (error) {
      if (mounted) {
        setState(() => _discoverError = error.toString());
      }
    } finally {
      if (mounted) {
        setState(() => _discovering = false);
      }
    }
  }
}

class _RolesTab extends ConsumerStatefulWidget {
  const _RolesTab({required this.providers, required this.roles});

  final List<ProviderSettingsView> providers;
  final List<RoleSettingsView> roles;

  @override
  ConsumerState<_RolesTab> createState() => _RolesTabState();
}

class _RolesTabState extends ConsumerState<_RolesTab> {
  final Map<String, String> _selectionByRole = {};

  @override
  Widget build(BuildContext context) {
    const roles = ['explorer', 'planner', 'executor', 'reviewer'];
    final options = _roleModelOptions(widget.providers);
    return _SettingsPane(
      children: [
        for (final role in roles)
          Card(
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Row(
                children: [
                  const Icon(Icons.badge_outlined),
                  const SizedBox(width: 12),
                  Expanded(
                    flex: 2,
                    child: Text(
                      role,
                      style: Theme.of(context).textTheme.titleSmall,
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    flex: 3,
                    child: DropdownButtonFormField<String>(
                      initialValue: _selectedRoleModelKey(role, options),
                      decoration: const InputDecoration(labelText: 'Model'),
                      items: [
                        if (options.isEmpty)
                          const DropdownMenuItem(
                            value: 'default::default',
                            child: Text('default'),
                          )
                        else
                          for (final option in options)
                            DropdownMenuItem(
                              value: option.key,
                              child: Text(
                                option.label,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                      ],
                      onChanged: (value) {
                        if (value == null) {
                          return;
                        }
                        setState(() => _selectionByRole[role] = value);
                        final option = options.firstWhere(
                          (option) => option.key == value,
                        );
                        ref
                            .read(studioControllerProvider.notifier)
                            .setModelRole(
                              roleKey: role,
                              providerId: option.providerId,
                              model: option.model,
                              effort: option.effort,
                            );
                      },
                    ),
                  ),
                ],
              ),
            ),
          ),
      ],
    );
  }

  String? _roleSelectionKey(String roleKey) {
    final role = widget.roles.where((role) => role.key == roleKey).firstOrNull;
    if (role == null || role.providerId.isEmpty || role.model.isEmpty) {
      return null;
    }
    return '${role.providerId}::${role.model}';
  }

  String _selectedRoleModelKey(String role, List<_RoleModelOption> options) {
    final selected = _selectionByRole[role];
    if (selected != null && options.any((option) => option.key == selected)) {
      return selected;
    }
    final configured = _roleSelectionKey(role);
    if (configured != null &&
        options.any((option) => option.key == configured)) {
      return configured;
    }
    return options.isEmpty ? 'default::default' : options.first.key;
  }

  List<_RoleModelOption> _roleModelOptions(
    List<ProviderSettingsView> providers,
  ) {
    final options = <_RoleModelOption>[];
    for (final provider in providers) {
      final models = provider.models.isEmpty
          ? [
              ProviderModelView(
                slug: provider.defaultModel,
                displayName: provider.defaultModel,
                reasoningEfforts: const [],
              ),
            ]
          : provider.models;
      for (final model in models) {
        if (model.slug.isEmpty) {
          continue;
        }
        options.add(
          _RoleModelOption(
            providerId: provider.id,
            model: model.slug,
            label:
                '${provider.name} / ${model.displayName.isEmpty ? model.slug : model.displayName}',
            effort: model.reasoningEfforts.firstOrNull,
          ),
        );
      }
    }
    return options;
  }
}

class _RoleModelOption {
  const _RoleModelOption({
    required this.providerId,
    required this.model,
    required this.label,
    required this.effort,
  });

  final String providerId;
  final String model;
  final String label;
  final String? effort;

  String get key => '$providerId::$model';
}

class _McpTab extends ConsumerStatefulWidget {
  const _McpTab({required this.servers});

  final List<McpServerSettingsView> servers;

  @override
  ConsumerState<_McpTab> createState() => _McpTabState();
}

class _McpTabState extends ConsumerState<_McpTab> {
  final Map<String, bool> _enabledByServer = {};
  final Map<String, String> _endpointByServer = {};
  Timer? _saveTimer;
  String? _error;

  @override
  void dispose() {
    _saveTimer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return _SettingsPane(
      children: [
        for (final server in widget.servers)
          Card(
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Column(
                children: [
                  SwitchListTile(
                    value: _enabledByServer[server.id] ?? server.enabled,
                    onChanged: (value) {
                      setState(() {
                        _enabledByServer[server.id] = value;
                      });
                      unawaited(_save());
                    },
                    secondary: const Icon(Icons.hub_outlined),
                    title: Text(server.id),
                    subtitle: Text('${server.transport}  ${server.status}'),
                  ),
                  TextFormField(
                    initialValue: server.endpoint,
                    decoration: const InputDecoration(labelText: 'Endpoint'),
                    onChanged: (value) => setState(() {
                      _endpointByServer[server.id] = value;
                      _scheduleSave();
                    }),
                  ),
                ],
              ),
            ),
          ),
        if (_error != null) _InlineError(message: _error!),
      ],
    );
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
      await ref.read(studioControllerProvider.notifier).saveMcpSettings({
        'servers': [
          for (final server in widget.servers)
            {
              'id': server.id,
              'enabled': _enabledByServer[server.id] ?? server.enabled,
              'transport': server.transport,
              'endpoint': _endpointByServer[server.id] ?? server.endpoint,
            },
        ],
      });
    } catch (error) {
      if (mounted) {
        setState(() => _error = error.toString());
      }
    }
  }
}

class _SecurityTab extends ConsumerWidget {
  const _SecurityTab({required this.mode});

  final PermissionMode mode;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return _SettingsPane(
      maxWidth: 620,
      children: [
        SegmentedButton<PermissionMode>(
          showSelectedIcon: false,
          segments: const [
            ButtonSegment(
              value: PermissionMode.requestApproval,
              icon: Icon(Icons.verified_user_outlined),
              label: Text('Request'),
            ),
            ButtonSegment(
              value: PermissionMode.autoReview,
              icon: Icon(Icons.rule_folder_outlined),
              label: Text('Review'),
            ),
            ButtonSegment(
              value: PermissionMode.fullAccess,
              icon: Icon(Icons.lock_open_outlined),
              label: Text('Full'),
            ),
          ],
          selected: {mode},
          onSelectionChanged: (selection) {
            ref
                .read(studioControllerProvider.notifier)
                .setPermissionMode(selection.first);
          },
        ),
        const SizedBox(height: 12),
        Card(
          child: ListTile(
            leading: const Icon(Icons.security_outlined),
            title: Text(mode.name),
            subtitle: const Text('Workspace boundary policy'),
          ),
        ),
      ],
    );
  }
}

class _GeneralTab extends ConsumerStatefulWidget {
  const _GeneralTab({required this.settings});

  final GeneralSettingsView settings;

  @override
  ConsumerState<_GeneralTab> createState() => _GeneralTabState();
}

class _GeneralTabState extends ConsumerState<_GeneralTab> {
  String? _error;

  @override
  Widget build(BuildContext context) {
    return _SettingsPane(
      children: [
        SwitchListTile(
          value: widget.settings.followSystemTheme,
          onChanged: (value) =>
              _save(widget.settings.copyWith(followSystemTheme: value)),
          secondary: const Icon(Icons.dark_mode_outlined),
          title: const Text('Follow system theme'),
        ),
        SwitchListTile(
          value: widget.settings.followActiveTurn,
          onChanged: (value) =>
              _save(widget.settings.copyWith(followActiveTurn: value)),
          secondary: const Icon(Icons.vertical_align_bottom),
          title: const Text('Follow active turn'),
        ),
        SwitchListTile(
          value: widget.settings.compactTimeline,
          onChanged: (value) =>
              _save(widget.settings.copyWith(compactTimeline: value)),
          secondary: const Icon(Icons.view_agenda_outlined),
          title: const Text('Compact timeline'),
        ),
        if (_error != null) _InlineError(message: _error!),
      ],
    );
  }

  Future<void> _save(GeneralSettingsView settings) async {
    try {
      setState(() => _error = null);
      await ref.read(studioControllerProvider.notifier).saveGeneralSettings({
        'followSystemTheme': settings.followSystemTheme,
        'followActiveTurn': settings.followActiveTurn,
        'compactTimeline': settings.compactTimeline,
      });
    } catch (error) {
      if (mounted) {
        setState(() => _error = error.toString());
      }
    }
  }
}

class _ReadonlyField extends StatelessWidget {
  const _ReadonlyField({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: TextFormField(
        initialValue: value,
        readOnly: true,
        decoration: InputDecoration(labelText: label),
      ),
    );
  }
}
