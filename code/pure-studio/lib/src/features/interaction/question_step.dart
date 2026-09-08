import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'interaction_widgets.dart';

class QuestionStep extends StatelessWidget {
  const QuestionStep({
    super.key,
    required this.question,
    required this.questionKey,
    required this.isFirstQuestion,
    required this.firstOptionKey,
    required this.controller,
    required this.selected,
    required this.onOptionChanged,
    required this.onTextChanged,
    required this.enabled,
  });

  final UserQuestionView question;
  final String questionKey;
  final bool isFirstQuestion;
  final GlobalKey firstOptionKey;
  final TextEditingController controller;
  final Set<String> selected;
  final void Function(String label, bool selected) onOptionChanged;
  final ValueChanged<String> onTextChanged;
  final bool enabled;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          question.header.isEmpty
              ? context.l10n.interactionQuestionFallback
              : question.header,
          style: Theme.of(context).textTheme.labelLarge,
        ),
        if (question.question.isNotEmpty) ...[
          const SizedBox(height: 4),
          Text(question.question),
        ],
        if (question.options.isNotEmpty) ...[
          const SizedBox(height: 10),
          for (final (optionIndex, option) in question.options.indexed)
            Padding(
              key: optionIndex == 0 && isFirstQuestion
                  ? StudioDriverKeys.userInputFirstOption
                  : StudioDriverKeys.userInputOption(questionKey, optionIndex),
              padding: const EdgeInsets.only(bottom: 7),
              child: KeyedSubtree(
                key: optionIndex == 0 && isFirstQuestion
                    ? firstOptionKey
                    : null,
                child: _QuestionOptionRow(
                  option: option,
                  selected: selected.contains(option.label),
                  onChanged: enabled
                      ? (value) => onOptionChanged(option.label, value)
                      : null,
                ),
              ),
            ),
        ],
        if (question.isOther || question.options.isEmpty) ...[
          const SizedBox(height: 8),
          TextField(
            key: isFirstQuestion
                ? StudioDriverKeys.userInputFirstText
                : StudioDriverKeys.userInputText(questionKey),
            controller: controller,
            enabled: enabled,
            obscureText: question.isSecret,
            minLines: 1,
            maxLines: question.isSecret ? 1 : 4,
            decoration: InputDecoration(
              labelText: question.isOther
                  ? context.l10n.interactionOtherLabel
                  : context.l10n.interactionAnswerLabel,
              hintText: question.isSecret
                  ? context.l10n.interactionSecretHint
                  : context.l10n.interactionTextHint,
              prefixIcon: Icon(
                question.isSecret
                    ? Icons.password_outlined
                    : Icons.short_text_outlined,
              ),
            ),
            onChanged: onTextChanged,
          ),
        ],
      ],
    );
  }
}

class _QuestionOptionRow extends StatelessWidget {
  const _QuestionOptionRow({
    required this.option,
    required this.selected,
    required this.onChanged,
  });

  final UserQuestionOptionView option;
  final bool selected;
  final ValueChanged<bool>? onChanged;

  @override
  Widget build(BuildContext context) {
    return IgnorePointer(
      ignoring: onChanged == null,
      child: Opacity(
        opacity: onChanged == null ? 0.55 : 1,
        child: DockOptionRow(
          title: option.label,
          subtitle: option.description,
          selected: selected,
          onPressed: () => onChanged?.call(!selected),
          leading: _OptionMark(selected: selected),
        ),
      ),
    );
  }
}

class _OptionMark extends StatelessWidget {
  const _OptionMark({required this.selected});

  final bool selected;

  @override
  Widget build(BuildContext context) {
    return AnimatedContainer(
      duration: const Duration(milliseconds: 140),
      width: 20,
      height: 20,
      margin: const EdgeInsets.only(top: 1),
      decoration: BoxDecoration(
        color: selected ? context.colors.primary : context.colors.surface,
        borderRadius: BorderRadius.circular(5),
        border: Border.all(
          color: selected ? StudioColors.clay : context.studioLine2,
          width: 2,
        ),
      ),
      child: selected
          ? const Icon(Icons.check, size: 13, color: StudioColors.white)
          : null,
    );
  }
}
