import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import '../../shared/upward_popup_menu.dart';
import 'status_bar_item.dart';

/// 根会话的模式选择器；起始页传新会话草稿，状态栏传当前 Thread 的 mode。
class SessionModeSelector extends StatelessWidget {
  const SessionModeSelector({
    required this.mode,
    required this.onSelected,
    this.enabled = true,
    super.key,
  });

  final StudioMode mode;
  final ValueChanged<StudioMode> onSelected;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    return UpwardPopupMenu<StudioMode>(
      key: StudioDriverKeys.sessionMode,
      tooltip: enabled
          ? context.l10n.statusSessionMode
          : context.l10n.statusSessionModeLocked,
      initialValue: mode,
      enabled: enabled,
      onSelected: onSelected,
      itemBuilder: (context) => [
        for (final option in StudioMode.values)
          PopupMenuItem<StudioMode>(
            key: StudioDriverKeys.sessionModeOption(option.name),
            value: option,
            child: Row(
              children: [
                Icon(sessionModeIcon(option), size: 18),
                const SizedBox(width: 10),
                Text(context.compileModeLabel(option)),
              ],
            ),
          ),
      ],
      child: StatusBarItem(
        icon: sessionModeIcon(mode),
        label: context.compileModeLabel(mode),
        enabled: enabled,
        trailingIcon: enabled ? Icons.keyboard_arrow_down : Icons.lock_outline,
        maxWidth: 96,
      ),
    );
  }
}

/// 按模式映射的 role（简单→executor、任务→planner）选择模型；写 role 级 Settings 配置。
class ModelRoleSelector extends ConsumerWidget {
  const ModelRoleSelector({
    required this.providers,
    required this.roles,
    required this.mode,
    super.key,
  });

  final List<ProviderSettingsView> providers;
  final List<RoleSettingsView> roles;
  final StudioMode mode;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final options = modelOptions(providers);
    if (options.isEmpty) {
      return const SizedBox.shrink();
    }
    final roleKey = roleKeyForMode(mode);
    final role = roleByKey(roles, roleKey);
    final current = modelFor(providers, roles, mode) ?? options.first;
    return UpwardPopupMenu<String>(
      key: StudioDriverKeys.model,
      tooltip: mode == StudioMode.task
          ? context.l10n.statusPlannerModel
          : context.l10n.statusExecutorModel,
      initialValue: current.key,
      onSelected: (key) {
        final option = options.firstWhere((option) => option.key == key);
        final effort = option.reasoningEfforts.contains(role?.effort)
            ? role?.effort
            : option.reasoningEfforts.firstOrNull;
        ref
            .read(studioControllerProvider.notifier)
            .setModelRole(
              roleKey: roleKey,
              providerId: option.providerId,
              model: option.model,
              effort: effort,
            );
      },
      itemBuilder: (context) => [
        for (final option in options)
          PopupMenuItem(
            key: StudioDriverKeys.modelOption(option.providerId, option.model),
            value: option.key,
            child: SizedBox(
              width: 260,
              child: Row(
                children: [
                  const Icon(Icons.smart_toy_outlined, size: 18),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(option.label, overflow: TextOverflow.ellipsis),
                        if (option.inputModalities.isNotEmpty)
                          Text(
                            option.inputModalities
                                .map(modelInputCapabilityLabel)
                                .join(' · '),
                            key: StudioDriverKeys.modelCapabilityTags(
                              option.providerId,
                              option.model,
                            ),
                            style: Theme.of(context).textTheme.labelSmall,
                          ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
      ],
      child: _ControlItem(
        label: [
          current.model,
          ...current.inputModalities.map(modelInputCapabilityLabel),
        ].join(' · '),
        enabled: true,
      ),
    );
  }
}

/// 当前模式对应 role 模型的思考等级选择器；写 role 级 Settings 配置。
class ReasoningEffortSelector extends ConsumerWidget {
  const ReasoningEffortSelector({
    required this.providers,
    required this.roles,
    required this.mode,
    super.key,
  });

  final List<ProviderSettingsView> providers;
  final List<RoleSettingsView> roles;
  final StudioMode mode;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final roleKey = roleKeyForMode(mode);
    final role = roleByKey(roles, roleKey);
    final currentModel = modelFor(providers, roles, mode);
    final efforts = currentModel?.reasoningEfforts ?? const [];
    if (role == null || currentModel == null || efforts.isEmpty) {
      return const SizedBox.shrink();
    }
    final current = efforts.contains(role.effort) ? role.effort : efforts.first;
    return UpwardPopupMenu<String>(
      key: StudioDriverKeys.reasoningEffort,
      tooltip: context.l10n.statusReasoningEffort,
      initialValue: current,
      onSelected: (effort) {
        ref
            .read(studioControllerProvider.notifier)
            .setModelRole(
              roleKey: roleKey,
              providerId: role.providerId,
              model: role.model,
              effort: effort,
            );
      },
      itemBuilder: (context) => [
        for (final effort in efforts)
          PopupMenuItem(
            key: StudioDriverKeys.reasoningEffortOption(effort),
            value: effort,
            child: Row(
              children: [
                const Icon(Icons.schedule_outlined, size: 18),
                const SizedBox(width: 10),
                Text(effort),
              ],
            ),
          ),
      ],
      child: _ControlItem(label: current, enabled: true),
    );
  }
}

class _ControlItem extends StatelessWidget {
  const _ControlItem({required this.label, required this.enabled});

  final String label;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    return StatusBarItem(
      label: label,
      enabled: enabled,
      maxWidth: 140,
      trailingIcon: Icons.keyboard_arrow_down,
    );
  }
}

IconData sessionModeIcon(StudioMode mode) {
  return switch (mode) {
    StudioMode.simple => Icons.flash_on,
    StudioMode.task => Icons.route_outlined,
  };
}

String roleKeyForMode(StudioMode mode) {
  return switch (mode) {
    StudioMode.simple => 'executor',
    StudioMode.task => 'planner',
  };
}

RoleSettingsView? roleByKey(List<RoleSettingsView> roles, String key) {
  return roles.where((role) => role.key == key).firstOrNull;
}

class ModelOption {
  const ModelOption({
    required this.providerId,
    required this.model,
    required this.label,
    required this.reasoningEfforts,
    required this.inputCapabilities,
  });

  final String providerId;
  final String model;
  final String label;
  final List<String> reasoningEfforts;
  final List<ModelInputCapabilityView> inputCapabilities;

  List<ModelModalityView> get inputModalities =>
      inputCapabilities.map((capability) => capability.modality).toList();

  String get key => '$providerId::$model';
}

ModelOption? modelFor(
  List<ProviderSettingsView> providers,
  List<RoleSettingsView> roles,
  StudioMode mode,
) {
  final role = roleByKey(roles, roleKeyForMode(mode));
  if (role == null) {
    return null;
  }
  final options = modelOptions(providers);
  if (options.isEmpty) {
    return null;
  }
  return options.firstWhere(
    (option) =>
        option.providerId == role.providerId && option.model == role.model,
    orElse: () => options.first,
  );
}

List<String> effortsFor(
  List<ProviderSettingsView> providers,
  List<RoleSettingsView> roles,
  StudioMode mode,
) {
  return modelFor(providers, roles, mode)?.reasoningEfforts ?? const [];
}

List<ModelOption> modelOptions(List<ProviderSettingsView> providers) {
  final options = <ModelOption>[];
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
        ModelOption(
          providerId: provider.id,
          model: model.slug,
          label:
              '${provider.name} / ${model.displayName.isEmpty ? model.slug : model.displayName}',
          reasoningEfforts: model.reasoningEfforts,
          inputCapabilities: model.inputCapabilities,
        ),
      );
    }
  }
  return options;
}

String modelInputCapabilityLabel(ModelModalityView modality) =>
    switch (modality) {
      ModelModalityView.text => '文本',
      ModelModalityView.image => '视觉',
      ModelModalityView.audio => '音频',
      ModelModalityView.video => '视频',
      ModelModalityView.file => '文件',
    };
