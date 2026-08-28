import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../app/theme/studio_tokens.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import '../../shared/studio_driver_state.dart';
import 'context_usage_readout.dart';
import 'session_selectors.dart';
import 'status_bar_item.dart';
import 'status_detail_popover.dart';
import 'task_runtime_detail.dart';

class ThreadStatusBar extends ConsumerWidget {
  const ThreadStatusBar({
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
    StudioDriverState.publishTask(runtime.task);
    final thread = workspace.thread;
    final capabilityLabel = _runtimeCapabilityLabel(context, runtime);
    final lspActiveServers = [
      for (final server
          in ref
                  .watch(
                    studioControllerProvider.select(
                      (state) => state.value?.lspState,
                    ),
                  )
                  ?.servers ??
              const <LspServerStateView>[])
        if (_lspActiveActivity(server) != null) server,
    ];
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
                final showModel =
                    constraints.maxWidth >= 610 &&
                    (runtime.task == null || constraints.maxWidth >= 760);
                final showEffort = constraints.maxWidth >= 720;
                final showCapabilities = constraints.maxWidth >= 840;
                final showLspActivity = constraints.maxWidth >= 850;
                final hasOverflow =
                    (!showEffort &&
                        thread.isRoot &&
                        effortsFor(
                          workspace.providers,
                          workspace.roles,
                          thread.mode,
                        ).isNotEmpty) ||
                    (!showCapabilities && capabilityLabel.isNotEmpty) ||
                    (!showLspActivity && lspActiveServers.isNotEmpty);
                final rootEffort = thread.isRoot
                    ? roleByKey(
                        workspace.roles,
                        roleKeyForMode(thread.mode),
                      )?.effort.trim()
                    : null;
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
                      label: context.roleLabel(thread.role),
                      tooltip: '${thread.title} · ${thread.status.name}',
                      maxWidth: 96,
                    ),
                    if (thread.isRoot)
                      SessionModeSelector(
                        mode: thread.mode,
                        enabled:
                            !runtime.hasActiveTask &&
                            thread.status == ThreadStatusView.idle,
                        onSelected: (mode) => ref
                            .read(studioControllerProvider.notifier)
                            .setThreadMode(mode),
                      ),
                    if (showModel &&
                        thread.isRoot &&
                        workspace.providers.isNotEmpty)
                      ModelRoleSelector(
                        providers: workspace.providers,
                        roles: workspace.roles,
                        mode: thread.mode,
                      )
                    else if (showModel &&
                        thread.isAgent &&
                        runtime.model.isNotEmpty)
                      _StatusReadout(
                        icon: Icons.smart_toy_outlined,
                        label: runtime.model,
                        tooltip: runtime.model,
                        maxWidth: 140,
                      ),
                    if (showEffort &&
                        thread.isRoot &&
                        effortsFor(
                          workspace.providers,
                          workspace.roles,
                          thread.mode,
                        ).isNotEmpty)
                      ReasoningEffortSelector(
                        providers: workspace.providers,
                        roles: workspace.roles,
                        mode: thread.mode,
                      ),
                    if (runtime.task case final task?)
                      _TaskRuntimeReadout(
                        task: task,
                        rootThreadId: thread.rootThreadId,
                        paused: _taskExecutionIsPaused(task, thread),
                      ),
                    ContextUsageReadout(runtime: runtime),
                    _StatusReadout(
                      key: StudioDriverKeys.threadThroughput,
                      icon: Icons.speed_outlined,
                      label: runtime.turnThroughputLabel,
                      tooltip: context.l10n.statusCurrentAgentTokenSpeed,
                      maxWidth: 84,
                    ),
                    if (showLspActivity && lspActiveServers.isNotEmpty)
                      _LspActivityReadout(servers: lspActiveServers),
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
                        effort: (rootEffort?.isEmpty ?? true)
                            ? null
                            : rootEffort,
                        capabilities: capabilityLabel,
                        runtime: runtime,
                        lspServers: showLspActivity
                            ? const <LspServerStateView>[]
                            : lspActiveServers,
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

class _LspActivityReadout extends StatelessWidget {
  const _LspActivityReadout({required this.servers});

  final List<LspServerStateView> servers;

  @override
  Widget build(BuildContext context) {
    final activeServers = servers;
    if (activeServers.isEmpty) {
      return const SizedBox.shrink();
    }
    final first = activeServers.first;
    final firstActivity = _lspActiveActivity(first)!;
    final firstLabel = _lspActivityLabel(context, firstActivity);
    final percentage = _lspActivityPercentage(firstActivity);
    final label = activeServers.length == 1
        ? (percentage == null
              ? firstLabel
              : '$firstLabel ${context.l10n.statusLspActivityPercentage(percentage)}')
        : '$firstLabel · ${context.l10n.statusLspCount(activeServers.length)}';
    final tooltip = [
      for (final server in activeServers)
        '${server.displayName} · ${_lspServerActivitySummary(context, server)}',
    ].join('\n');
    return KeyedSubtree(
      key: StudioDriverKeys.lspActivity(),
      child: _StatusReadout(
        key: StudioDriverKeys.lspActivityDetail(),
        icon: Icons.terminal_outlined,
        label: label,
        tooltip: tooltip,
        maxWidth: 120,
        interactive: true,
        detailBuilder: (context) => _LspActivityDetail(servers: activeServers),
      ),
    );
  }
}

LspActivity? _lspActiveActivity(LspServerStateView server) {
  return switch (server.state) {
    LspAvailableState(activity: final activity)
        when activity is! LspIdleActivity =>
      activity,
    LspAvailableState() ||
    LspCheckingState() ||
    LspUnavailableState() ||
    LspDisabledState() => null,
  };
}

String _lspActivityLabel(BuildContext context, LspActivity activity) {
  return switch (activity) {
    LspIndexingActivity() => context.l10n.statusLspIndexing,
    LspBusyActivity() => context.l10n.statusLspBusy,
    LspIdleActivity() => 'idle',
  };
}

int? _lspActivityPercentage(LspActivity activity) => switch (activity) {
  LspBusyActivity(:final percentage) ||
  LspIndexingActivity(:final percentage) => percentage,
  LspIdleActivity() => null,
};

String _lspServerActivitySummary(
  BuildContext context,
  LspServerStateView server,
) {
  final activity = _lspActiveActivity(server);
  if (activity == null) return '';
  return [
    _lspActivityLabel(context, activity),
    if (_lspActivityPercentage(activity) case final percentage?)
      context.l10n.statusLspActivityPercentage(percentage),
    if (activity
        case LspBusyActivity(title: final title?) ||
            LspIndexingActivity(title: final title?)
        when title.isNotEmpty)
      title,
    if (activity
        case LspBusyActivity(message: final message?) ||
            LspIndexingActivity(message: final message?)
        when message.isNotEmpty)
      message,
  ].join(' · ');
}

class _LspActivityDetail extends StatelessWidget {
  const _LspActivityDetail({required this.servers});

  final List<LspServerStateView> servers;

  @override
  Widget build(BuildContext context) {
    return StatusDetailPanel(
      title: context.l10n.statusLspSection,
      children: [
        for (final server in servers)
          StatusDetailIconRow(
            icon: Icons.terminal_outlined,
            title: server.displayName,
            detail: _lspServerActivitySummary(context, server),
            iconColor: StudioColors.ochre,
            backgroundColor: StudioColors.ochre.withValues(alpha: 0.15),
          ),
      ],
    );
  }
}

class _TaskRuntimeReadout extends StatelessWidget {
  const _TaskRuntimeReadout({
    required this.task,
    required this.rootThreadId,
    required this.paused,
  });

  final TaskRuntimeView task;
  final String rootThreadId;
  final bool paused;

  @override
  Widget build(BuildContext context) {
    final issue = task.issues.lastOrNull;
    final failedOutcome = switch (task.state) {
      CompletedTaskStateView(outcome: final FailedTaskOutcomeView outcome) =>
        outcome,
      _ => null,
    };
    final paused = this.paused && failedOutcome == null && issue == null;
    final status =
        failedOutcome?.summary ??
        issue?.message ??
        (paused ? context.l10n.statusTaskPausedHint : task.stateSummary);
    final fatal =
        failedOutcome?.kind == TaskFailureKindView.fatal ||
        (issue?.isFatal ?? false);
    final taskLabel = paused
        ? context.l10n.statusTaskPaused
        : context.taskPhaseLabel(task.state.kind);
    final tooltip = status.isEmpty
        ? '${context.taskPhaseLabel(task.state.kind)} · ${task.runId}'
        : '${fatal ? context.l10n.statusTaskFailed : taskLabel} · $status';
    final readout = _StatusReadout(
      key: StudioDriverKeys.taskPhase(task.runId, task.state.kind),
      icon: fatal
          ? Icons.error_outline
          : issue != null
          ? Icons.warning_amber_outlined
          : paused
          ? Icons.pause_circle_outline
          : Icons.route_outlined,
      label: fatal
          ? context.l10n.statusTaskFailed
          : issue != null
          ? context.l10n.statusTaskRecoverable
          : taskLabel,
      tooltip: tooltip,
      maxWidth: 120,
      interactive: true,
      detailWidth: 560,
      detailBuilder: (context) => TaskRuntimeDetail(
        task: task,
        rootThreadId: rootThreadId,
        paused: paused,
      ),
    );
    return KeyedSubtree(
      key: StudioDriverKeys.taskRuntime(task.runId),
      child: Semantics(
        key: StudioDriverKeys.taskStatus(task.runId, status),
        label: tooltip,
        child: paused
            ? KeyedSubtree(
                key: StudioDriverKeys.taskPaused(task.runId),
                child: readout,
              )
            : readout,
      ),
    );
  }
}

bool _taskExecutionIsPaused(TaskRuntimeView task, StudioThread thread) {
  if (!thread.isRoot ||
      thread.status != ThreadStatusView.idle ||
      !task.isActive ||
      task.state.kind == TaskStateKind.pendingConfirmation) {
    return false;
  }
  final activeWorkUnit = task.workUnits.any(
    (unit) => const {
      TaskWorkUnitStateKind.pending,
      TaskWorkUnitStateKind.running,
    }.contains(unit.state.kind),
  );
  final activeReview = task.reviews.any(
    (review) => const {
      TaskReviewStateKind.pendingDispatch,
      TaskReviewStateKind.dispatched,
      TaskReviewStateKind.running,
    }.contains(review.state.kind),
  );
  return !activeWorkUnit && !activeReview;
}

String _runtimeCapabilityLabel(
  BuildContext context,
  ThreadRuntimeView runtime,
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

class _StatusOverflow extends StatelessWidget {
  const _StatusOverflow({
    required this.effort,
    required this.capabilities,
    required this.runtime,
    this.lspServers = const <LspServerStateView>[],
  });

  final String? effort;
  final String capabilities;
  final ThreadRuntimeView runtime;
  final List<LspServerStateView> lspServers;

  @override
  Widget build(BuildContext context) {
    return PopupMenuButton<_StatusOverflowAction>(
      key: const ValueKey('status-overflow'),
      tooltip: MaterialLocalizations.of(context).moreButtonTooltip,
      position: PopupMenuPosition.over,
      icon: const Icon(Icons.more_horiz, size: 18),
      padding: EdgeInsets.zero,
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
        if (capabilities.isNotEmpty)
          PopupMenuItem<_StatusOverflowAction>(
            value: _StatusOverflowAction.capabilities,
            child: Text(
              capabilities,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
            ),
          ),
        if (lspServers.isNotEmpty)
          PopupMenuItem<_StatusOverflowAction>(
            key: StudioDriverKeys.lspActivityOverflow(),
            value: _StatusOverflowAction.lsp,
            child: Text(
              _lspOverflowSummary(context, lspServers),
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
            ),
          ),
      ],
    );
  }

  void _showDetail(BuildContext context, _StatusOverflowAction action) {
    final detail = switch (action) {
      _StatusOverflowAction.capabilities => _CapabilityDetail(runtime: runtime),
      _StatusOverflowAction.lsp => _LspActivityDetail(servers: lspServers),
    };
    showDialog<void>(
      context: context,
      builder: (context) => Dialog(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 560, maxHeight: 520),
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: SingleChildScrollView(child: detail),
          ),
        ),
      ),
    );
  }
}

enum _StatusOverflowAction { capabilities, lsp }

String _lspOverflowSummary(
  BuildContext context,
  List<LspServerStateView> servers,
) {
  final first = servers.first;
  final summary =
      '${first.displayName} · ${_lspServerActivitySummary(context, first)}';
  if (servers.length == 1) {
    return summary;
  }
  return '$summary · ${context.l10n.statusLspCount(servers.length)}';
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
    super.key,
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
      semanticsLabel: tooltip.isEmpty ? label : tooltip,
      detailBuilder: detailBuilder,
      detailWidth: detailWidth,
      enableHover: detailBuilder == null,
      interactive: interactive,
      maxWidth: maxWidth,
    );
  }
}

class _CapabilityDetail extends StatelessWidget {
  const _CapabilityDetail({required this.runtime});

  final ThreadRuntimeView runtime;

  @override
  Widget build(BuildContext context) {
    return StatusDetailPanel(
      title: context.l10n.statusCapabilitiesTitle,
      children: [
        if (runtime.activeSkills.isNotEmpty)
          StatusDetailIconList(
            icon: Icons.extension_outlined,
            title: context.l10n.statusSkillsSection,
            items: runtime.activeSkills,
            itemKey: StudioDriverKeys.statusActiveSkill,
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
