import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'interaction_payload.dart';
import 'interaction_widgets.dart';
import 'fallback_question_dock.dart';
import 'question_progress.dart';
import 'question_step.dart';

class UserInputDock extends ConsumerStatefulWidget {
  const UserInputDock({
    required this.threadId,
    required this.interactionId,
    required this.payload,
    required this.enabled,
    this.trailing,
    super.key,
  });

  final String threadId;
  final String interactionId;
  final InteractionPayloadSnapshot payload;
  final bool enabled;
  final Widget? trailing;

  @override
  ConsumerState<UserInputDock> createState() => _UserInputDockState();
}

class _UserInputDockState extends ConsumerState<UserInputDock> {
  late final TextEditingController _fallbackController;
  final Map<String, TextEditingController> _textControllers = {};
  final Map<String, Set<String>> _selectedOptions = {};
  final _firstOptionKey = GlobalKey();
  final _submitKey = GlobalKey();
  String? _ensuredQuestionKey;
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
      return FallbackQuestionDock(
        body: widget.payload.rawBody,
        controller: _fallbackController,
        trailing: widget.trailing,
        onChanged: () => setState(() {}),
        onSubmit: !widget.enabled || _fallbackController.text.trim().isEmpty
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
    if (index == 0 && question.options.isNotEmpty) {
      _ensureFirstOptionVisible(key);
    }
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
            TextButton.icon(
              icon: const Icon(Icons.chevron_left),
              label: Text(context.l10n.interactionPreviousQuestion),
              onPressed: () => setState(() => _index -= 1),
            ),
          KeyedSubtree(
            key: _submitKey,
            child: FilledButton.icon(
              key: StudioDriverKeys.userInputSubmit,
              icon: Icon(isLast ? Icons.check : Icons.chevron_right),
              label: Text(
                isLast
                    ? context.l10n.interactionSubmitAnswers
                    : context.l10n.interactionNextQuestion,
              ),
              onPressed: widget.enabled
                  ? () {
                      if (isLast) {
                        _submitAnswers();
                      } else {
                        setState(() => _index += 1);
                      }
                    }
                  : null,
            ),
          ),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: [
          QuestionProgress(
            total: total,
            currentIndex: index,
            answeredCount: answeredCount,
            questions: questions,
            answered: _answered,
            onSelected: (value) => setState(() => _index = value),
          ),
          const SizedBox(height: 12),
          QuestionStep(
            enabled: widget.enabled,
            isFirstQuestion: index == 0,
            firstOptionKey: _firstOptionKey,
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
              _ensureSubmitVisible();
            },
            onTextChanged: (_) {
              setState(() {});
              _ensureSubmitVisible();
            },
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
    _ensuredQuestionKey = null;
  }

  void _ensureFirstOptionVisible(String questionKey) {
    if (_ensuredQuestionKey == questionKey) {
      return;
    }
    _ensuredQuestionKey = questionKey;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) {
        return;
      }
      final context = _firstOptionKey.currentContext;
      if (context != null) {
        Scrollable.ensureVisible(
          context,
          alignment: 0.8,
          duration: Duration.zero,
        );
      }
    });
  }

  void _ensureSubmitVisible() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) {
        return;
      }
      final context = _submitKey.currentContext;
      if (context != null) {
        Scrollable.ensureVisible(
          context,
          alignment: 0.8,
          duration: Duration.zero,
        );
      }
    });
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
          widget.interactionId,
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
          widget.interactionId,
          UserInputResolutionCommand(answers: _answers()),
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
