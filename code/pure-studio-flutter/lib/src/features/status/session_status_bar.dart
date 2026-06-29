import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../app/theme/studio_tokens.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/upward_popup_menu.dart';
import 'context_usage_ring.dart';
import 'status_bar_chip.dart';
import 'status_detail_popover.dart';

class SessionStatusBar extends ConsumerWidget {
  const SessionStatusBar({required this.state, super.key});

  final StudioState state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final runtime = state.runtime;
    final session = state.sessions
        .where((session) => session.id == state.selectedSessionId)
        .firstOrNull;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: context.studioPaper2,
        border: Border(top: BorderSide(color: context.studioLine)),
      ),
      child: SizedBox(
        height: 44,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 26),
          child: Align(
            alignment: Alignment.center,
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 740),
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
                              enabled: !state.isBusy,
                            ),
                          if (state.providers.isNotEmpty)
                            _PlannerModelSelector(state: state),
                          if (_plannerEffortsForState(state).isNotEmpty)
                            _ReasoningEffortSelector(state: state),
                          ContextUsageRing(runtime: runtime),
                          if (runtime.costLabel.isNotEmpty)
                            _StatusChip(
                              label: runtime.costLabel,
                              tooltip: context.l10n.statusCost,
                              detailWidth: 260,
                              detailBuilder: (context) =>
                                  _CostDetail(runtime: runtime),
                            ),
                          if (_runtimeCapabilityLabel(
                            context,
                            runtime,
                          ).isNotEmpty)
                            _StatusChip(
                              icon: Icons.tune_outlined,
                              label: _runtimeCapabilityLabel(context, runtime),
                              tooltip: context.l10n.statusCapabilitiesTitle,
                              detailWidth: 310,
                              maxWidth: 150,
                              detailBuilder: (context) =>
                                  _CapabilityDetail(runtime: runtime),
                            ),
                        ],
                      ),
                    ),
                  ),
                  const SizedBox(width: 8),
                  _PhasePill(
                    phase:
                        state.activeInteraction?.kind.name ??
                        state.turnPhase.name,
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
    if (runtime.agentCount > 0)
      context.l10n.statusAgentsCount(runtime.agentCount),
  ];
  return parts.join(' · ');
}

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
      child: _ControlChip(
        icon: _modeIcon(mode),
        label: context.compileModeLabel(mode),
        enabled: enabled,
      ),
    );
  }

  IconData _modeIcon(CompileMode value) {
    return switch (value) {
      CompileMode.auto => Icons.flash_on,
      CompileMode.plan => Icons.route_outlined,
    };
  }
}

class _PlannerModelSelector extends ConsumerWidget {
  const _PlannerModelSelector({required this.state});

  final StudioState state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final options = _plannerModelOptions(state.providers);
    if (options.isEmpty) {
      return const SizedBox.shrink();
    }
    final role = state.role('planner');
    final current = _plannerModelFor(state) ?? options.first;
    return UpwardPopupMenu<String>(
      tooltip: context.l10n.statusPlannerModel,
      initialValue: current.key,
      onSelected: (key) {
        final option = options.firstWhere((option) => option.key == key);
        final effort = option.reasoningEfforts.contains(role?.effort)
            ? role?.effort
            : option.reasoningEfforts.firstOrNull;
        ref
            .read(studioControllerProvider.notifier)
            .setModelRole(
              roleKey: 'planner',
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
      child: _ControlChip(
        icon: Icons.smart_toy_outlined,
        label: current.model,
        enabled: true,
      ),
    );
  }
}

class _ReasoningEffortSelector extends ConsumerWidget {
  const _ReasoningEffortSelector({required this.state});

  final StudioState state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final role = state.role('planner');
    final currentModel = _plannerModelFor(state);
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
              roleKey: 'planner',
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
      child: _ControlChip(
        icon: Icons.schedule_outlined,
        label: current,
        enabled: true,
      ),
    );
  }
}

class _ControlChip extends StatelessWidget {
  const _ControlChip({
    required this.icon,
    required this.label,
    required this.enabled,
  });

  final IconData icon;
  final String label;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    return StatusBarChip(
      icon: icon,
      label: label,
      enabled: enabled,
      maxWidth: 160,
      trailingIcon: Icons.keyboard_arrow_down,
    );
  }
}

class _PlannerModelOption {
  const _PlannerModelOption({
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

_PlannerModelOption? _plannerModelFor(StudioState state) {
  final role = state.role('planner');
  if (role == null) {
    return null;
  }
  final options = _plannerModelOptions(state.providers);
  if (options.isEmpty) {
    return null;
  }
  return options.firstWhere(
    (option) =>
        option.providerId == role.providerId && option.model == role.model,
    orElse: () => options.first,
  );
}

List<String> _plannerEffortsForState(StudioState state) {
  return _plannerModelFor(state)?.reasoningEfforts ?? const [];
}

List<_PlannerModelOption> _plannerModelOptions(
  List<ProviderSettingsView> providers,
) {
  final options = <_PlannerModelOption>[];
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
        _PlannerModelOption(
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

class _StatusChip extends StatelessWidget {
  const _StatusChip({
    required this.label,
    required this.tooltip,
    this.icon,
    this.detailBuilder,
    this.detailWidth = 300,
    this.maxWidth = 180,
  });

  final IconData? icon;
  final String label;
  final String tooltip;
  final WidgetBuilder? detailBuilder;
  final double detailWidth;
  final double maxWidth;

  @override
  Widget build(BuildContext context) {
    final chip = StatusBarChip(
      icon: icon,
      label: label,
      tooltip: detailBuilder == null
          ? (tooltip.isEmpty ? label : tooltip)
          : null,
      detailBuilder: detailBuilder,
      detailWidth: detailWidth,
      enableHover: detailBuilder == null,
      maxWidth: maxWidth,
    );
    return chip;
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
        if (runtime.agentCount > 0)
          StatusDetailRow(
            label: context.l10n.statusSubagentsSection,
            value: context.l10n.statusAgentsCount(runtime.agentCount),
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
        if (runtime.agentCount > 0)
          StatusDetailIconRow(
            icon: Icons.group_outlined,
            title: context.l10n.statusSubagentsSection,
            detail: context.l10n.statusAgentsCount(runtime.agentCount),
            iconColor: StudioColors.rose,
            backgroundColor: StudioColors.rose.withValues(alpha: 0.13),
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

class _PhasePill extends StatelessWidget {
  const _PhasePill({required this.phase});

  final String phase;

  @override
  Widget build(BuildContext context) {
    final idle = phase == TurnPhase.idle.name;
    final foreground = idle ? context.studioInkSoft : StudioColors.clayDeep;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: idle ? context.studioPaper3 : StudioColors.claySoft,
        border: Border.all(
          color: idle ? context.studioLine2 : StudioColors.claySoft,
        ),
        borderRadius: BorderRadius.circular(StudioRadii.pill),
      ),
      child: SizedBox(
        height: 26,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 12),
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
                constraints: const BoxConstraints(maxWidth: 160),
                child: Text(
                  phase,
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
      ),
    );
  }
}
