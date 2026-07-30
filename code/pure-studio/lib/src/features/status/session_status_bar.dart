import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../app/theme/studio_tokens.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/upward_popup_menu.dart';
import 'context_usage_readout.dart';
import 'status_bar_item.dart';
import 'status_detail_popover.dart';

class SessionStatusBar extends ConsumerWidget {
  const SessionStatusBar({
    this.workspace,
    this.view,
    this.showTodo = false,
    this.todoExpanded = false,
    this.onToggleTodo,
    super.key,
  }) : assert(workspace != null || view != null);

  final AgentWorkspaceView? workspace;
  final StatusBarView? view;
  final bool showTodo;
  final bool todoExpanded;
  final VoidCallback? onToggleTodo;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final workspace = view ?? StatusBarView.fromWorkspace(this.workspace!);
    final runtime = workspace.runtime;
    final session = workspace.session;
    final capabilityLabel = _runtimeCapabilityLabel(context, runtime);
    return DecoratedBox(
      decoration: BoxDecoration(
        color: context.studioPaper,
        border: Border(top: BorderSide(color: context.studioLine)),
      ),
      child: SizedBox(
        height: 38,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12),
          child: Align(
            alignment: Alignment.center,
            child: LayoutBuilder(
              builder: (context, constraints) {
                final showModel = constraints.maxWidth >= 610;
                final showEffort = constraints.maxWidth >= 720;
                final showCost = constraints.maxWidth >= 790;
                final showCapabilities = constraints.maxWidth >= 840;
                final hasOverflow =
                    (!showEffort &&
                        session.isRoot &&
                        _effortsForWorkspace(
                          workspace,
                          session.mode,
                        ).isNotEmpty) ||
                    (!showCost && runtime.costLabel.isNotEmpty) ||
                    (!showCapabilities && capabilityLabel.isNotEmpty);
                return Row(
                  children: [
                    if (showTodo)
                      IconButton(
                        key: const ValueKey('todo-open-button'),
                        tooltip: context.l10n.timelineTodoListFallback,
                        visualDensity: VisualDensity.compact,
                        constraints: const BoxConstraints.tightFor(
                          width: 32,
                          height: 32,
                        ),
                        iconSize: 18,
                        icon: Icon(
                          todoExpanded
                              ? Icons.checklist
                              : Icons.checklist_outlined,
                          color: todoExpanded
                              ? StudioColors.clayDeep
                              : context.studioInkSoft,
                        ),
                        onPressed: onToggleTodo,
                      ),
                    _StatusReadout(
                      icon: Icons.account_tree_outlined,
                      label: session.ownerRole.isEmpty
                          ? (session.isRoot ? 'planner' : 'agent')
                          : session.ownerRole,
                      tooltip: session.agentStatus.isEmpty
                          ? session.title
                          : '${session.title} · ${session.agentStatus}',
                      maxWidth: 96,
                    ),
                    if (session.isRoot)
                      _SessionModeSelector(
                        mode: session.mode,
                        enabled: !workspace.isBusy && !runtime.hasActiveTask,
                      ),
                    if (showModel &&
                        session.isRoot &&
                        workspace.providers.isNotEmpty)
                      _ModeModelSelector(
                        workspace: workspace,
                        mode: session.mode,
                      )
                    else if (showModel &&
                        session.isAgent &&
                        runtime.model.isNotEmpty)
                      _StatusReadout(
                        icon: Icons.smart_toy_outlined,
                        label: runtime.model,
                        tooltip: runtime.model,
                        maxWidth: 140,
                      ),
                    if (showEffort &&
                        session.isRoot &&
                        _effortsForWorkspace(
                          workspace,
                          session.mode,
                        ).isNotEmpty)
                      _ReasoningEffortSelector(
                        workspace: workspace,
                        mode: session.mode,
                      ),
                    ContextUsageReadout(runtime: runtime),
                    if (showCost && runtime.costLabel.isNotEmpty)
                      _StatusReadout(
                        label: runtime.costLabel,
                        tooltip: context.l10n.statusCost,
                        detailWidth: 260,
                        detailBuilder: (context) =>
                            _CostDetail(runtime: runtime),
                      ),
                    if (showCapabilities && capabilityLabel.isNotEmpty)
                      _StatusReadout(
                        icon: Icons.tune_outlined,
                        label: capabilityLabel,
                        tooltip: context.l10n.statusCapabilitiesTitle,
                        detailWidth: 520,
                        maxWidth: 160,
                        interactive: true,
                        detailBuilder: (context) =>
                            _CapabilityDetail(runtime: runtime),
                      ),
                    if (hasOverflow)
                      _StatusOverflow(
                        effort: session.isAgent
                            ? null
                            : _selectedEffort(workspace, session.mode),
                        cost: runtime.costLabel,
                        capabilities: capabilityLabel,
                        runtime: runtime,
                      ),
                    const Spacer(),
                    const SizedBox(width: 8),
                  ],
                );
              },
            ),
          ),
        ),
      ),
    );
  }
}

String _runtimeCapabilityLabel(
  BuildContext context,
  SessionRuntimeView runtime,
) {
  final parts = [
    if (runtime.activeSkills.isNotEmpty)
      context.l10n.statusSkillsCount(runtime.activeSkills.length),
    if (runtime.activeMcpServers.isNotEmpty)
      context.l10n.statusMcpCount(runtime.activeMcpServers.length),
    if (runtime.activeLspServers.isNotEmpty)
      context.l10n.statusLspCount(runtime.activeLspServers.length),
  ];
  return parts.join(' · ');
}

String? _selectedEffort(StatusBarView workspace, StudioMode mode) {
  final effort = workspace.role(_roleKeyForMode(mode))?.effort.trim() ?? '';
  return effort.isEmpty ? null : effort;
}

class _StatusOverflow extends StatelessWidget {
  const _StatusOverflow({
    required this.effort,
    required this.cost,
    required this.capabilities,
    required this.runtime,
  });

  final String? effort;
  final String cost;
  final String capabilities;
  final SessionRuntimeView runtime;

  @override
  Widget build(BuildContext context) {
    return PopupMenuButton<_StatusOverflowAction>(
      key: const ValueKey('status-overflow'),
      tooltip: MaterialLocalizations.of(context).moreButtonTooltip,
      position: PopupMenuPosition.over,
      icon: const Icon(Icons.more_horiz, size: 18),
      padding: EdgeInsets.zero,
      constraints: const BoxConstraints.tightFor(width: 32, height: 32),
      onSelected: (action) => _showDetail(context, action),
      itemBuilder: (context) => [
        if (effort case final value?)
          PopupMenuItem<_StatusOverflowAction>(
            enabled: false,
            child: Text(
              '${context.l10n.statusReasoningEffort}: $value',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          ),
        if (cost.isNotEmpty)
          PopupMenuItem<_StatusOverflowAction>(
            value: _StatusOverflowAction.cost,
            child: Text(
              '${context.l10n.statusCost}: $cost',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          ),
        if (capabilities.isNotEmpty)
          PopupMenuItem<_StatusOverflowAction>(
            value: _StatusOverflowAction.capabilities,
            child: Text(
              capabilities,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
            ),
          ),
      ],
    );
  }

  void _showDetail(BuildContext context, _StatusOverflowAction action) {
    final detail = switch (action) {
      _StatusOverflowAction.cost => _CostDetail(runtime: runtime),
      _StatusOverflowAction.capabilities => _CapabilityDetail(runtime: runtime),
    };
    showDialog<void>(
      context: context,
      builder: (context) => Dialog(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 560, maxHeight: 520),
          child: Padding(padding: const EdgeInsets.all(16), child: detail),
        ),
      ),
    );
  }
}

enum _StatusOverflowAction { cost, capabilities }

class _SessionModeSelector extends ConsumerWidget {
  const _SessionModeSelector({required this.mode, required this.enabled});

  final StudioMode mode;
  final bool enabled;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return UpwardPopupMenu<StudioMode>(
      tooltip: context.l10n.statusSessionMode,
      enabled: enabled,
      initialValue: mode,
      onSelected: ref.read(studioControllerProvider.notifier).setSessionMode,
      itemBuilder: (context) => [
        for (final option in StudioMode.values)
          PopupMenuItem(
            value: option,
            child: Row(
              children: [
                Icon(_modeIcon(option), size: 18),
                const SizedBox(width: 10),
                Text(context.compileModeLabel(option)),
              ],
            ),
          ),
      ],
      child: _ControlItem(
        label: context.compileModeLabel(mode),
        enabled: enabled,
      ),
    );
  }

  IconData _modeIcon(StudioMode value) {
    return switch (value) {
      StudioMode.simple => Icons.flash_on,
      StudioMode.task => Icons.route_outlined,
    };
  }
}

class _ModeModelSelector extends ConsumerWidget {
  const _ModeModelSelector({required this.workspace, required this.mode});

  final StatusBarView workspace;
  final StudioMode mode;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final options = _modelOptions(workspace.providers);
    if (options.isEmpty) {
      return const SizedBox.shrink();
    }
    final roleKey = _roleKeyForMode(mode);
    final role = workspace.role(roleKey);
    final current = _modelFor(workspace, mode) ?? options.first;
    return UpwardPopupMenu<String>(
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
            value: option.key,
            child: SizedBox(
              width: 260,
              child: Row(
                children: [
                  const Icon(Icons.smart_toy_outlined, size: 18),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Text(option.label, overflow: TextOverflow.ellipsis),
                  ),
                ],
              ),
            ),
          ),
      ],
      child: _ControlItem(label: current.model, enabled: true),
    );
  }
}

class _ReasoningEffortSelector extends ConsumerWidget {
  const _ReasoningEffortSelector({required this.workspace, required this.mode});

  final StatusBarView workspace;
  final StudioMode mode;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final roleKey = _roleKeyForMode(mode);
    final role = workspace.role(roleKey);
    final currentModel = _modelFor(workspace, mode);
    final efforts = currentModel?.reasoningEfforts ?? const [];
    if (role == null || currentModel == null || efforts.isEmpty) {
      return const SizedBox.shrink();
    }
    final current = efforts.contains(role.effort) ? role.effort : efforts.first;
    return UpwardPopupMenu<String>(
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

class _ModeModelOption {
  const _ModeModelOption({
    required this.providerId,
    required this.model,
    required this.label,
    required this.reasoningEfforts,
  });

  final String providerId;
  final String model;
  final String label;
  final List<String> reasoningEfforts;

  String get key => '$providerId::$model';
}

String _roleKeyForMode(StudioMode mode) {
  return switch (mode) {
    StudioMode.simple => 'executor',
    StudioMode.task => 'planner',
  };
}

_ModeModelOption? _modelFor(StatusBarView workspace, StudioMode mode) {
  final role = workspace.role(_roleKeyForMode(mode));
  if (role == null) {
    return null;
  }
  final options = _modelOptions(workspace.providers);
  if (options.isEmpty) {
    return null;
  }
  return options.firstWhere(
    (option) =>
        option.providerId == role.providerId && option.model == role.model,
    orElse: () => options.first,
  );
}

List<String> _effortsForWorkspace(StatusBarView workspace, StudioMode mode) {
  return _modelFor(workspace, mode)?.reasoningEfforts ?? const [];
}

List<_ModeModelOption> _modelOptions(List<ProviderSettingsView> providers) {
  final options = <_ModeModelOption>[];
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
        _ModeModelOption(
          providerId: provider.id,
          model: model.slug,
          label:
              '${provider.name} / ${model.displayName.isEmpty ? model.slug : model.displayName}',
          reasoningEfforts: model.reasoningEfforts,
        ),
      );
    }
  }
  return options;
}

class _StatusReadout extends StatelessWidget {
  const _StatusReadout({
    required this.label,
    required this.tooltip,
    this.icon,
    this.detailBuilder,
    this.detailWidth = 300,
    this.maxWidth = 180,
    this.interactive = false,
  });

  final IconData? icon;
  final String label;
  final String tooltip;
  final WidgetBuilder? detailBuilder;
  final double detailWidth;
  final double maxWidth;
  final bool interactive;

  @override
  Widget build(BuildContext context) {
    return StatusBarItem(
      icon: icon,
      label: label,
      tooltip: detailBuilder == null
          ? (tooltip.isEmpty ? label : tooltip)
          : null,
      detailBuilder: detailBuilder,
      detailWidth: detailWidth,
      enableHover: detailBuilder == null,
      interactive: interactive,
      maxWidth: maxWidth,
    );
  }
}

class _CostDetail extends StatelessWidget {
  const _CostDetail({required this.runtime});

  final SessionRuntimeView runtime;

  @override
  Widget build(BuildContext context) {
    return StatusDetailPanel(
      title: context.l10n.statusCostDetailTitle,
      children: [
        Text(
          runtime.costLabel,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: context.text.headlineSmall?.copyWith(
            color: StudioColors.clayDeep,
            fontWeight: FontWeight.w500,
          ),
        ),
        const SizedBox(height: 8),
        StatusDetailRow(
          label: context.l10n.statusTotalTokensLabel,
          value: _formatStatusCount(runtime.totalTokens),
        ),
      ],
    );
  }
}

class _CapabilityDetail extends StatelessWidget {
  const _CapabilityDetail({required this.runtime});

  final SessionRuntimeView runtime;

  @override
  Widget build(BuildContext context) {
    return StatusDetailPanel(
      title: context.l10n.statusCapabilitiesTitle,
      children: [
        if (runtime.activeSkills.isNotEmpty)
          StatusDetailIconRow(
            icon: Icons.extension_outlined,
            title: context.l10n.statusSkillsSection,
            detail: runtime.activeSkills.join(', '),
            iconColor: StudioColors.clayDeep,
            backgroundColor: StudioColors.claySoft,
          ),
        if (runtime.activeMcpServers.isNotEmpty)
          StatusDetailIconRow(
            icon: Icons.hub_outlined,
            title: context.l10n.statusMcpSection,
            detail: runtime.activeMcpServers.join(', '),
            iconColor: StudioColors.sage,
            backgroundColor: StudioColors.sageSoft,
          ),
        if (runtime.activeLspServers.isNotEmpty)
          StatusDetailIconRow(
            icon: Icons.terminal_outlined,
            title: context.l10n.statusLspSection,
            detail: runtime.activeLspServers.join(', '),
            iconColor: StudioColors.ochre,
            backgroundColor: StudioColors.ochre.withValues(alpha: 0.15),
          ),
      ],
    );
  }
}

String _formatStatusCount(int value) {
  final text = value.toString();
  final buffer = StringBuffer();
  for (var index = 0; index < text.length; index++) {
    if (index > 0 && (text.length - index) % 3 == 0) {
      buffer.write(',');
    }
    buffer.write(text[index]);
  }
  return buffer.toString();
}
