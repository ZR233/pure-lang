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
      title: '几个问题想确认',
      subtitle: isLast ? '最后一题' : '回答后继续',
      footerHint: isLast
          ? '提交后未答问题保留空数组'
          : '已答 $answeredCount 题 · ${total - answeredCount} 题待答',
      footer: DockActions(
        children: [
          if (index > 0)
            OutlinedButton.icon(
              icon: const Icon(Icons.chevron_left),
              label: const Text('上一题'),
              onPressed: () => setState(() => _index -= 1),
            ),
          FilledButton.icon(
            icon: Icon(isLast ? Icons.check : Icons.chevron_right),
            label: Text(isLast ? '提交答案' : '下一题'),
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
      title: '需要你的输入',
      subtitle: '回答后继续',
      footerHint: 'Pure 会把这条回答作为当前问题的答案继续执行。',
      footer: DockActions(
        children: [
          FilledButton.icon(
            icon: const Icon(Icons.reply),
            label: const Text('回答'),
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
              labelText: '答案',
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
            '问题 ${currentIndex + 1} / $total',
            style: Theme.of(
              context,
            ).textTheme.labelMedium?.copyWith(color: colors.onSurfaceVariant),
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
            '$answeredCount 已答',
            style: Theme.of(
              context,
            ).textTheme.labelSmall?.copyWith(color: colors.onSurfaceVariant),
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
      message: '问题 ${index + 1}',
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
          question.header.isEmpty ? '问题' : question.header,
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
              labelText: question.isOther ? '其它' : '答案',
              hintText: question.isSecret ? '输入秘密答案' : '输入你的回答...',
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
    return DockOptionRow(
      title: option.label,
      subtitle: option.description,
      selected: selected,
      onPressed: () => onChanged(!selected),
      leading: SizedBox.square(
        dimension: 22,
        child: Checkbox(
          value: selected,
          onChanged: (value) => onChanged(value ?? false),
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
