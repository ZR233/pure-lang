import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/model_route_selector.dart';
import '../../shared/studio_form_select.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';

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
    final storedEffort = configuredRole?.effort;
    final canonicalEffort = storedEffort?.isNotEmpty == true
        ? storedEffort
        : null;
    final efforts = option?.reasoningEfforts ?? const <String>[];
    final defaultEffort = option == null
        ? null
        : option.defaultReasoningEffort.isNotEmpty
        ? option.defaultReasoningEffort
        : option.reasoningEfforts.firstOrNull;
    final selectedEffort = hasConfiguredRoute ? canonicalEffort : defaultEffort;

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
        final models = provider.models;
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
    final modelSelector = Row(
      children: [
        Padding(
          padding: const EdgeInsets.only(right: 8),
          child: SettingsFieldLabel(label: context.l10n.settingsModelField),
        ),
        Flexible(
          child: ModelRouteSelector(
            selectorKey: StudioDriverKeys.settingsRoleModel(role),
            fieldLabel: context.l10n.settingsModelField,
            options: options,
            providerId: selectedProviderId,
            model: selectedModel,
            unresolvedLabel: unresolvedLabel,
            showUnresolvedOption: showUnresolvedOption,
            enabled: options.isNotEmpty,
            onSelected: onModelChanged,
            optionKeyBuilder: (option) =>
                StudioDriverKeys.settingsRoleModelOption(
                  role,
                  option.providerId,
                  option.model,
                ),
          ),
        ),
      ],
    );
    final effortUnresolvedLabel =
        selectedEffort != null && !efforts.contains(selectedEffort)
        ? context.l10n.settingsAgentRouteUnavailable(selectedEffort!)
        : selectedEffort == null && efforts.isNotEmpty
        ? context.l10n.statusModelRouteUnavailable
        : null;
    final effortSelector = StudioFormSelectField<String>(
      key: StudioDriverKeys.settingsRoleEffort(role),
      value: selectedEffort,
      hint: effortUnresolvedLabel == null ? null : Text(effortUnresolvedLabel),
      decoration: InputDecoration(
        labelText: context.l10n.statusReasoningEffort,
        isDense: true,
      ),
      items: [
        for (final effort in efforts)
          StudioFormSelectItem<String>(
            value: effort,
            itemKey: StudioDriverKeys.settingsRoleEffortOption(role, effort),
            child: Text(effort, overflow: TextOverflow.ellipsis),
          ),
      ],
      onChanged: efforts.isEmpty
          ? null
          : (effort) {
              if (effort != null) {
                onEffortChanged(effort);
              }
            },
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
