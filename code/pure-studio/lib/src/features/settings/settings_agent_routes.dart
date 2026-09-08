import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_model_labels.dart';

class _AgentRouteConfiguration {
  const _AgentRouteConfiguration({
    required this.providers,
    required this.roles,
  });

  final List<ProviderSettingsView> providers;
  final List<RoleSettingsView> roles;

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
    BuildContext context,
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
        final modalities = model.inputCapabilities
            .map((capability) => context.modalityLabel(capability.modality))
            .join('/');
        options.add(
          _RoleModelOption(
            providerId: provider.id,
            model: model.slug,
            label: [
              '${provider.name} / ${model.displayName.isEmpty ? model.slug : model.displayName}',
              if (modalities.isNotEmpty) modalities,
              modelProtocolLabel(model.wireProtocol),
              modelConnectionLabel(model.connectionMode),
            ].join(' · '),
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

/// One built-in Agent route editor embedded in its canonical Agent card.
class AgentRouteControls extends ConsumerWidget {
  const AgentRouteControls({
    super.key,
    required this.role,
    required this.providers,
    required this.roles,
  });

  final String role;
  final List<ProviderSettingsView> providers;
  final List<RoleSettingsView> roles;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final section = _AgentRouteConfiguration(
      providers: providers,
      roles: roles,
    );
    final options = section._roleModelOptions(context, providers);
    return section._buildRoleRow(ref, role, options);
  }
}

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
                modelSelector,
                const SizedBox(height: 10),
                effortSelector,
              ],
            );
          }
          return Row(
            children: [
              Expanded(child: modelSelector),
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
