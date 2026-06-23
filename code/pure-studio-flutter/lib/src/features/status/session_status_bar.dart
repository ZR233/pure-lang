import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../app/theme/studio_tokens.dart';
import '../../shared/studio_chrome.dart';
import '../../shared/upward_popup_menu.dart';
import 'context_usage_ring.dart';

class SessionStatusBar extends ConsumerWidget {
  const SessionStatusBar({required this.state, super.key});

  final StudioState state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final runtime = state.runtime;
    final session = state.sessions
        .where((session) => session.id == state.selectedSessionId)
        .firstOrNull;
    return SizedBox(
      height: 34,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(18, 5, 18, 0),
        child: Align(
          alignment: Alignment.center,
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 860),
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
                            tooltip: 'Cost',
                          ),
                        if (_runtimeCapabilityLabel(runtime).isNotEmpty)
                          _StatusChip(
                            icon: Icons.tune_outlined,
                            label: _runtimeCapabilityLabel(runtime),
                            tooltip: _runtimeCapabilityTooltip(runtime),
                          ),
                      ],
                    ),
                  ),
                ),
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
    );
  }
}

String _runtimeCapabilityLabel(SessionRuntimeView runtime) {
  final parts = [
    if (runtime.activeSkills.isNotEmpty)
      '${runtime.activeSkills.length} skills',
    if (runtime.activeMcpServers.isNotEmpty)
      '${runtime.activeMcpServers.length} MCP',
    if (runtime.activeLspServers.isNotEmpty)
      '${runtime.activeLspServers.length} LSP',
    if (runtime.agentCount > 0) '${runtime.agentCount} agents',
  ];
  return parts.join(' · ');
}

String _runtimeCapabilityTooltip(SessionRuntimeView runtime) {
  final sections = [
    if (runtime.activeSkills.isNotEmpty)
      'Skills\n${runtime.activeSkills.join('\n')}',
    if (runtime.activeMcpServers.isNotEmpty)
      'MCP\n${runtime.activeMcpServers.join('\n')}',
    if (runtime.activeLspServers.isNotEmpty)
      'LSP\n${runtime.activeLspServers.join('\n')}',
    if (runtime.agentCount > 0) 'Subagents\n${runtime.agentCount}',
  ];
  return sections.join('\n\n');
}

class _SessionModeSelector extends ConsumerWidget {
  const _SessionModeSelector({required this.mode, required this.enabled});

  final CompileMode mode;
  final bool enabled;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return UpwardPopupMenu<CompileMode>(
      tooltip: 'Session mode',
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
                Text(_modeLabel(option)),
              ],
            ),
          ),
      ],
      child: _ControlChip(
        icon: _modeIcon(mode),
        label: _modeLabel(mode),
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

  String _modeLabel(CompileMode value) {
    return switch (value) {
      CompileMode.auto => 'Auto',
      CompileMode.plan => 'Plan',
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
      tooltip: 'Planner model',
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
      tooltip: 'Reasoning effort',
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
    final colors = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.only(right: 6),
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: enabled
              ? colors.surfaceContainerLowest.withValues(alpha: 0.78)
              : colors.surfaceContainerHighest,
          border: Border.all(
            color: colors.outlineVariant.withValues(alpha: 0.72),
          ),
          borderRadius: BorderRadius.circular(8),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 4),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(icon, size: 15, color: colors.onSurfaceVariant),
              const SizedBox(width: 5),
              ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 160),
                child: Text(
                  label,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.labelSmall?.copyWith(
                    color: colors.onSurfaceVariant,
                  ),
                ),
              ),
              const SizedBox(width: 2),
              Icon(
                Icons.keyboard_arrow_down,
                size: 15,
                color: colors.onSurfaceVariant,
              ),
            ],
          ),
        ),
      ),
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
  const _StatusChip({required this.label, required this.tooltip, this.icon});

  final IconData? icon;
  final String label;
  final String tooltip;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: tooltip.isEmpty ? label : tooltip,
      child: Padding(
        padding: const EdgeInsets.only(right: 8),
        child: DecoratedBox(
          decoration: BoxDecoration(
            color: Theme.of(context).colorScheme.surfaceContainerLowest,
            border: Border.all(
              color: Theme.of(
                context,
              ).colorScheme.outlineVariant.withValues(alpha: 0.72),
            ),
            borderRadius: BorderRadius.circular(8),
          ),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 4),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (icon != null) ...[
                  Icon(
                    icon,
                    size: 15,
                    color: Theme.of(context).colorScheme.onSurfaceVariant,
                  ),
                  const SizedBox(width: 5),
                ],
                ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 180),
                  child: Text(
                    label,
                    overflow: TextOverflow.ellipsis,
                    style: Theme.of(context).textTheme.labelSmall?.copyWith(
                      color: Theme.of(context).colorScheme.onSurfaceVariant,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _PhasePill extends StatelessWidget {
  const _PhasePill({required this.phase});

  final String phase;

  @override
  Widget build(BuildContext context) {
    return StudioPill(
      label: phase,
      backgroundColor: StudioColors.claySoft,
      foregroundColor: StudioColors.clayDeep,
      borderColor: StudioColors.claySoft,
    );
  }
}
