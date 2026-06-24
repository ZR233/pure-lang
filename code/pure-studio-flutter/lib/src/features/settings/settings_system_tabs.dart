part of 'settings_page.dart';

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
        _SettingsHeader(
          title: context.l10n.settingsRolesTitle,
          subtitle: context.l10n.settingsRolesSubtitle,
        ),
        const SizedBox(height: 16),
        LayoutBuilder(
          builder: (context, constraints) {
            final twoColumns = constraints.maxWidth >= 760;
            return Wrap(
              spacing: 14,
              runSpacing: 14,
              children: [
                for (final role in roles)
                  SizedBox(
                    width: twoColumns
                        ? (constraints.maxWidth - 14) / 2
                        : constraints.maxWidth,
                    child: _RoleSettingsCard(
                      role: role,
                      selectedValue: _selectedRoleModelKey(role, options),
                      options: options,
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
            );
          },
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

class _RoleSettingsCard extends StatelessWidget {
  const _RoleSettingsCard({
    required this.role,
    required this.selectedValue,
    required this.options,
    required this.onChanged,
  });

  final String role;
  final String selectedValue;
  final List<_RoleModelOption> options;
  final ValueChanged<String?> onChanged;

  @override
  Widget build(BuildContext context) {
    return _SectionPanel(
      title: role,
      children: [
        Text(
          _roleDescription(context, role),
          style: context.text.bodySmall?.copyWith(color: context.studioInkSoft),
        ),
        const SizedBox(height: 10),
        DropdownButtonFormField<String>(
          initialValue: selectedValue,
          isExpanded: true,
          decoration: InputDecoration(
            labelText: context.l10n.settingsModelField,
          ),
          selectedItemBuilder: (context) {
            final entries = options.isEmpty
                ? const [_RoleModelOption.defaultOption()]
                : options;
            return [
              for (final option in entries)
                Align(
                  alignment: Alignment.centerLeft,
                  child: Text(
                    option.label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
            ];
          },
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
                  child: Text(option.label, overflow: TextOverflow.ellipsis),
                ),
          ],
          onChanged: onChanged,
        ),
      ],
    );
  }
}

String _roleDescription(BuildContext context, String role) {
  return switch (role) {
    'explorer' => context.l10n.settingsRoleExplorerDescription,
    'planner' => context.l10n.settingsRolePlannerDescription,
    'executor' => context.l10n.settingsRoleExecutorDescription,
    'reviewer' => context.l10n.settingsRoleReviewerDescription,
    _ => context.l10n.settingsRoleFallbackDescription,
  };
}

class _RoleModelOption {
  const _RoleModelOption({
    required this.providerId,
    required this.model,
    required this.label,
    required this.effort,
  });

  const _RoleModelOption.defaultOption()
    : providerId = 'default',
      model = 'default',
      label = 'default',
      effort = null;

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
        _SettingsHeader(
          title: context.l10n.settingsMcpTitle,
          subtitle: context.l10n.settingsMcpSubtitle,
        ),
        const SizedBox(height: 16),
        for (final server in widget.servers)
          Padding(
            padding: const EdgeInsets.only(bottom: 10),
            child: _SectionPanel(
              title: server.id,
              trailing: Switch(
                value: _enabledByServer[server.id] ?? server.enabled,
                onChanged: (value) {
                  setState(() {
                    _enabledByServer[server.id] = value;
                  });
                  unawaited(_save());
                },
              ),
              children: [
                Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  children: [
                    _InfoPill(
                      icon: Icons.hub_outlined,
                      label: server.transport,
                    ),
                    _InfoPill(
                      icon: Icons.circle_outlined,
                      label: server.status,
                    ),
                  ],
                ),
                const SizedBox(height: 10),
                TextFormField(
                  initialValue: server.endpoint,
                  decoration: InputDecoration(
                    labelText: context.l10n.settingsEndpoint,
                  ),
                  onChanged: (value) => setState(() {
                    _endpointByServer[server.id] = value;
                    _scheduleSave();
                  }),
                ),
              ],
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
        _SettingsHeader(
          title: context.l10n.settingsSecurityTitle,
          subtitle: context.l10n.settingsSecurityModeSubtitle,
        ),
        const SizedBox(height: 16),
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
        _SettingsRow(
          icon: Icons.security_outlined,
          title: context.l10n.settingsCurrentMode(mode.name),
          subtitle: context.l10n.settingsWorkspaceBoundary,
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
        _SettingsHeader(
          title: context.l10n.settingsGeneralTitle,
          subtitle: context.l10n.settingsGeneralSubtitle,
        ),
        const SizedBox(height: 16),
        _SettingsToggleRow(
          icon: Icons.dark_mode_outlined,
          title: context.l10n.settingsFollowSystemTheme,
          subtitle: context.l10n.settingsFollowSystemThemeSubtitle,
          value: widget.settings.followSystemTheme,
          onChanged: (value) =>
              _save(widget.settings.copyWith(followSystemTheme: value)),
        ),
        const SizedBox(height: 10),
        _SettingsToggleRow(
          icon: Icons.vertical_align_bottom,
          title: context.l10n.settingsFollowActiveTurn,
          subtitle: context.l10n.settingsFollowActiveTurnSubtitle,
          value: widget.settings.followActiveTurn,
          onChanged: (value) =>
              _save(widget.settings.copyWith(followActiveTurn: value)),
        ),
        const SizedBox(height: 10),
        _SettingsToggleRow(
          icon: Icons.view_agenda_outlined,
          title: context.l10n.settingsCompactTimeline,
          subtitle: context.l10n.settingsCompactTimelineSubtitle,
          value: widget.settings.compactTimeline,
          onChanged: (value) =>
              _save(widget.settings.copyWith(compactTimeline: value)),
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
