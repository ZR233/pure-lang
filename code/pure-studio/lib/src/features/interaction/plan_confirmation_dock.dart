import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import '../../shared/studio_driver_state.dart';
import 'interaction_widgets.dart';

class PlanConfirmationDock extends ConsumerStatefulWidget {
  const PlanConfirmationDock({
    required this.threadId,
    required this.planContent,
    this.trailing,
    super.key,
  });

  final String threadId;
  final String planContent;
  final Widget? trailing;

  @override
  ConsumerState<PlanConfirmationDock> createState() =>
      _PlanConfirmationDockState();
}

class _PlanConfirmationDockState extends ConsumerState<PlanConfirmationDock> {
  late final TextEditingController _adjustmentController;
  bool _submitting = false;
  String? _error;

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
    StudioDriverState.publishPlan(widget.planContent);
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
            enabled: !_submitting,
            canSubmit: canSubmitAdjustment && !_submitting,
            onChanged: () => setState(() {}),
            onSubmit: _resolveContinuePlanning,
          ),
          if (_error case final error?) ...[
            const SizedBox(height: 10),
            _PlanResolutionError(message: error),
          ],
          const SizedBox(height: 12),
          _PlanDecisionActions(
            onDismiss: _submitting ? null : _resolveDismiss,
            onImplement: _submitting ? null : _resolveImplement,
          ),
        ],
      ),
    );
  }

  void _resolveImplement() {
    unawaited(
      _resolve(
        const PlanConfirmationResolutionCommand(
          decision: PlanConfirmationDecision.implementFreshContext,
        ),
      ),
    );
  }

  void _resolveContinuePlanning() {
    final content = _adjustmentController.text.trim();
    if (content.isEmpty) {
      return;
    }
    unawaited(
      _resolve(
        PlanConfirmationResolutionCommand(
          decision: PlanConfirmationDecision.continuePlanning,
          content: content,
          reason: 'continue planning',
        ),
      ),
    );
  }

  void _resolveDismiss() {
    unawaited(
      _resolve(
        const PlanConfirmationResolutionCommand(
          decision: PlanConfirmationDecision.dismiss,
          reason: 'dismissed',
        ),
      ),
    );
  }

  Future<void> _resolve(InteractionResolutionCommand resolution) async {
    if (_submitting) {
      return;
    }
    setState(() {
      _submitting = true;
      _error = null;
    });
    try {
      await ref
          .read(studioControllerProvider.notifier)
          .resolveActiveInteraction(widget.threadId, resolution);
    } catch (error) {
      if (mounted) {
        setState(() => _error = error.toString());
      }
    } finally {
      if (mounted) {
        setState(() => _submitting = false);
      }
    }
  }
}

class _PlanResolutionError extends StatelessWidget {
  const _PlanResolutionError({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Container(
      key: const Key('plan-confirmation-error'),
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
      decoration: BoxDecoration(
        color: colors.errorContainer.withValues(alpha: 0.55),
        border: Border.all(color: colors.error.withValues(alpha: 0.35)),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(Icons.error_outline, size: 18, color: colors.error),
          const SizedBox(width: 8),
          Expanded(
            child: SelectableText(
              message,
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: colors.onErrorContainer),
            ),
          ),
        ],
      ),
    );
  }
}

class _PlanDecisionActions extends StatelessWidget {
  const _PlanDecisionActions({
    required this.onDismiss,
    required this.onImplement,
  });

  final VoidCallback? onDismiss;
  final VoidCallback? onImplement;

  @override
  Widget build(BuildContext context) {
    final hint = Text(
      context.l10n.interactionPlanImplementFooterHint(
        context.compileModeLabel(StudioMode.task),
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
          key: StudioDriverKeys.planImplement,
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
    required this.enabled,
    required this.canSubmit,
    required this.onChanged,
    required this.onSubmit,
  });

  final TextEditingController controller;
  final bool enabled;
  final bool canSubmit;
  final VoidCallback onChanged;
  final VoidCallback onSubmit;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 560;
        final input = TextField(
          key: StudioDriverKeys.planAdjustmentInput,
          controller: controller,
          enabled: enabled,
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
          key: StudioDriverKeys.planContinue,
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
