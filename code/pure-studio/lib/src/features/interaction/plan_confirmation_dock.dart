import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'interaction_widgets.dart';

class PlanConfirmationDock extends ConsumerStatefulWidget {
  const PlanConfirmationDock({
    required this.threadId,
    required this.plan,
    required this.enabled,
    this.trailing,
    super.key,
  });

  final String threadId;
  final PlanConfirmationView plan;
  final bool enabled;
  final Widget? trailing;

  @override
  ConsumerState<PlanConfirmationDock> createState() =>
      _PlanConfirmationDockState();
}

class _PlanConfirmationDockState extends ConsumerState<PlanConfirmationDock> {
  late final TextEditingController _feedbackController;

  @override
  void initState() {
    super.initState();
    _feedbackController = TextEditingController();
  }

  @override
  void didUpdateWidget(covariant PlanConfirmationDock oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.plan.interactionId != widget.plan.interactionId) {
      _feedbackController.clear();
    }
  }

  @override
  void dispose() {
    _feedbackController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final feedback = _feedbackController.text.trim();
    return InteractionDockShell(
      kind: InteractionDockKind.plan,
      trailing: widget.trailing,
      title: context.l10n.interactionPlanConfirmTitle,
      subtitle: context.l10n.interactionPlanConfirmSubtitle,
      footerHint: context.l10n.interactionPlanComposerPausedHint,
      footer: DockActions(
        children: [
          OutlinedButton.icon(
            key: StudioDriverKeys.planSubmitRevision,
            icon: const Icon(Icons.send_outlined),
            label: Text(context.l10n.interactionPlanAdjustSubmit),
            onPressed: widget.enabled && feedback.isNotEmpty
                ? _submitRevision
                : null,
          ),
          FilledButton.icon(
            key: StudioDriverKeys.planApprove,
            icon: const Icon(Icons.check),
            label: Text(context.l10n.interactionPlanConfirmAction),
            onPressed: widget.enabled ? _approve : null,
          ),
        ],
      ),
      child: TextField(
        key: StudioDriverKeys.planFeedbackInput,
        controller: _feedbackController,
        enabled: widget.enabled,
        minLines: 1,
        maxLines: 4,
        decoration: InputDecoration(
          labelText: context.l10n.interactionPlanAdjust,
          hintText: context.l10n.interactionPlanAdjustHint,
          prefixIcon: const Icon(Icons.edit_note_outlined),
        ),
        onChanged: (_) => setState(() {}),
      ),
    );
  }

  void _submitRevision() {
    final feedback = _feedbackController.text.trim();
    if (feedback.isEmpty) return;
    _resolve([agentSessionPlanReviseAnswer, feedback]);
  }

  void _approve() {
    _resolve(const [agentSessionPlanApproveAnswer]);
  }

  void _resolve(List<String> answers) {
    ref
        .read(studioControllerProvider.notifier)
        .resolveActiveInteraction(
          widget.threadId,
          widget.plan.interactionId,
          UserInputResolutionCommand(
            answers: [
              UserInputAnswerCommand(
                questionId: widget.plan.questionId,
                answers: answers,
              ),
            ],
          ),
        );
  }
}
