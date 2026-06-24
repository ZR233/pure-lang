import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_chrome.dart';
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
      title: context.l10n.interactionPlanConfirmTitle,
      subtitle: context.l10n.interactionPlanConfirmSubtitle,
      footerHint: _editingAdjustment
          ? context.l10n.interactionPlanEditingFooterHint
          : context.l10n.interactionPlanImplementFooterHint,
      footer: DockActions(
        children: [
          TextButton.icon(
            icon: const Icon(Icons.close),
            label: Text(context.l10n.interactionPlanIgnore),
            onPressed: _resolveDismiss,
          ),
          if (!_editingAdjustment)
            OutlinedButton.icon(
              icon: const Icon(Icons.edit_note),
              label: Text(context.l10n.interactionPlanAdjust),
              onPressed: () => setState(() => _editingAdjustment = true),
            ),
          FilledButton.icon(
            icon: Icon(_editingAdjustment ? Icons.check : Icons.play_arrow),
            label: Text(
              _editingAdjustment
                  ? context.l10n.interactionPlanAdjustSubmit
                  : context.l10n.interactionPlanImplement,
            ),
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
          StudioNotice(
            icon: _editingAdjustment
                ? Icons.edit_note_outlined
                : Icons.info_outline,
            message: _editingAdjustment
                ? context.l10n.interactionPlanEditingNotice
                : context.l10n.interactionPlanViewNotice,
            tone: StudioNoticeTone.warning,
            iconSize: 17,
            padding: const EdgeInsets.fromLTRB(12, 9, 12, 9),
          ),
          if (_editingAdjustment)
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: TextField(
                controller: _adjustmentController,
                autofocus: true,
                minLines: 2,
                maxLines: 5,
                decoration: InputDecoration(
                  hintText: context.l10n.interactionPlanAdjustHint,
                  alignLabelWithHint: true,
                  prefixIcon: const Icon(Icons.edit_note),
                ),
                onChanged: (_) => setState(() {}),
                onSubmitted: (_) {
                  if (canSubmitAdjustment) {
                    _resolveContinuePlanning();
                  }
                },
              ),
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
