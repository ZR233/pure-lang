import 'package:flutter/material.dart';

import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';

class QuestionProgress extends StatelessWidget {
  const QuestionProgress({
    super.key,
    required this.total,
    required this.currentIndex,
    required this.answeredCount,
    required this.questions,
    required this.answered,
    required this.onSelected,
  });

  final int total;
  final int currentIndex;
  final int answeredCount;
  final List<UserQuestionView> questions;
  final bool Function(UserQuestionView question) answered;
  final ValueChanged<int> onSelected;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: Row(
        children: [
          Text(
            context.l10n.interactionQuestionProgress(currentIndex + 1, total),
            style: Theme.of(context).textTheme.labelMedium
                ?.copyWith(color: colors.onSurfaceVariant),
          ),
          const SizedBox(width: 10),
          for (var index = 0; index < questions.length; index++)
            Padding(
              padding: EdgeInsets.only(
                right: index == questions.length - 1 ? 0 : 6,
              ),
              child: _ProgressDot(
                index: index,
                active: index == currentIndex,
                answered: answered(questions[index]),
                onPressed: () => onSelected(index),
              ),
            ),
          const SizedBox(width: 10),
          Text(
            context.l10n.interactionAnsweredCount(answeredCount),
            style: Theme.of(context).textTheme.labelSmall
                ?.copyWith(color: colors.onSurfaceVariant),
          ),
        ],
      ),
    );
  }
}

class _ProgressDot extends StatelessWidget {
  const _ProgressDot({
    required this.index,
    required this.active,
    required this.answered,
    required this.onPressed,
  });

  final int index;
  final bool active;
  final bool answered;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final background = active
        ? colors.primary
        : answered
        ? colors.primary.withValues(alpha: 0.42)
        : colors.surfaceContainerHighest;
    return Tooltip(
      message: context.l10n.interactionQuestionTooltip(index + 1),
      child: InkResponse(
        onTap: onPressed,
        radius: 12,
        child: AnimatedContainer(
          duration: const Duration(milliseconds: 140),
          width: active ? 18 : 8,
          height: 8,
          decoration: BoxDecoration(
            color: background,
            borderRadius: BorderRadius.circular(999),
            border: Border.all(color: colors.outlineVariant),
          ),
        ),
      ),
    );
  }
}
