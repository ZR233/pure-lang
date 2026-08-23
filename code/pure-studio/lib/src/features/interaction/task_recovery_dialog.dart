import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import '../../shared/studio_driver_state.dart';

Future<TaskRecoveryResult?> showTaskRecoveryDialog(
  BuildContext context,
  String rootThreadId,
) {
  StudioDriverState.clearTaskRecovery();
  return showDialog<TaskRecoveryResult>(
    context: context,
    barrierDismissible: false,
    builder: (context) => _TaskRecoveryDialog(rootThreadId: rootThreadId),
  );
}

class _TaskRecoveryDialog extends ConsumerStatefulWidget {
  const _TaskRecoveryDialog({required this.rootThreadId});

  final String rootThreadId;

  @override
  ConsumerState<_TaskRecoveryDialog> createState() =>
      _TaskRecoveryDialogState();
}

class _TaskRecoveryDialogState extends ConsumerState<_TaskRecoveryDialog> {
  late Future<TaskRecoveryPreview> _preview;

  @override
  void initState() {
    super.initState();
    _preview = _loadPreview();
  }

  Future<TaskRecoveryPreview> _loadPreview() {
    return ref
        .read(studioControllerProvider.notifier)
        .previewTaskRecovery(widget.rootThreadId)
        .then((preview) {
          StudioDriverState.publishTaskRecoveryPreview(preview);
          return preview;
        });
  }

  void _refreshPreview() {
    setState(() => _preview = _loadPreview());
  }

  @override
  Widget build(BuildContext context) {
    return Dialog(
      key: StudioDriverKeys.taskRecoveryDialog,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 720, maxHeight: 760),
        child: FutureBuilder<TaskRecoveryPreview>(
          future: _preview,
          builder: (context, snapshot) {
            if (snapshot.connectionState != ConnectionState.done) {
              return const Padding(
                padding: EdgeInsets.all(32),
                child: Center(child: CircularProgressIndicator()),
              );
            }
            final preview = snapshot.data;
            if (preview == null) {
              return _TaskRecoveryPreviewError(
                error: snapshot.error,
                onRetry: _refreshPreview,
              );
            }
            return _TaskRecoveryForm(preview: preview);
          },
        ),
      ),
    );
  }
}

class _TaskRecoveryPreviewError extends StatelessWidget {
  const _TaskRecoveryPreviewError({required this.error, required this.onRetry});

  final Object? error;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(24),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            context.l10n.taskRecoveryDialogTitle,
            style: Theme.of(context).textTheme.titleLarge,
          ),
          const SizedBox(height: 12),
          SelectableText(error?.toString() ?? 'Unknown recovery error'),
          const SizedBox(height: 20),
          Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              TextButton(
                onPressed: () => Navigator.of(context).pop(),
                child: Text(context.l10n.recoveryCleanupCancel),
              ),
              const SizedBox(width: 8),
              FilledButton(
                onPressed: onRetry,
                child: Text(context.l10n.recoveryCleanupRefreshPreview),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class _TaskRecoveryForm extends ConsumerStatefulWidget {
  const _TaskRecoveryForm({required this.preview});

  final TaskRecoveryPreview preview;

  @override
  ConsumerState<_TaskRecoveryForm> createState() => _TaskRecoveryFormState();
}

class _TaskRecoveryFormState extends ConsumerState<_TaskRecoveryForm> {
  late String _targetThreadId;
  late ConversationRecoveryMode _mode;
  late int _tailCount;
  late final String _recoveryId;
  bool _confirmationArmed = false;
  bool _submitting = false;
  String? _error;

  TaskRecoveryTarget get _target => widget.preview.target(_targetThreadId);

  List<TaskRecoveryTurn> get _selectedTurns {
    final turns = _target.turns;
    return turns.sublist(turns.length - _tailCount);
  }

  @override
  void initState() {
    super.initState();
    _targetThreadId = widget.preview.recommendedThreadId;
    _resetTargetSelection();
    _recoveryId =
        'studio:${widget.preview.runId}:'
        '${DateTime.now().microsecondsSinceEpoch}';
  }

  void _resetTargetSelection() {
    final target = _target;
    _tailCount = target.defaultTurnIds.length.clamp(1, target.turns.length);
    _mode = target.availableModes.contains(ConversationRecoveryMode.rewindTail)
        ? ConversationRecoveryMode.rewindTail
        : ConversationRecoveryMode.rebuildThread;
    _confirmationArmed = false;
    _error = null;
  }

  void _changeTarget(String? threadId) {
    if (threadId == null || threadId == _targetThreadId) return;
    setState(() {
      _targetThreadId = threadId;
      _resetTargetSelection();
    });
  }

  void _changeTailCount(int? count) {
    if (count == null || count == _tailCount) return;
    setState(() {
      _tailCount = count;
      _confirmationArmed = false;
      _error = null;
    });
  }

  void _changeMode(ConversationRecoveryMode? mode) {
    if (mode == null || mode == _mode) return;
    setState(() {
      _mode = mode;
      _confirmationArmed = false;
      _error = null;
    });
  }

  @override
  Widget build(BuildContext context) {
    final preview = widget.preview;
    final target = _target;
    return Padding(
      padding: const EdgeInsets.all(24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            context.l10n.taskRecoveryDialogTitle,
            style: Theme.of(context).textTheme.titleLarge,
          ),
          const SizedBox(height: 6),
          Text(
            context.l10n.taskRecoveryDialogBody,
            style: Theme.of(context).textTheme.bodyMedium
                ?.copyWith(color: context.studioInkSoft),
          ),
          const SizedBox(height: 18),
          Expanded(
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  _TaskRecoveryFacts(preview: preview),
                  const SizedBox(height: 16),
                  _LabeledField(
                    label: context.l10n.taskRecoveryTargetLabel,
                    child: DropdownButton<String>(
                      key: StudioDriverKeys.taskRecoveryTarget,
                      value: _targetThreadId,
                      isExpanded: true,
                      items: [
                        for (final candidate in preview.targets)
                          DropdownMenuItem(
                            value: candidate.threadId,
                            child: Text(_targetLabel(context, candidate)),
                          ),
                      ],
                      onChanged: _submitting ? null : _changeTarget,
                    ),
                  ),
                  const SizedBox(height: 12),
                  _LabeledField(
                    label: context.l10n.taskRecoveryTurnSuffixLabel,
                    child: DropdownButton<int>(
                      key: StudioDriverKeys.taskRecoveryTailCount,
                      value: _tailCount,
                      isExpanded: true,
                      items: [
                        for (
                          var count = 1;
                          count <= target.turns.length;
                          count++
                        )
                          DropdownMenuItem(value: count, child: Text('$count')),
                      ],
                      onChanged: _submitting ? null : _changeTailCount,
                    ),
                  ),
                  const SizedBox(height: 12),
                  _LabeledField(
                    label: context.l10n.taskRecoveryModeLabel,
                    child: DropdownButton<ConversationRecoveryMode>(
                      key: StudioDriverKeys.taskRecoveryMode,
                      value: _mode,
                      isExpanded: true,
                      items: [
                        for (final mode in target.availableModes)
                          DropdownMenuItem(
                            value: mode,
                            child: Text(
                              _modeLabel(context, mode),
                              key: StudioDriverKeys.taskRecoveryModeOption(
                                mode.name,
                              ),
                            ),
                          ),
                      ],
                      onChanged: _submitting ? null : _changeMode,
                    ),
                  ),
                  const SizedBox(height: 16),
                  for (final turn in _selectedTurns)
                    _TaskRecoveryTurnCard(turn: turn),
                  const SizedBox(height: 8),
                  _RecoveryWarning(
                    icon: Icons.account_tree_outlined,
                    text: context.l10n.taskRecoveryGitPreserved,
                    detail:
                        '${target.branch}'
                        '${target.baseCommit == null ? '' : ' · ${_shortHash(target.baseCommit!)}'}\n'
                        '${target.worktreePath}',
                  ),
                  if (_mode == ConversationRecoveryMode.rebuildThread) ...[
                    const SizedBox(height: 10),
                    _RecoveryWarning(
                      icon: Icons.warning_amber_outlined,
                      text: context.l10n.taskRecoveryRebuildWarning,
                    ),
                  ],
                  if (_confirmationArmed) ...[
                    const SizedBox(height: 12),
                    _RecoveryWarning(
                      icon: Icons.verified_user_outlined,
                      text:
                          '${_modeLabel(context, _mode)} · '
                          '${_selectedTurns.length} Turn(s) · '
                          '${_targetLabel(context, target)}',
                    ),
                  ],
                  if (_error case final error?) ...[
                    const SizedBox(height: 12),
                    SelectableText(
                      error,
                      key: StudioDriverKeys.taskRecoveryError,
                      style: TextStyle(
                        color: Theme.of(context).colorScheme.error,
                      ),
                    ),
                  ],
                ],
              ),
            ),
          ),
          const SizedBox(height: 18),
          Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              TextButton(
                onPressed: _submitting
                    ? null
                    : () => Navigator.of(context).pop(),
                child: Text(context.l10n.recoveryCleanupCancel),
              ),
              const SizedBox(width: 8),
              FilledButton.icon(
                key: _confirmationArmed
                    ? StudioDriverKeys.taskRecoveryApply
                    : StudioDriverKeys.taskRecoveryConfirm,
                onPressed: _submitting ? null : _confirm,
                icon: _submitting
                    ? const SizedBox.square(
                        dimension: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : Icon(
                        _confirmationArmed
                            ? Icons.restore_outlined
                            : Icons.fact_check_outlined,
                      ),
                label: Text(
                  _submitting
                      ? context.l10n.taskRecoveryApplying
                      : _confirmationArmed
                      ? context.l10n.taskRecoveryFinalConfirm
                      : context.l10n.taskRecoveryFirstConfirm,
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }

  Future<void> _confirm() async {
    if (!_confirmationArmed) {
      setState(() => _confirmationArmed = true);
      return;
    }
    setState(() {
      _submitting = true;
      _error = null;
    });
    try {
      final result = await ref
          .read(studioControllerProvider.notifier)
          .applyTaskRecovery(
            TaskRecoveryRequest(
              recoveryId: _recoveryId,
              rootThreadId: widget.preview.rootThreadId,
              targetThreadId: _targetThreadId,
              mode: _mode,
              turnIds: _selectedTurns.map((turn) => turn.turnId).toList(),
              preview: widget.preview,
            ),
          );
      StudioDriverState.publishTaskRecoveryResult(result);
      if (mounted) Navigator.of(context).pop(result);
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    } finally {
      if (mounted) setState(() => _submitting = false);
    }
  }
}

class _TaskRecoveryFacts extends StatelessWidget {
  const _TaskRecoveryFacts({required this.preview});

  final TaskRecoveryPreview preview;

  @override
  Widget build(BuildContext context) {
    return Wrap(
      spacing: 8,
      runSpacing: 8,
      children: [
        Chip(label: Text('Task ${preview.runId}')),
        Chip(label: Text(context.taskPhaseLabel(preview.state))),
      ],
    );
  }
}

class _TaskRecoveryTurnCard extends StatelessWidget {
  const _TaskRecoveryTurnCard({required this.turn});

  final TaskRecoveryTurn turn;

  @override
  Widget build(BuildContext context) {
    final metadata =
        '${turn.itemCount} ${context.l10n.taskRecoveryItems} · '
        '${turn.inputCount} ${context.l10n.taskRecoveryInputs} · '
        '${turn.toolCount} ${context.l10n.taskRecoveryTools}';
    return Card(
      key: StudioDriverKeys.taskRecoveryTurn(turn.turnId),
      margin: const EdgeInsets.only(bottom: 8),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Expanded(
                  child: Text(
                    turn.turnId,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: Theme.of(context).textTheme.labelLarge,
                  ),
                ),
                Chip(label: Text(turn.state.name)),
              ],
            ),
            Text(metadata, style: Theme.of(context).textTheme.bodySmall),
            if (turn.toolSummaries.isNotEmpty) ...[
              const SizedBox(height: 8),
              Wrap(
                spacing: 6,
                runSpacing: 6,
                children: [
                  for (final tool in turn.toolSummaries)
                    Chip(label: Text(tool)),
                ],
              ),
            ],
          ],
        ),
      ),
    );
  }
}

class _RecoveryWarning extends StatelessWidget {
  const _RecoveryWarning({required this.icon, required this.text, this.detail});

  final IconData icon;
  final String text;
  final String? detail;

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(
        color: context.studioPaper2,
        border: Border.all(color: context.studioLine),
        borderRadius: BorderRadius.circular(StudioRadii.sm),
      ),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Icon(icon, size: 19, color: context.studioInkSoft),
            const SizedBox(width: 10),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(text),
                  if (detail case final detail?) ...[
                    const SizedBox(height: 4),
                    SelectableText(
                      detail,
                      style: Theme.of(context).textTheme.bodySmall
                          ?.copyWith(color: context.studioInkSoft),
                    ),
                  ],
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _LabeledField extends StatelessWidget {
  const _LabeledField({required this.label, required this.child});

  final String label;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(label, style: Theme.of(context).textTheme.labelMedium),
        child,
      ],
    );
  }
}

String _targetLabel(BuildContext context, TaskRecoveryTarget target) {
  final role = switch (target.kind) {
    TaskRecoveryTargetKind.planner => context.l10n.taskRecoveryTargetPlanner,
    TaskRecoveryTargetKind.executor => context.l10n.taskRecoveryTargetExecutor,
  };
  final workUnit = target.workUnitId == null ? '' : ' · ${target.workUnitId}';
  final attempt = target.attempt == null ? '' : ' · attempt ${target.attempt}';
  return '$role$workUnit$attempt';
}

String _modeLabel(BuildContext context, ConversationRecoveryMode mode) {
  return switch (mode) {
    ConversationRecoveryMode.rewindTail => context.l10n.taskRecoveryModeRewind,
    ConversationRecoveryMode.rebuildThread =>
      context.l10n.taskRecoveryModeRebuild,
  };
}

String _shortHash(String value) {
  return value.length <= 10 ? value : value.substring(0, 10);
}
