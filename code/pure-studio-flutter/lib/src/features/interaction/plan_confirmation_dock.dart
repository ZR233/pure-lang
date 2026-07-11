import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
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
      title: context.l10n.interactionPlanConfirmTitle,
      subtitle: context.l10n.interactionPlanConfirmSubtitle,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          _PlanAdjustmentInput(
            controller: _adjustmentController,
            canSubmit: canSubmitAdjustment,
            onChanged: () => setState(() {}),
            onSubmit: _resolveContinuePlanning,
          ),
          const SizedBox(height: 12),
          _PlanDecisionActions(
            onDismiss: _resolveDismiss,
            onImplement: _resolveImplement,
          ),
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

class _PlanDecisionActions extends StatelessWidget {
  const _PlanDecisionActions({
    required this.onDismiss,
    required this.onImplement,
  });

  final VoidCallback onDismiss;
  final VoidCallback onImplement;

  @override
  Widget build(BuildContext context) {
    final hint = Text(
      context.l10n.interactionPlanImplementFooterHint(
        context.compileModeLabel(CompileMode.task),
      ),
      maxLines: 2,
      overflow: TextOverflow.ellipsis,
      style: Theme.of(context).textTheme.labelSmall?.copyWith(
        color: context.studioInkSoft.withValues(alpha: 0.66),
      ),
    );
    final actions = DockActions(
      children: [
        TextButton.icon(
          icon: const Icon(Icons.close),
          label: Text(context.l10n.interactionPlanIgnore),
          onPressed: onDismiss,
        ),
        FilledButton.icon(
          icon: const Icon(Icons.play_arrow),
          label: Text(context.l10n.interactionPlanImplement),
          onPressed: onImplement,
        ),
      ],
    );

    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 560;
        if (compact) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            mainAxisSize: MainAxisSize.min,
            children: [
              hint,
              const SizedBox(height: 8),
              Align(alignment: Alignment.centerRight, child: actions),
            ],
          );
        }

        return Row(
          crossAxisAlignment: CrossAxisAlignment.end,
          children: [
            Expanded(child: hint),
            const SizedBox(width: 12),
            Flexible(child: actions),
          ],
        );
      },
    );
  }
}

class _PlanAdjustmentInput extends StatelessWidget {
  const _PlanAdjustmentInput({
    required this.controller,
    required this.canSubmit,
    required this.onChanged,
    required this.onSubmit,
  });

  final TextEditingController controller;
  final bool canSubmit;
  final VoidCallback onChanged;
  final VoidCallback onSubmit;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 560;
        final input = TextField(
          controller: controller,
          minLines: 1,
          maxLines: 4,
          textInputAction: TextInputAction.send,
          decoration: InputDecoration(
            hintText: context.l10n.interactionPlanAdjustHint,
            prefixIcon: const Icon(Icons.edit_note),
            filled: true,
            isDense: true,
          ),
          onChanged: (_) => onChanged(),
          onSubmitted: (_) {
            if (canSubmit) {
              onSubmit();
            }
          },
        );
        final submit = FilledButton.tonalIcon(
          icon: const Icon(Icons.send),
          label: Text(context.l10n.interactionPlanAdjustSubmit),
          onPressed: canSubmit ? onSubmit : null,
        );

        if (compact) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            mainAxisSize: MainAxisSize.min,
            children: [
              input,
              const SizedBox(height: 8),
              Align(alignment: Alignment.centerRight, child: submit),
            ],
          );
        }

        return Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(child: input),
            const SizedBox(width: 10),
            Padding(padding: const EdgeInsets.only(top: 1), child: submit),
          ],
        );
      },
    );
  }
}
