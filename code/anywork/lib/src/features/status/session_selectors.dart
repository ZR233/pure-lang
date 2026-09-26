import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import '../../shared/upward_popup_menu.dart';

/// 根会话的模式选择器；起始页传新会话草稿，状态栏传当前 Thread 的 mode。
class SessionModeSelector extends ConsumerWidget {
  const SessionModeSelector({
    required this.mode,
    required this.onSelected,
    this.enabled = true,
    super.key,
  });

  final ThreadModeId mode;
  final ValueChanged<ThreadModeId> onSelected;
  final bool enabled;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final state = ref.watch(studioControllerProvider).value;
    final discovered =
        state?.threadModeCatalog.modes ?? const <ThreadModeDescriptorView>[];
    final options = discovered.isEmpty
        ? ThreadModeId.values
        : discovered.map((descriptor) => descriptor.mode).toList();
    if (!options.contains(mode)) options.add(mode);
    final selectedLabel = discovered
        .where((descriptor) => descriptor.id == mode.id)
        .firstOrNull
        ?.displayName;
    return UpwardPopupMenu<ThreadModeId>(
      key: StudioDriverKeys.sessionMode,
      tooltip: enabled
          ? context.l10n.statusSessionMode
          : context.l10n.statusSessionModeLocked,
      initialValue: mode,
      enabled: enabled,
      onSelected: onSelected,
      itemBuilder: (context) => [
        for (final option in options)
          PopupMenuItem<ThreadModeId>(
            key: StudioDriverKeys.sessionModeOption(option.name),
            value: option,
            child: Row(
              children: [
                Icon(sessionModeIcon(option), size: 18),
                const SizedBox(width: 10),
                Text(
                  discovered
                          .where((descriptor) => descriptor.id == option.id)
                          .firstOrNull
                          ?.displayName ??
                      context.compileModeLabel(option),
                ),
              ],
            ),
          ),
      ],
      child: StudioMenuLabel(
        label: selectedLabel ?? context.compileModeLabel(mode),
        enabled: enabled,
        maxWidth: 96,
      ),
    );
  }
}

/// 起始页会话工作区选择器：`local` 使用 Project 目录，`worktree` 新建 Git 工作树。
///
/// 选择属于当前项目的输入草稿，只通过创建命令传给运行时；已有的 Thread 工作区模式
/// 只读来自 canonical 目录事实，因此该控件不出现在已建会话的 composer 中。本地与 SSH
/// 项目都提供 `worktree`：非 Git 项目、无 `HEAD` 或远端当前不可用在提交时以类型化错误
/// 就地反馈，GUI 不做可用性推断。
class SessionWorkspaceModeSelector extends ConsumerWidget {
  const SessionWorkspaceModeSelector({
    required this.mode,
    required this.onSelected,
    super.key,
  });

  final ThreadWorkspaceMode mode;
  final ValueChanged<ThreadWorkspaceMode> onSelected;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    return UpwardPopupMenu<ThreadWorkspaceMode>(
      key: StudioDriverKeys.sessionWorkspaceMode,
      tooltip: l10n.composerWorkspaceModeLabel,
      initialValue: mode,
      onSelected: onSelected,
      itemBuilder: (context) => [
        PopupMenuItem<ThreadWorkspaceMode>(
          key: StudioDriverKeys.sessionWorkspaceModeOption(
            ThreadWorkspaceMode.local.id,
          ),
          value: ThreadWorkspaceMode.local,
          child: _WorkspaceModeItem(
            icon: Icons.folder_outlined,
            label: l10n.composerWorkspaceModeLocal,
          ),
        ),
        PopupMenuItem<ThreadWorkspaceMode>(
          key: StudioDriverKeys.sessionWorkspaceModeOption(
            ThreadWorkspaceMode.worktree.id,
          ),
          value: ThreadWorkspaceMode.worktree,
          child: _WorkspaceModeItem(
            icon: Icons.account_tree_outlined,
            label: l10n.composerWorkspaceModeWorktree,
          ),
        ),
      ],
      child: StudioMenuLabel(label: _label(context), maxWidth: 96),
    );
  }

  String _label(BuildContext context) => mode.isWorktree
      ? context.l10n.composerWorkspaceModeWorktree
      : context.l10n.composerWorkspaceModeLocal;
}

class _WorkspaceModeItem extends StatelessWidget {
  const _WorkspaceModeItem({required this.icon, required this.label});

  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: 260,
      child: Row(
        children: [
          Icon(icon, size: 18),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [Text(label, overflow: TextOverflow.ellipsis)],
            ),
          ),
        ],
      ),
    );
  }
}

typedef ModelRouteChanged = void Function(
  String providerId,
  String model,
  String? effort,
);

/// 根会话或新建会话草稿的模型选择器。
class ModelRoleSelector extends StatelessWidget {
  const ModelRoleSelector({
    required this.providers,
    required this.providerId,
    required this.model,
    required this.effort,
    required this.onSelected,
    this.available = true,
    this.unavailableReason,
    this.blockedReason,
    this.onExplain,
    super.key,
  });

  final List<ProviderSettingsView> providers;
  final String providerId;
  final String model;
  final String? effort;
  final ModelRouteChanged onSelected;
  final bool available;
  final String? unavailableReason;
  final String? blockedReason;
  final ValueChanged<String>? onExplain;

  @override
  Widget build(BuildContext context) {
    final options = modelOptions(providers);
    final current = modelForRoute(providers, providerId, model);
    final selectedKey = current?.key;
    final warning = !available || current == null;
    final selectionBlockedReason =
        blockedReason ??
        (options.isEmpty
            ? context.l10n.statusModelRouteUnavailableFallback
            : null);
    void explain(String message) {
      if (onExplain case final callback?) {
        callback(message);
      } else {
        final messenger = ScaffoldMessenger.of(context);
        messenger.hideCurrentSnackBar();
        messenger.showSnackBar(
          SnackBar(content: Text(message), showCloseIcon: true),
        );
      }
    }

    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (warning)
          SizedBox.square(
            dimension: 32,
            child: IconButton(
              key: const ValueKey('model-route-warning'),
              padding: EdgeInsets.zero,
              tooltip: context.l10n.statusModelRouteUnavailable,
              onPressed: () => explain(
                unavailableReason?.isNotEmpty == true
                    ? context.l10n.statusModelRouteUnavailableDetail(
                        unavailableReason!,
                      )
                    : context.l10n.statusModelRouteUnavailableFallback,
              ),
              icon: Icon(
                Icons.warning_amber_outlined,
                size: 15,
                color: Theme.of(context).colorScheme.error,
              ),
            ),
          ),
        UpwardPopupMenu<String>(
          key: StudioDriverKeys.model,
          tooltip: selectionBlockedReason ?? context.l10n.statusPlannerModel,
          initialValue: selectedKey,
          enabled: selectionBlockedReason == null,
          onBlockedTap: selectionBlockedReason == null
              ? null
              : () => explain(selectionBlockedReason),
          onSelected: (key) {
            final option = options.firstWhere((option) => option.key == key);
            final nextEffort = option.reasoningEfforts.contains(effort)
                ? effort
                : option.reasoningEfforts.firstOrNull;
            onSelected(option.providerId, option.model, nextEffort);
          },
          itemBuilder: (context) => [
            for (final option in options)
              PopupMenuItem(
                key: StudioDriverKeys.modelOption(
                  option.providerId,
                  option.model,
                ),
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
                                    .map(context.modalityLabel)
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
            label: model,
            enabled: selectionBlockedReason == null,
          ),
        ),
      ],
    );
  }
}

/// 根会话或 Mode 默认路由的思考强度选择器。
class ReasoningEffortSelector extends StatelessWidget {
  const ReasoningEffortSelector({
    required this.providers,
    required this.providerId,
    required this.model,
    required this.effort,
    required this.onSelected,
    this.blockedReason,
    this.onExplain,
    super.key,
  });

  final List<ProviderSettingsView> providers;
  final String providerId;
  final String model;
  final String? effort;
  final ModelRouteChanged onSelected;
  final String? blockedReason;
  final ValueChanged<String>? onExplain;

  @override
  Widget build(BuildContext context) {
    final currentModel = modelForRoute(providers, providerId, model);
    final efforts = currentModel?.reasoningEfforts ?? const [];
    if (currentModel == null || efforts.isEmpty) {
      return const SizedBox.shrink();
    }
    final current = efforts.contains(effort) ? effort! : efforts.first;
    return UpwardPopupMenu<String>(
      key: StudioDriverKeys.reasoningEffort,
      tooltip: blockedReason ?? context.l10n.statusReasoningEffort,
      initialValue: current,
      enabled: blockedReason == null,
      onBlockedTap: blockedReason == null
          ? null
          : () => onExplain?.call(blockedReason!),
      onSelected: (nextEffort) => onSelected(providerId, model, nextEffort),
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
      child: _ControlItem(label: current, enabled: blockedReason == null),
    );
  }
}

class _ControlItem extends StatelessWidget {
  const _ControlItem({required this.label, required this.enabled});

  final String label;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        StudioMenuLabel(label: label, enabled: enabled, maxWidth: 140),
      ],
    );
  }
}

IconData sessionModeIcon(ThreadModeId mode) {
  return mode == ThreadModeId.simple ? Icons.flash_on : Icons.route_outlined;
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

ModelOption? modelForRoute(
  List<ProviderSettingsView> providers,
  String providerId,
  String model,
) {
  final options = modelOptions(providers);
  return options
      .where(
        (option) => option.providerId == providerId && option.model == model,
      )
      .firstOrNull;
}

ModeModelRouteView? modeRouteFor(
  List<ModeModelRouteView> routes,
  ThreadModeId mode,
) {
  return routes.where((route) => route.modeId == mode).firstOrNull ??
      routes.where((route) => route.modeId == ThreadModeId.simple).firstOrNull;
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
