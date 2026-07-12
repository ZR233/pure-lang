import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../app/theme/studio_tokens.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/upward_popup_menu.dart';
import 'agent_detail_panel.dart';
import 'context_usage_readout.dart';
import 'status_bar_item.dart';
import 'status_detail_popover.dart';
import 'task_runtime_detail.dart';

class SessionStatusBar extends ConsumerWidget {
  const SessionStatusBar({required this.state, super.key});

  final StudioState state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final runtime = state.runtime;
    final selectedAgents = state.selectedAgents;
    final session = state.sessions
        .where((session) => session.id == state.selectedSessionId)
        .firstOrNull;
    final activityLabel = _runtimeActivityLabel(
      context,
      runtime,
      selectedAgents,
    );
    return DecoratedBox(
      decoration: BoxDecoration(
        color: context.studioPaper2,
        border: Border(top: BorderSide(color: context.studioLine)),
      ),
      child: SizedBox(
        height: 38,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12),
          child: Align(
            alignment: Alignment.center,
            child: ConstrainedBox(
              constraints: const BoxConstraints(
                maxWidth: StudioLayout.conversationWidth,
              ),
              child: Row(
                children: [
                  Expanded(
                    child: SingleChildScrollView(
                      scrollDirection: Axis.horizontal,
                      child: Row(
                        children: [
                          if (session != null)
                            _SessionModeSelector(
                              mode: session.mode,
                              enabled: !state.isBusy && !runtime.hasActiveTask,
                            ),
                          if (session != null && state.providers.isNotEmpty)
                            _ModeModelSelector(
                              state: state,
                              mode: session.mode,
                            ),
                          if (session != null &&
                              _effortsForState(state, session.mode).isNotEmpty)
                            _ReasoningEffortSelector(
                              state: state,
                              mode: session.mode,
                            ),
                          ContextUsageReadout(runtime: runtime),
                          if (runtime.costLabel.isNotEmpty)
                            _StatusReadout(
                              label: runtime.costLabel,
                              tooltip: context.l10n.statusCost,
                              detailWidth: 260,
                              detailBuilder: (context) =>
                                  _CostDetail(runtime: runtime),
                            ),
                          if (activityLabel.isNotEmpty)
                            _StatusReadout(
                              icon: Icons.tune_outlined,
                              label: activityLabel,
                              tooltip: context.l10n.statusCapabilitiesTitle,
                              detailWidth: 520,
                              maxWidth: 240,
                              interactive: true,
                              detailBuilder: (context) => _ActivityDetail(
                                runtime: runtime,
                                agents: selectedAgents,
                              ),
                            ),
                        ],
                      ),
                    ),
                  ),
                  const SizedBox(width: 8),
                  _PhaseReadout(
                    turnPhase: state.turnPhase,
                    interactionKind: state.activeInteraction?.kind,
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

String _runtimeActivityLabel(
  BuildContext context,
  SessionRuntimeView runtime,
  List<StudioAgentView> agents,
) {
  final taskAgents = runtime.task?.agents;
  final agentCount = taskAgents != null && taskAgents.isNotEmpty
      ? taskAgents.where((agent) => _isActiveTaskAgent(agent.status)).length
      : agents.where((agent) => _isActiveMemoryAgent(agent.status)).length;
  final parts = [
    if (runtime.task case final task?) context.taskPhaseLabel(task.phase),
    if (runtime.activeSkills.isNotEmpty)
      context.l10n.statusSkillsCount(runtime.activeSkills.length),
    if (runtime.activeMcpServers.isNotEmpty)
      context.l10n.statusMcpCount(runtime.activeMcpServers.length),
    if (runtime.activeLspServers.isNotEmpty)
      context.l10n.statusLspCount(runtime.activeLspServers.length),
    if (agentCount > 0) context.l10n.statusAgentsCount(agentCount),
  ];
  return parts.join(' · ');
}

bool _isActiveTaskAgent(String status) =>
    const {'queued', 'running', 'waitingForDelivery'}.contains(status);

bool _isActiveMemoryAgent(String status) =>
    const {'queued', 'running', 'waiting'}.contains(status);

class _SessionModeSelector extends ConsumerWidget {
  const _SessionModeSelector({required this.mode, required this.enabled});

  final CompileMode mode;
  final bool enabled;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return UpwardPopupMenu<CompileMode>(
      tooltip: context.l10n.statusSessionMode,
      enabled: enabled,
      initialValue: mode,
      onSelected: ref.read(studioControllerProvider.notifier).setSessionMode,
      itemBuilder: (context) => [
        for (final option in CompileMode.values)
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

  IconData _modeIcon(CompileMode value) {
    return switch (value) {
      CompileMode.simple => Icons.flash_on,
      CompileMode.task => Icons.route_outlined,
    };
  }
}

class _ModeModelSelector extends ConsumerWidget {
  const _ModeModelSelector({required this.state, required this.mode});

  final StudioState state;
  final CompileMode mode;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final options = _modelOptions(state.providers);
    if (options.isEmpty) {
      return const SizedBox.shrink();
    }
    final roleKey = _roleKeyForMode(mode);
    final role = state.role(roleKey);
    final current = _modelFor(state, mode) ?? options.first;
    return UpwardPopupMenu<String>(
      tooltip: mode == CompileMode.task
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
  const _ReasoningEffortSelector({required this.state, required this.mode});

  final StudioState state;
  final CompileMode mode;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final roleKey = _roleKeyForMode(mode);
    final role = state.role(roleKey);
    final currentModel = _modelFor(state, mode);
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

String _roleKeyForMode(CompileMode mode) {
  return switch (mode) {
    CompileMode.simple => 'executor',
    CompileMode.task => 'planner',
  };
}

_ModeModelOption? _modelFor(StudioState state, CompileMode mode) {
  final role = state.role(_roleKeyForMode(mode));
  if (role == null) {
    return null;
  }
  final options = _modelOptions(state.providers);
  if (options.isEmpty) {
    return null;
  }
  return options.firstWhere(
    (option) =>
        option.providerId == role.providerId && option.model == role.model,
    orElse: () => options.first,
  );
}

List<String> _effortsForState(StudioState state, CompileMode mode) {
  return _modelFor(state, mode)?.reasoningEfforts ?? const [];
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

class _ActivityDetail extends StatelessWidget {
  const _ActivityDetail({required this.runtime, required this.agents});

  final SessionRuntimeView runtime;
  final List<StudioAgentView> agents;

  @override
  Widget build(BuildContext context) {
    final hasCapabilities =
        runtime.activeSkills.isNotEmpty ||
        runtime.activeMcpServers.isNotEmpty ||
        runtime.activeLspServers.isNotEmpty;
    final task = runtime.task;
    if (task == null) {
      return Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (hasCapabilities) _CapabilityDetail(runtime: runtime),
          if (hasCapabilities && agents.isNotEmpty)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 12),
              child: Divider(height: 1, color: context.studioLine),
            ),
          if (agents.isNotEmpty) AgentDetailPanel(agents: agents),
        ],
      );
    }
    return ConstrainedBox(
      constraints: const BoxConstraints(maxHeight: 480),
      child: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TaskRuntimeDetail(task: task),
            if (hasCapabilities)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 12),
                child: Divider(height: 1, color: context.studioLine),
              ),
            if (hasCapabilities) _CapabilityDetail(runtime: runtime),
          ],
        ),
      ),
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

class _PhaseReadout extends StatelessWidget {
  const _PhaseReadout({required this.turnPhase, required this.interactionKind});

  final TurnPhase turnPhase;
  final InteractionKind? interactionKind;

  @override
  Widget build(BuildContext context) {
    final idle = interactionKind == null && turnPhase == TurnPhase.idle;
    final label = interactionKind == null
        ? context.turnPhaseLabel(turnPhase)
        : context.interactionKindLabel(interactionKind!);
    final foreground = idle ? context.studioInkSoft : StudioColors.clayDeep;
    return SizedBox(
      height: 26,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 7),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            DecoratedBox(
              decoration: BoxDecoration(
                color: idle ? context.studioInkSoft : StudioColors.clay,
                borderRadius: BorderRadius.circular(StudioRadii.pill),
              ),
              child: const SizedBox.square(dimension: 5),
            ),
            const SizedBox(width: 7),
            ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 120),
              child: Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: context.text.labelSmall?.copyWith(
                  color: foreground,
                  fontWeight: FontWeight.w500,
                  height: 1,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
