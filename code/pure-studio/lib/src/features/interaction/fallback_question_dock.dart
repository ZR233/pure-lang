import 'package:flutter/material.dart';

import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'interaction_widgets.dart';

class FallbackQuestionDock extends StatelessWidget {
  const FallbackQuestionDock({
    super.key,
    required this.body,
    required this.controller,
    required this.trailing,
    required this.onChanged,
    required this.onSubmit,
  });

  final String body;
  final TextEditingController controller;
  final Widget? trailing;
  final VoidCallback onChanged;
  final VoidCallback? onSubmit;

  @override
  Widget build(BuildContext context) {
    return InteractionDockShell(
      kind: InteractionDockKind.question,
      trailing: trailing,
      title: context.l10n.interactionNeedInputTitle,
      subtitle: context.l10n.interactionContinueAfterAnswer,
      footerHint: context.l10n.interactionAnswerHint,
      footer: DockActions(
        children: [
          FilledButton.icon(
            key: StudioDriverKeys.fallbackUserInputSubmit,
            icon: const Icon(Icons.reply),
            label: Text(context.l10n.interactionAnswerButton),
            onPressed: onSubmit,
          ),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          if (body.trim().isNotEmpty) ...[
            Text(body.trim()),
            const SizedBox(height: 10),
          ],
          TextField(
            key: StudioDriverKeys.fallbackUserInput,
            controller: controller,
            minLines: 1,
            maxLines: 4,
            decoration: InputDecoration(
              labelText: context.l10n.interactionAnswerLabel,
              prefixIcon: const Icon(Icons.short_text_outlined),
            ),
            onChanged: (_) => onChanged(),
            onSubmitted: (_) => onSubmit?.call(),
          ),
        ],
      ),
    );
  }
}
