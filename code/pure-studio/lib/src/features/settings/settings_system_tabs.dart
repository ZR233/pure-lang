import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';
import 'settings_update_row.dart';
import 'settings_web_search.dart';

class RolesTab extends ConsumerWidget {
  const RolesTab({super.key, required this.providers, required this.roles});

  final List<ProviderSettingsView> providers;
  final List<RoleSettingsView> roles;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    const roleKeys = ['explorer', 'planner', 'executor', 'reviewer'];
    final options = _roleModelOptions(providers);
    return SettingsPane(
      children: [
        SettingsHeader(
          title: context.l10n.settingsRolesTitle,
          subtitle: context.l10n.settingsRolesSubtitle,
        ),
        const SizedBox(height: 16),
        SettingsGroup(
          children: [
            for (final role in roleKeys) _buildRoleRow(ref, role, options),
          ],
        ),
      ],
    );
  }

  Widget _buildRoleRow(
    WidgetRef ref,
    String role,
    List<_RoleModelOption> options,
  ) {
    final selectedModel = _selectedRoleModelKey(role, options);
    final selectedOption = options
        .where((option) => option.key == selectedModel)
        .firstOrNull;
    final option = selectedOption ?? const _RoleModelOption.defaultOption();
    final configuredRole = roles
        .where((candidate) => candidate.key == role)
        .firstOrNull;
    final canonicalEffort = configuredRole?.effort;
    final selectedEffort =
        option.key == _roleSelectionKey(role) &&
            option.efforts.contains(canonicalEffort)
        ? canonicalEffort
        : option.defaultEffort;

    return _RoleSettingsRow(
      role: role,
      selectedModel: selectedModel,
      selectedEffort: selectedEffort,
      options: options,
      efforts: option.efforts,
      onModelChanged: (value) {
        final selected = options.firstWhere(
          (candidate) => candidate.key == value,
        );
        ref
            .read(studioControllerProvider.notifier)
            .setModelRole(
              roleKey: role,
              providerId: selected.providerId,
              model: selected.model,
              effort: selected.defaultEffort,
            );
      },
      onEffortChanged: (value) {
        ref
            .read(studioControllerProvider.notifier)
            .setModelRole(
              roleKey: role,
              providerId: option.providerId,
              model: option.model,
              effort: value,
            );
      },
    );
  }

  String? _roleSelectionKey(String roleKey) {
    final role = roles.where((role) => role.key == roleKey).firstOrNull;
    if (role == null || role.providerId.isEmpty || role.model.isEmpty) {
      return null;
    }
    return '${role.providerId}::${role.model}';
  }

  String _selectedRoleModelKey(String role, List<_RoleModelOption> options) {
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
                '${provider.name} / ${model.displayName.isEmpty ? model.slug : model.displayName} · ${_roleProtocolLabel(model.wireProtocol)} · ${_roleConnectionLabel(model.connectionMode)}',
            efforts: model.reasoningEfforts,
            defaultEffort: model.defaultReasoningEffort.isNotEmpty
                ? model.defaultReasoningEffort
                : model.reasoningEfforts.firstOrNull,
          ),
        );
      }
    }
    return options;
  }
}

String _roleProtocolLabel(String protocol) => switch (protocol) {
  'responses' => 'Responses',
  'chat_completions' => 'Chat Completions',
  _ => protocol,
};

String _roleConnectionLabel(String mode) => switch (mode) {
  'web_socket' => 'WS',
  'http' => 'HTTP',
  _ => mode,
};

class _RoleSettingsRow extends StatelessWidget {
  const _RoleSettingsRow({
    required this.role,
    required this.selectedModel,
    required this.selectedEffort,
    required this.options,
    required this.efforts,
    required this.onModelChanged,
    required this.onEffortChanged,
  });

  final String role;
  final String selectedModel;
  final String? selectedEffort;
  final List<_RoleModelOption> options;
  final List<String> efforts;
  final ValueChanged<String> onModelChanged;
  final ValueChanged<String> onEffortChanged;

  @override
  Widget build(BuildContext context) {
    final title = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          context.roleLabel(role),
          style: context.text.bodyMedium?.copyWith(
            color: context.studioInk,
            fontWeight: FontWeight.w600,
          ),
        ),
        const SizedBox(height: 2),
        Text(
          _roleDescription(context, role),
          style: context.text.bodySmall?.copyWith(color: context.studioInkSoft),
        ),
      ],
    );
    final modelEntries = options.isEmpty
        ? const [_RoleModelOption.defaultOption()]
        : options;
    final modelSelector = _RoleSelectField(
      selectorKey: StudioDriverKeys.settingsRoleModel(role),
      label: context.l10n.settingsModelField,
      value: selectedModel,
      options: [
        for (final option in modelEntries)
          _RoleSelectOption(
            key: StudioDriverKeys.settingsRoleModelOption(
              role,
              option.providerId,
              option.model,
            ),
            value: option.key,
            label: option.label,
          ),
      ],
      onChanged: options.isEmpty ? null : onModelChanged,
    );
    final effortSelector = _RoleSelectField(
      selectorKey: StudioDriverKeys.settingsRoleEffort(role),
      label: context.l10n.statusReasoningEffort,
      value: selectedEffort,
      options: [
        for (final effort in efforts)
          _RoleSelectOption(
            key: StudioDriverKeys.settingsRoleEffortOption(role, effort),
            value: effort,
            label: effort,
          ),
      ],
      onChanged: efforts.isEmpty ? null : onEffortChanged,
    );
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      child: LayoutBuilder(
        builder: (context, constraints) {
          if (constraints.maxWidth < 760) {
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                title,
                const SizedBox(height: 10),
                modelSelector,
                const SizedBox(height: 10),
                effortSelector,
              ],
            );
          }
          return Row(
            children: [
              Expanded(child: title),
              const SizedBox(width: 20),
              SizedBox(width: 280, child: modelSelector),
              const SizedBox(width: 12),
              SizedBox(width: 140, child: effortSelector),
            ],
          );
        },
      ),
    );
  }
}

class _RoleSelectField extends StatelessWidget {
  const _RoleSelectField({
    required this.selectorKey,
    required this.label,
    required this.value,
    required this.options,
    required this.onChanged,
  });

  final Key selectorKey;
  final String label;
  final String? value;
  final List<_RoleSelectOption> options;
  final ValueChanged<String>? onChanged;

  @override
  Widget build(BuildContext context) {
    final enabled = onChanged != null && options.isNotEmpty;
    final selectedLabel = options
        .where((option) => option.value == value)
        .firstOrNull
        ?.label;
    return MenuAnchor(
      menuChildren: [
        for (final option in options)
          MenuItemButton(
            key: option.key,
            onPressed: enabled ? () => onChanged!(option.value) : null,
            child: Text(option.label, overflow: TextOverflow.ellipsis),
          ),
      ],
      builder: (context, controller, child) {
        return InkWell(
          key: selectorKey,
          onTap: enabled
              ? () => controller.isOpen ? controller.close() : controller.open()
              : null,
          borderRadius: BorderRadius.circular(4),
          child: InputDecorator(
            isEmpty: selectedLabel == null,
            isFocused: controller.isOpen,
            decoration: InputDecoration(
              labelText: label,
              isDense: true,
              enabled: enabled,
              suffixIcon: const Icon(Icons.arrow_drop_down),
            ),
            child: Text(
              selectedLabel ?? '',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          ),
        );
      },
    );
  }
}

class _RoleSelectOption {
  const _RoleSelectOption({
    required this.key,
    required this.value,
    required this.label,
  });

  final Key key;
  final String value;
  final String label;
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
    required this.efforts,
    required this.defaultEffort,
  });

  const _RoleModelOption.defaultOption()
    : providerId = 'default',
      model = 'default',
      label = 'default',
      efforts = const [],
      defaultEffort = null;

  final String providerId;
  final String model;
  final String label;
  final List<String> efforts;
  final String? defaultEffort;

  String get key => '$providerId::$model';
}

class McpTab extends ConsumerStatefulWidget {
  const McpTab({super.key, required this.settingsServers, required this.state});

  final List<McpServerSettingsView> settingsServers;
  final McpStateSnapshot state;

  @override
  ConsumerState<McpTab> createState() => McpTabState();
}

class McpTabState extends ConsumerState<McpTab> {
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
      children: [
        SettingsHeader(
          title: context.l10n.settingsMcpTitle,
          subtitle: context.l10n.settingsMcpSubtitle,
          trailing: Wrap(
            spacing: 8,
            children: [
              OutlinedButton.icon(
                key: StudioDriverKeys.mcpRefresh,
                onPressed: () => unawaited(_run(_refresh)),
                icon: const Icon(Icons.refresh),
                label: Text(context.l10n.settingsMcpRefresh),
              ),
              FilledButton.tonalIcon(
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
    final availabilityMessage = server.availabilityMessage;
    return Padding(
      key: StudioDriverKeys.mcpServerRow(server.id),
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  server.id,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.bodyMedium?.copyWith(
                    color: context.studioInk,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              Switch(value: enabled, onChanged: onEnabledChanged),
              const SizedBox(width: 8),
              OutlinedButton.icon(
                key: StudioDriverKeys.mcpResetServer(server.id),
                onPressed: server.enabled ? onReconnect : null,
                icon: const Icon(Icons.sync),
                label: Text(context.l10n.settingsMcpReconnect),
              ),
            ],
          ),
          Wrap(
            spacing: 8,
            runSpacing: 6,
            children: [
              SettingsInfoPill(
                icon: Icons.hub_outlined,
                label: server.transport,
              ),
              SettingsInfoPill(
                icon: Icons.circle_outlined,
                label: server.displayedAvailability,
              ),
            ],
          ),
          if (server.availabilityKind == 'unavailable' &&
              availabilityMessage != null) ...[
            const SizedBox(height: 8),
            Text(
              availabilityMessage,
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
        ],
      ),
    );
  }
}

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
      children: [
        SettingsHeader(
          title: context.l10n.settingsLspTitle,
          subtitle: context.l10n.settingsLspSubtitle,
          trailing: Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              OutlinedButton.icon(
                key: StudioDriverKeys.lspRefresh,
                onPressed: () => unawaited(_run(_refresh)),
                icon: const Icon(Icons.refresh),
                label: Text(context.l10n.settingsLspRefresh),
              ),
              FilledButton.tonalIcon(
                key: StudioDriverKeys.lspProbe,
                onPressed: widget.projectId == null
                    ? null
                    : () => unawaited(_run(_probe)),
                icon: const Icon(Icons.monitor_heart_outlined),
                label: Text(context.l10n.settingsLspProbe),
              ),
              OutlinedButton.icon(
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
        const SizedBox(height: 16),
        if (widget.state.servers.isNotEmpty)
          SettingsGroup(
            children: [
              for (final server in widget.state.servers)
                _LspSettingsRow(
                  server: server,
                  onRepair: server.availability == 'missingServerComponent'
                      ? () => unawaited(_run(() => _repair(server.id)))
                      : null,
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
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  server.displayName,
                  style: context.text.bodyMedium?.copyWith(
                    color: context.studioInk,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              if (onRepair != null)
                FilledButton.tonalIcon(
                  key: StudioDriverKeys.lspRepairServer(server.id),
                  onPressed: onRepair,
                  icon: const Icon(Icons.build_outlined),
                  label: Text(context.l10n.settingsLspRepair),
                ),
              const SizedBox(width: 8),
              OutlinedButton.icon(
                key: StudioDriverKeys.lspResetServer(server.id),
                onPressed: onReset,
                icon: const Icon(Icons.restart_alt),
                label: Text(context.l10n.settingsLspReset),
              ),
            ],
          ),
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            runSpacing: 6,
            children: [
              SettingsInfoPill(
                icon: Icons.circle_outlined,
                label: server.availability,
              ),
              SettingsInfoPill(
                icon: Icons.rule_outlined,
                label: server.diagnosticCount.toString(),
              ),
            ],
          ),
          if (server.message case final message?) ...[
            const SizedBox(height: 8),
            Text(message, style: context.text.bodySmall),
          ],
          if (server.lastError case final error?) ...[
            const SizedBox(height: 8),
            Text(
              error,
              style: context.text.bodySmall?.copyWith(
                color: context.colors.error,
              ),
            ),
          ],
        ],
      ),
    );
  }
}

class SecurityTab extends ConsumerWidget {
  const SecurityTab({super.key, required this.mode});

  final PermissionMode mode;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return SettingsPane(
      maxWidth: 620,
      children: [
        SettingsHeader(
          title: context.l10n.settingsSecurityTitle,
          subtitle: context.l10n.settingsSecurityModeSubtitle,
        ),
        const SizedBox(height: 16),
        SettingsGroup(
          children: [
            Padding(
              padding: const EdgeInsets.all(14),
              child: LayoutBuilder(
                builder: (context, constraints) {
                  return Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      SegmentedButton<PermissionMode>(
                        direction: constraints.maxWidth < 520
                            ? Axis.vertical
                            : Axis.horizontal,
                        showSelectedIcon: false,
                        segments: [
                          ButtonSegment(
                            value: PermissionMode.requestApproval,
                            icon: const Icon(Icons.verified_user_outlined),
                            label: Text(
                              context.permissionModeLabel(
                                PermissionMode.requestApproval,
                              ),
                            ),
                          ),
                          ButtonSegment(
                            value: PermissionMode.autoReview,
                            icon: const Icon(Icons.rule_folder_outlined),
                            label: Text(
                              context.permissionModeLabel(
                                PermissionMode.autoReview,
                              ),
                            ),
                          ),
                          ButtonSegment(
                            value: PermissionMode.fullAccess,
                            icon: const Icon(Icons.lock_open_outlined),
                            label: Text(
                              context.permissionModeLabel(
                                PermissionMode.fullAccess,
                              ),
                            ),
                          ),
                        ],
                        selected: {mode},
                        onSelectionChanged: (selection) {
                          ref
                              .read(studioControllerProvider.notifier)
                              .setPermissionMode(selection.first);
                        },
                      ),
                      const SizedBox(height: 10),
                      Text(
                        context.l10n.settingsWorkspaceBoundary,
                        style: context.text.bodySmall?.copyWith(
                          color: context.studioInkSoft,
                        ),
                      ),
                    ],
                  );
                },
              ),
            ),
          ],
        ),
      ],
    );
  }
}

class GeneralTab extends ConsumerStatefulWidget {
  const GeneralTab({
    super.key,
    required this.settings,
    required this.webSearch,
    required this.runtimeBusy,
  });

  final GeneralSettingsView settings;
  final WebSearchSettingsView webSearch;
  final bool runtimeBusy;

  @override
  ConsumerState<GeneralTab> createState() => GeneralTabState();
}

class GeneralTabState extends ConsumerState<GeneralTab> {
  String? _error;

  @override
  Widget build(BuildContext context) {
    return SettingsPane(
      children: [
        SettingsHeader(
          title: context.l10n.settingsGeneralTitle,
          subtitle: context.l10n.settingsGeneralSubtitle,
        ),
        const SizedBox(height: 16),
        SettingsGroup(
          children: [
            SettingsToggleRow(
              icon: Icons.dark_mode_outlined,
              title: context.l10n.settingsFollowSystemTheme,
              subtitle: context.l10n.settingsFollowSystemThemeSubtitle,
              value: widget.settings.followSystemTheme,
              onChanged: (value) =>
                  _save(widget.settings.copyWith(followSystemTheme: value)),
            ),
            SettingsToggleRow(
              icon: Icons.vertical_align_bottom,
              title: context.l10n.settingsFollowActiveTurn,
              subtitle: context.l10n.settingsFollowActiveTurnSubtitle,
              value: widget.settings.followActiveTurn,
              onChanged: (value) =>
                  _save(widget.settings.copyWith(followActiveTurn: value)),
            ),
            SettingsToggleRow(
              icon: Icons.view_agenda_outlined,
              title: context.l10n.settingsCompactTimeline,
              subtitle: context.l10n.settingsCompactTimelineSubtitle,
              value: widget.settings.compactTimeline,
              onChanged: (value) =>
                  _save(widget.settings.copyWith(compactTimeline: value)),
            ),
            WebSearchSettingsCard(settings: widget.webSearch),
            StudioUpdateSettingsRow(runtimeBusy: widget.runtimeBusy),
          ],
        ),
        if (_error != null) SettingsInlineError(message: _error!),
      ],
    );
  }

  Future<void> _save(GeneralSettingsView settings) async {
    try {
      setState(() => _error = null);
      await ref
          .read(studioControllerProvider.notifier)
          .saveGeneralSettings(
            GeneralSettingsCommand(
              followSystemTheme: settings.followSystemTheme,
              followActiveTurn: settings.followActiveTurn,
              compactTimeline: settings.compactTimeline,
            ),
          );
    } catch (error) {
      if (mounted) {
        setState(() => _error = error.toString());
      }
    }
  }
}
