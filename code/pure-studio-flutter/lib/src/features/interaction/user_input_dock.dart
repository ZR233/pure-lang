import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import 'interaction_payload.dart';
import 'interaction_widgets.dart';

class UserInputDock extends ConsumerStatefulWidget {
  const UserInputDock({required this.payload, this.trailing, super.key});

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
    if (_questionSignature(oldWidget.payload.questions) !=
        _questionSignature(widget.payload.questions)) {
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
    final total = questions.length;
    final answeredCount = questions.where(_answered).length;
    final isLast = index >= total - 1;
    return InteractionDockShell(
      kind: InteractionDockKind.question,
      trailing: widget.trailing,
      header: _QuestionHeader(
        total: total,
        currentIndex: index,
        answeredCount: answeredCount,
        questions: questions,
        answered: _answered,
        onSelected: (value) => setState(() => _index = value),
      ),
      footer: DockActions(
        children: [
          if (index > 0)
            OutlinedButton.icon(
              icon: const Icon(Icons.chevron_left),
              label: const Text('Back'),
              onPressed: () => setState(() => _index -= 1),
            ),
          FilledButton.icon(
            icon: Icon(isLast ? Icons.check : Icons.chevron_right),
            label: Text(isLast ? 'Submit answers' : 'Next'),
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
      child: _QuestionStep(
        question: question,
        controller: _textControllers[question.id]!,
        selected: _selectedOptions[question.id] ?? <String>{},
        onOptionChanged: (label, selected) {
          setState(() {
            final values = _selectedOptions[question.id] ?? <String>{};
            if (selected) {
              values.add(label);
            } else {
              values.remove(label);
            }
            _selectedOptions[question.id] = values;
          });
        },
        onTextChanged: (_) => setState(() {}),
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
        questions.map(
          (question) => MapEntry(question.id, TextEditingController()),
        ),
      );
    _selectedOptions
      ..clear()
      ..addEntries(
        questions.map((question) => MapEntry(question.id, <String>{})),
      );
    _index = 0;
  }

  bool _answered(UserQuestionView question) {
    final selected = _selectedOptions[question.id] ?? const <String>{};
    final text = _textControllers[question.id]?.text.trim() ?? '';
    return selected.isNotEmpty || text.isNotEmpty;
  }

  Map<String, Object> _answers() {
    final answers = <String, Object>{};
    for (final question in widget.payload.questions) {
      final values = <String>[
        ...(_selectedOptions[question.id] ?? const <String>{}),
      ];
      final text = _textControllers[question.id]?.text.trim() ?? '';
      if ((question.isOther || question.options.isEmpty) && text.isNotEmpty) {
        values.add(text);
      }
      answers[question.id] = {'answers': values};
    }
    return answers;
  }

  void _submitFallbackAnswer() {
    final text = _fallbackController.text.trim();
    if (text.isEmpty) {
      return;
    }
    ref.read(studioControllerProvider.notifier).resolveActiveInteraction({
      'type': 'userInput',
      'answers': {
        'answer': {
          'answers': [text],
        },
      },
    });
  }

  void _submitAnswers() {
    ref.read(studioControllerProvider.notifier).resolveActiveInteraction({
      'type': 'userInput',
      'answers': _answers(),
    });
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
      header: const Text('Assistant needs input'),
      footer: DockActions(
        children: [
          FilledButton.icon(
            icon: const Icon(Icons.reply),
            label: const Text('Answer'),
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
            decoration: const InputDecoration(
              labelText: 'Answer',
              prefixIcon: Icon(Icons.short_text_outlined),
            ),
            onChanged: (_) => onChanged(),
            onSubmitted: (_) => onSubmit?.call(),
          ),
        ],
      ),
    );
  }
}

class _QuestionHeader extends StatelessWidget {
  const _QuestionHeader({
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
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                '$total ${total == 1 ? 'question' : 'questions'}',
                overflow: TextOverflow.ellipsis,
              ),
            ),
            Text(
              'Question ${currentIndex + 1} / $total',
              style: Theme.of(
                context,
              ).textTheme.labelSmall?.copyWith(color: colors.onSurfaceVariant),
            ),
          ],
        ),
        const SizedBox(height: 8),
        Row(
          children: [
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
              '$answeredCount answered',
              style: Theme.of(
                context,
              ).textTheme.labelSmall?.copyWith(color: colors.onSurfaceVariant),
            ),
          ],
        ),
      ],
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
      message: 'Question ${index + 1}',
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
    required this.controller,
    required this.selected,
    required this.onOptionChanged,
    required this.onTextChanged,
  });

  final UserQuestionView question;
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
          question.header.isEmpty ? 'Question' : question.header,
          style: Theme.of(context).textTheme.labelLarge,
        ),
        if (question.question.isNotEmpty) ...[
          const SizedBox(height: 4),
          Text(question.question),
        ],
        if (question.options.isNotEmpty) ...[
          const SizedBox(height: 10),
          for (final option in question.options)
            Padding(
              padding: const EdgeInsets.only(bottom: 7),
              child: _QuestionOptionRow(
                option: option,
                selected: selected.contains(option.label),
                onChanged: (value) => onOptionChanged(option.label, value),
              ),
            ),
        ],
        if (question.isOther || question.options.isEmpty) ...[
          const SizedBox(height: 8),
          TextField(
            controller: controller,
            obscureText: question.isSecret,
            minLines: 1,
            maxLines: question.isSecret ? 1 : 4,
            decoration: InputDecoration(
              labelText: question.isOther ? 'Other' : 'Answer',
              hintText: question.isSecret
                  ? 'Enter secret answer'
                  : 'Type your answer...',
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
  final ValueChanged<bool> onChanged;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Material(
      color: selected
          ? colors.primaryContainer.withValues(alpha: 0.3)
          : colors.surfaceContainerLowest,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(8),
        side: BorderSide(
          color: selected ? colors.primary : colors.outlineVariant,
        ),
      ),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: () => onChanged(!selected),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(10, 8, 12, 8),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              SizedBox.square(
                dimension: 22,
                child: Checkbox(
                  value: selected,
                  onChanged: (value) => onChanged(value ?? false),
                ),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      option.label,
                      style: Theme.of(context).textTheme.labelLarge,
                    ),
                    if (option.description.isNotEmpty) ...[
                      const SizedBox(height: 2),
                      Text(
                        option.description,
                        style: Theme.of(context).textTheme.bodySmall?.copyWith(
                          color: colors.onSurfaceVariant,
                        ),
                      ),
                    ],
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

String _questionSignature(List<UserQuestionView> questions) {
  return questions
      .map((question) => '${question.id}:${question.options.length}')
      .join('|');
}
