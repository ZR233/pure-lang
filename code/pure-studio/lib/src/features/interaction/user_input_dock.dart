import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'interaction_payload.dart';
import 'interaction_widgets.dart';

class UserInputDock extends ConsumerStatefulWidget {
  const UserInputDock({
    required this.threadId,
    required this.interactionId,
    required this.payload,
    this.trailing,
    super.key,
  });

  final String threadId;
  final String interactionId;
  final InteractionPayloadSnapshot payload;
  final Widget? trailing;

  @override
  ConsumerState<UserInputDock> createState() => _UserInputDockState();
}

class _UserInputDockState extends ConsumerState<UserInputDock> {
  late final TextEditingController _fallbackController;
  final Map<String, TextEditingController> _textControllers = {};
  final Map<String, Set<String>> _selectedOptions = {};
  int _index = 0;

  @override
  void initState() {
    super.initState();
    _fallbackController = TextEditingController();
    _resetQuestionDraft(widget.payload.questions);
  }

  @override
  void didUpdateWidget(covariant UserInputDock oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_dockSignature(oldWidget.interactionId, oldWidget.payload.questions) !=
        _dockSignature(widget.interactionId, widget.payload.questions)) {
      _resetQuestionDraft(widget.payload.questions);
    }
  }

  @override
  void dispose() {
    _fallbackController.dispose();
    for (final controller in _textControllers.values) {
      controller.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final questions = widget.payload.questions;
    if (questions.isEmpty) {
      return _FallbackQuestionDock(
        body: widget.payload.rawBody,
        controller: _fallbackController,
        trailing: widget.trailing,
        onChanged: () => setState(() {}),
        onSubmit: _fallbackController.text.trim().isEmpty
            ? null
            : _submitFallbackAnswer,
      );
    }

    final index = _index.clamp(0, questions.length - 1).toInt();
    if (index != _index) {
      _index = index;
    }
    final question = questions[index];
    final key = _questionKey(question, index);
    final total = questions.length;
    final answeredCount = questions.where(_answered).length;
    final isLast = index >= total - 1;
    return InteractionDockShell(
      kind: InteractionDockKind.question,
      trailing: widget.trailing,
      title: context.l10n.interactionQuestionsTitle,
      subtitle: isLast
          ? context.l10n.interactionLastQuestion
          : context.l10n.interactionContinueAfterAnswer,
      footerHint: isLast
          ? context.l10n.interactionSubmitEmptyAnswersHint
          : context.l10n.interactionAnsweredPendingHint(
              answeredCount,
              total - answeredCount,
            ),
      footer: DockActions(
        children: [
          if (index > 0)
            OutlinedButton.icon(
              icon: const Icon(Icons.chevron_left),
              label: Text(context.l10n.interactionPreviousQuestion),
              onPressed: () => setState(() => _index -= 1),
            ),
          FilledButton.icon(
            key: StudioDriverKeys.userInputSubmit,
            icon: Icon(isLast ? Icons.check : Icons.chevron_right),
            label: Text(
              isLast
                  ? context.l10n.interactionSubmitAnswers
                  : context.l10n.interactionNextQuestion,
            ),
            onPressed: () {
              if (isLast) {
                _submitAnswers();
              } else {
                setState(() => _index += 1);
              }
            },
          ),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          _QuestionProgress(
            total: total,
            currentIndex: index,
            answeredCount: answeredCount,
            questions: questions,
            answered: _answered,
            onSelected: (value) => setState(() => _index = value),
          ),
          const SizedBox(height: 12),
          _QuestionStep(
            question: question,
            questionKey: key,
            controller: _textControllers[key]!,
            selected: _selectedOptions[key] ?? <String>{},
            onOptionChanged: (label, selected) {
              setState(() {
                final values = _selectedOptions[key] ?? <String>{};
                if (selected) {
                  values.add(label);
                } else {
                  values.remove(label);
                }
                _selectedOptions[key] = values;
              });
            },
            onTextChanged: (_) => setState(() {}),
          ),
        ],
      ),
    );
  }

  void _resetQuestionDraft(List<UserQuestionView> questions) {
    for (final controller in _textControllers.values) {
      controller.dispose();
    }
    _textControllers
      ..clear()
      ..addEntries(
        questions.indexed.map(
          (entry) => MapEntry(
            _questionKey(entry.$2, entry.$1),
            TextEditingController(),
          ),
        ),
      );
    _selectedOptions
      ..clear()
      ..addEntries(
        questions.indexed.map(
          (entry) => MapEntry(_questionKey(entry.$2, entry.$1), <String>{}),
        ),
      );
    _index = 0;
  }

  bool _answered(UserQuestionView question) {
    final index = widget.payload.questions.indexOf(question);
    final key = _questionKey(question, index);
    final selected = _selectedOptions[key] ?? const <String>{};
    final text = _textControllers[key]?.text.trim() ?? '';
    return selected.isNotEmpty || text.isNotEmpty;
  }

  List<UserInputAnswerCommand> _answers() {
    final answers = <UserInputAnswerCommand>[];
    for (final entry in widget.payload.questions.indexed) {
      final index = entry.$1;
      final question = entry.$2;
      final key = _questionKey(question, index);
      final values = <String>[...(_selectedOptions[key] ?? const <String>{})];
      final text = _textControllers[key]?.text.trim() ?? '';
      if ((question.isOther || question.options.isEmpty) && text.isNotEmpty) {
        values.add(text);
      }
      answers.add(UserInputAnswerCommand(questionId: key, answers: values));
    }
    return answers;
  }

  void _submitFallbackAnswer() {
    final text = _fallbackController.text.trim();
    if (text.isEmpty) {
      return;
    }
    ref
        .read(studioControllerProvider.notifier)
        .resolveActiveInteraction(
          widget.threadId,
          UserInputResolutionCommand(
            answers: [
              UserInputAnswerCommand(questionId: 'answer', answers: [text]),
            ],
          ),
        );
  }

  void _submitAnswers() {
    ref
        .read(studioControllerProvider.notifier)
        .resolveActiveInteraction(
          widget.threadId,
          UserInputResolutionCommand(answers: _answers()),
        );
  }
}

class _FallbackQuestionDock extends StatelessWidget {
  const _FallbackQuestionDock({
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

class _QuestionProgress extends StatelessWidget {
  const _QuestionProgress({
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

class _QuestionStep extends StatelessWidget {
  const _QuestionStep({
    required this.question,
    required this.questionKey,
    required this.controller,
    required this.selected,
    required this.onOptionChanged,
    required this.onTextChanged,
  });

  final UserQuestionView question;
  final String questionKey;
  final TextEditingController controller;
  final Set<String> selected;
  final void Function(String label, bool selected) onOptionChanged;
  final ValueChanged<String> onTextChanged;

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
              padding: const EdgeInsets.only(bottom: 7),
              child: _QuestionOptionRow(
                key: StudioDriverKeys.userInputOption(questionKey, optionIndex),
                option: option,
                selected: selected.contains(option.label),
                onChanged: (value) => onOptionChanged(option.label, value),
              ),
            ),
        ],
        if (question.isOther || question.options.isEmpty) ...[
          const SizedBox(height: 8),
          TextField(
            key: StudioDriverKeys.userInputText(questionKey),
            controller: controller,
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
    super.key,
  });

  final UserQuestionOptionView option;
  final bool selected;
  final ValueChanged<bool> onChanged;

  @override
  Widget build(BuildContext context) {
    return DockOptionRow(
      title: option.label,
      subtitle: option.description,
      selected: selected,
      onPressed: () => onChanged(!selected),
      leading: _OptionMark(selected: selected),
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
        color: selected ? StudioColors.clay : StudioColors.white,
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

String _dockSignature(String interactionId, List<UserQuestionView> questions) {
  return [
    interactionId,
    ...questions.indexed.map(
      (entry) => [
        _questionKey(entry.$2, entry.$1),
        entry.$2.header,
        entry.$2.question,
        entry.$2.isOther,
        entry.$2.isSecret,
        for (final option in entry.$2.options)
          '${option.label}\u{1f}${option.description}',
      ].join('\u{1e}'),
    ),
  ].join('\u{1d}');
}

String _questionKey(UserQuestionView question, int index) {
  final id = question.id.trim();
  return id.isEmpty ? 'answer_$index' : id;
}
