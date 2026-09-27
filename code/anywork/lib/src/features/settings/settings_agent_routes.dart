import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/model_route_selector.dart';
import '../../shared/studio_driver_keys.dart';

class _AgentRouteConfiguration {
  const _AgentRouteConfiguration({
    required this.providers,
    required this.roles,
  });

  final List<ProviderSettingsView> providers;
  final List<RoleSettingsView> roles;

  Widget _buildRoleRow(
    BuildContext context,
    WidgetRef ref,
    String role,
    List<ModelSelectionOption> options,
  ) {
    final configuredRole = roles
        .where((candidate) => candidate.key == role)
        .firstOrNull;
    final hasConfiguredRoute =
        configuredRole != null &&
        (configuredRole.providerId.isNotEmpty ||
            configuredRole.model.isNotEmpty);
    final option = hasConfiguredRoute
        ? options
              .where(
                (candidate) =>
                    candidate.providerId == configuredRole.providerId &&
                    candidate.model == configuredRole.model,
              )
              .firstOrNull
        : options.firstOrNull;
    final routeUnavailable = hasConfiguredRoute && option == null;
    final unresolvedLabel = routeUnavailable
        ? context.l10n.settingsAgentRouteUnavailable(
            [
              configuredRole.providerId,
              configuredRole.model,
            ].where((part) => part.isNotEmpty).join(' / '),
          )
        : option == null
        ? context.l10n.settingsDefaultModel
        : null;
    final selectedProviderId =
        option?.providerId ??
        (hasConfiguredRoute ? configuredRole.providerId : 'default');
    final selectedModel =
        option?.model ??
        (hasConfiguredRoute ? configuredRole.model : 'default');
    final canonicalEffort = configuredRole?.effort;
    final efforts = option?.reasoningEfforts ?? const <String>[];
    final defaultEffort = option == null
        ? null
        : option.defaultReasoningEffort.isNotEmpty
        ? option.defaultReasoningEffort
        : option.reasoningEfforts.firstOrNull;
    final selectedEffort =
        hasConfiguredRoute &&
            option != null &&
            canonicalEffort != null &&
            efforts.contains(canonicalEffort)
        ? canonicalEffort
        : defaultEffort;

    return _RoleSettingsRow(
      role: role,
      selectedProviderId: selectedProviderId,
      selectedModel: selectedModel,
      selectedEffort: selectedEffort,
      options: options,
      efforts: efforts,
      unresolvedLabel: unresolvedLabel,
      showUnresolvedOption: routeUnavailable,
      onModelChanged: (value) {
        // 重选当前 provider/model 不产生任何变更。
        if (value.providerId == selectedProviderId &&
            value.model == selectedModel) {
          return;
        }
        ref
            .read(studioControllerProvider.notifier)
            .setModelRole(
              roleKey: role,
              providerId: value.providerId,
              model: value.model,
              effort: value.defaultReasoningEffort.isNotEmpty
                  ? value.defaultReasoningEffort
                  : value.reasoningEfforts.firstOrNull,
            );
      },
      onEffortChanged: (value) {
        if (option == null) return;
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

  List<ModelSelectionOption> _roleModelOptions(
    List<ProviderSettingsView> providers,
  ) {
    return buildModelSelectionOptions(
      providers,
      modelsForProvider: (provider) {
        final models = provider.models.isEmpty
            ? [
                ProviderModelView(
                  slug: provider.defaultModel,
                  displayName: provider.defaultModel,
                  reasoningEfforts: const [],
                ),
              ]
            : provider.models;
        return models.where((model) => model.slug.isNotEmpty).toList();
      },
    );
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
    final options = section._roleModelOptions(providers);
    return section._buildRoleRow(context, ref, role, options);
  }
}

class _RoleSettingsRow extends StatelessWidget {
  const _RoleSettingsRow({
    required this.role,
    required this.selectedProviderId,
    required this.selectedModel,
    required this.selectedEffort,
    required this.options,
    required this.efforts,
    required this.unresolvedLabel,
    required this.showUnresolvedOption,
    required this.onModelChanged,
    required this.onEffortChanged,
  });

  final String role;
  final String selectedProviderId;
  final String selectedModel;
  final String? selectedEffort;
  final List<ModelSelectionOption> options;
  final List<String> efforts;
  final String? unresolvedLabel;
  final bool showUnresolvedOption;
  final ValueChanged<ModelSelectionOption> onModelChanged;
  final ValueChanged<String> onEffortChanged;

  @override
  Widget build(BuildContext context) {
    final modelSelector = ModelRouteSelector(
      selectorKey: StudioDriverKeys.settingsRoleModel(role),
      fieldLabel: context.l10n.settingsModelField,
      options: options,
      providerId: selectedProviderId,
      model: selectedModel,
      unresolvedLabel: unresolvedLabel,
      showUnresolvedOption: showUnresolvedOption,
      enabled: options.isNotEmpty,
      onSelected: onModelChanged,
      optionKeyBuilder: (option) => StudioDriverKeys.settingsRoleModelOption(
        role,
        option.providerId,
        option.model,
      ),
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
