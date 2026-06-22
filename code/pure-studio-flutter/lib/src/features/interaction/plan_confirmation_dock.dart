import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import 'interaction_widgets.dart';

class PlanConfirmationDock extends ConsumerStatefulWidget {
  const PlanConfirmationDock({this.trailing, super.key});

  final Widget? trailing;

  @override
  ConsumerState<PlanConfirmationDock> createState() =>
      _PlanConfirmationDockState();
}

class _PlanConfirmationDockState extends ConsumerState<PlanConfirmationDock> {
  late final TextEditingController _adjustmentController;
  bool _editingAdjustment = false;

  @override
  void initState() {
    super.initState();
    _adjustmentController = TextEditingController();
  }

  @override
  void dispose() {
    _adjustmentController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final canSubmitAdjustment = _adjustmentController.text.trim().isNotEmpty;
    return InteractionDockShell(
      kind: InteractionDockKind.plan,
      trailing: widget.trailing,
      header: const Text('Implement this plan?'),
      footer: DockActions(
        children: [
          TextButton.icon(
            icon: const Icon(Icons.close),
            label: const Text('Ignore'),
            onPressed: _resolveDismiss,
          ),
          FilledButton.icon(
            icon: Icon(_editingAdjustment ? Icons.check : Icons.play_arrow),
            label: Text(_editingAdjustment ? 'Submit' : 'Implement plan'),
            onPressed: _editingAdjustment
                ? (canSubmitAdjustment ? _resolveContinuePlanning : null)
                : _resolveImplement,
          ),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          _PlanChoiceTile(
            index: 1,
            icon: Icons.check_circle_outline,
            title: 'Yes, implement this plan',
            subtitle: 'Start implementation in the current session.',
            selected: !_editingAdjustment,
            onPressed: _resolveImplement,
          ),
          const SizedBox(height: 8),
          _PlanChoiceTile(
            index: 2,
            icon: Icons.edit_note,
            title: 'No, tell Pure what to adjust',
            subtitle: 'Keep planning with a short instruction.',
            selected: _editingAdjustment,
            onPressed: () => setState(() => _editingAdjustment = true),
          ),
          if (_editingAdjustment) ...[
            const SizedBox(height: 8),
            TextField(
              controller: _adjustmentController,
              autofocus: true,
              minLines: 2,
              maxLines: 5,
              decoration: const InputDecoration(
                hintText: 'Tell Pure how to adjust the plan...',
                alignLabelWithHint: true,
              ),
              onChanged: (_) => setState(() {}),
              onSubmitted: (_) {
                if (canSubmitAdjustment) {
                  _resolveContinuePlanning();
                }
              },
            ),
          ],
        ],
      ),
    );
  }

  void _resolveImplement() {
    ref.read(studioControllerProvider.notifier).resolveActiveInteraction({
      'type': 'planConfirmation',
      'decision': 'implementFreshContext',
    });
  }

  void _resolveContinuePlanning() {
    final content = _adjustmentController.text.trim();
    if (content.isEmpty) {
      return;
    }
    ref.read(studioControllerProvider.notifier).resolveActiveInteraction({
      'type': 'planConfirmation',
      'decision': 'continuePlanning',
      'content': content,
      'reason': 'continue planning',
    });
  }

  void _resolveDismiss() {
    ref.read(studioControllerProvider.notifier).resolveActiveInteraction({
      'type': 'planConfirmation',
      'decision': 'dismiss',
      'reason': 'dismissed',
    });
  }
}

class _PlanChoiceTile extends StatelessWidget {
  const _PlanChoiceTile({
    required this.index,
    required this.icon,
    required this.title,
    required this.subtitle,
    required this.selected,
    required this.onPressed,
  });

  final int index;
  final IconData icon;
  final String title;
  final String subtitle;
  final bool selected;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 360;
        return Material(
          color: selected
              ? colors.primaryContainer.withValues(alpha: 0.35)
              : colors.surfaceContainerLowest,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(8),
            side: BorderSide(
              color: selected ? colors.primary : colors.outlineVariant,
            ),
          ),
          clipBehavior: Clip.antiAlias,
          child: InkWell(
            onTap: onPressed,
            child: Padding(
              padding: const EdgeInsets.fromLTRB(10, 9, 10, 9),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  SizedBox.square(
                    dimension: 24,
                    child: Center(
                      child: Text(
                        '$index',
                        style: Theme.of(context).textTheme.labelMedium
                            ?.copyWith(
                              color: selected
                                  ? colors.primary
                                  : colors.onSurfaceVariant,
                            ),
                      ),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Padding(
                    padding: const EdgeInsets.only(top: 2),
                    child: Icon(
                      icon,
                      size: 18,
                      color: selected
                          ? colors.primary
                          : colors.onSurfaceVariant,
                    ),
                  ),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          title,
                          maxLines: compact ? 2 : 1,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.labelLarge,
                        ),
                        const SizedBox(height: 2),
                        Text(
                          subtitle,
                          maxLines: compact ? 2 : 1,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.bodySmall
                              ?.copyWith(color: colors.onSurfaceVariant),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
        );
      },
    );
  }
}
