import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../shared/upward_popup_menu.dart';

class ComposerDock extends ConsumerWidget {
  const ComposerDock({required this.state, super.key});

  final StudioState state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final interaction = state.activeInteraction;
    return SafeArea(
      top: false,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(18, 4, 18, 14),
        child: Align(
          alignment: Alignment.center,
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 860),
            child: interaction == null
                ? _PromptComposer(state: state)
                : _InteractionDock(state: state, interaction: interaction),
          ),
        ),
      ),
    );
  }
}

class _PromptComposer extends ConsumerStatefulWidget {
  const _PromptComposer({required this.state});

  final StudioState state;

  @override
  ConsumerState<_PromptComposer> createState() => _PromptComposerState();
}

class _PromptComposerState extends ConsumerState<_PromptComposer> {
  late final TextEditingController _controller;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.state.composerText);
  }

  @override
  void didUpdateWidget(covariant _PromptComposer oldWidget) {
    super.didUpdateWidget(oldWidget);
    final nextText = widget.state.composerText;
    if (nextText != _controller.text) {
      _controller.value = TextEditingValue(
        text: nextText,
        selection: TextSelection.collapsed(offset: nextText.length),
      );
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final canSubmit =
        widget.state.selectedSessionId != null &&
        widget.state.composerText.trim().isNotEmpty &&
        !widget.state.isBusy;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: colors.surface,
        border: Border.all(color: colors.outlineVariant.withValues(alpha: 0.9)),
        borderRadius: BorderRadius.circular(8),
        boxShadow: [
          BoxShadow(
            color: colors.shadow.withValues(alpha: 0.08),
            blurRadius: 18,
            offset: const Offset(0, 8),
          ),
        ],
      ),
      child: Padding(
        padding: const EdgeInsets.fromLTRB(10, 8, 8, 8),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: _controller,
              minLines: 1,
              maxLines: 6,
              decoration: InputDecoration(
                hintText: 'Ask Pure Studio',
                hintStyle: TextStyle(color: colors.onSurfaceVariant),
                isDense: true,
                filled: false,
                border: InputBorder.none,
                enabledBorder: InputBorder.none,
                focusedBorder: InputBorder.none,
                prefixIcon: Icon(
                  Icons.edit_outlined,
                  color: colors.onSurfaceVariant,
                ),
                contentPadding: const EdgeInsets.symmetric(vertical: 8),
              ),
              onChanged: ref
                  .read(studioControllerProvider.notifier)
                  .updateComposer,
              onSubmitted: (_) {
                if (canSubmit) {
                  ref.read(studioControllerProvider.notifier).submitComposer();
                }
              },
            ),
            Row(
              children: [
                _PermissionSelector(mode: widget.state.permissionMode),
                const Spacer(),
                if (widget.state.isBusy)
                  IconButton.filledTonal(
                    tooltip: 'Stop',
                    icon: const Icon(Icons.stop),
                    onPressed: ref.read(studioControllerProvider.notifier).stop,
                  )
                else
                  IconButton.filled(
                    tooltip: 'Send',
                    icon: const Icon(Icons.arrow_upward),
                    onPressed: canSubmit
                        ? ref
                              .read(studioControllerProvider.notifier)
                              .submitComposer
                        : null,
                  ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

class _PermissionSelector extends ConsumerWidget {
  const _PermissionSelector({required this.mode});

  final PermissionMode mode;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return UpwardPopupMenu<PermissionMode>(
      tooltip: 'Permission mode',
      initialValue: mode,
      onSelected: ref.read(studioControllerProvider.notifier).setPermissionMode,
      itemBuilder: (context) => [
        for (final option in PermissionMode.values)
          PopupMenuItem(
            value: option,
            child: SizedBox(
              width: 136,
              height: 36,
              child: Row(
                children: [
                  Icon(_permissionIcon(option), size: 18),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Text(
                      _permissionLabel(option),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                ],
              ),
            ),
          ),
      ],
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: Theme.of(context).colorScheme.surfaceContainerLow,
          border: Border.all(
            color: Theme.of(context).colorScheme.outlineVariant,
          ),
          borderRadius: BorderRadius.circular(8),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 6),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(_permissionIcon(mode), size: 18),
              const SizedBox(width: 6),
              Text(
                _permissionLabel(mode),
                style: Theme.of(context).textTheme.labelMedium,
              ),
              const SizedBox(width: 4),
              const Icon(Icons.arrow_drop_down, size: 18),
            ],
          ),
        ),
      ),
    );
  }

  IconData _permissionIcon(PermissionMode value) {
    return switch (value) {
      PermissionMode.requestApproval => Icons.verified_user_outlined,
      PermissionMode.autoReview => Icons.rule_folder_outlined,
      PermissionMode.fullAccess => Icons.lock_open_outlined,
    };
  }

  String _permissionLabel(PermissionMode value) {
    return switch (value) {
      PermissionMode.requestApproval => 'Request',
      PermissionMode.autoReview => 'Review',
      PermissionMode.fullAccess => 'Full',
    };
  }
}

class _InteractionDock extends StatelessWidget {
  const _InteractionDock({required this.state, required this.interaction});

  final StudioState state;
  final PendingInteraction interaction;

  @override
  Widget build(BuildContext context) {
    final payload = _InteractionPayloadSnapshot.from(interaction);
    final colors = Theme.of(context).colorScheme;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: colors.surface,
        border: Border.all(color: colors.outlineVariant.withValues(alpha: 0.9)),
        borderRadius: BorderRadius.circular(8),
        boxShadow: [
          BoxShadow(
            color: colors.shadow.withValues(alpha: 0.07),
            blurRadius: 18,
            offset: const Offset(0, 8),
          ),
        ],
      ),
      child: Padding(
        padding: const EdgeInsets.fromLTRB(12, 12, 10, 12),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Icon(_iconFor(interaction.kind), color: colors.onSurfaceVariant),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(
                    interaction.title,
                    style: Theme.of(context).textTheme.titleSmall,
                  ),
                  const SizedBox(height: 4),
                  if (payload.summary.isNotEmpty) Text(payload.summary),
                  const SizedBox(height: 10),
                  _InteractionActions(
                    interaction: interaction,
                    payload: payload,
                  ),
                ],
              ),
            ),
            if (state.isBusy)
              Consumer(
                builder: (context, ref, child) => IconButton.filledTonal(
                  tooltip: 'Stop',
                  icon: const Icon(Icons.stop),
                  onPressed: ref.read(studioControllerProvider.notifier).stop,
                ),
              ),
          ],
        ),
      ),
    );
  }

  IconData _iconFor(InteractionKind kind) {
    return switch (kind) {
      InteractionKind.toolApproval => Icons.admin_panel_settings_outlined,
      InteractionKind.userInput => Icons.help_outline,
      InteractionKind.planConfirmation => Icons.route_outlined,
    };
  }
}

class _InteractionActions extends StatelessWidget {
  const _InteractionActions({required this.interaction, required this.payload});

  final PendingInteraction interaction;
  final _InteractionPayloadSnapshot payload;

  @override
  Widget build(BuildContext context) {
    return switch (interaction.kind) {
      InteractionKind.toolApproval => _ToolApprovalForm(payload: payload),
      InteractionKind.userInput => _UserInputForm(payload: payload),
      InteractionKind.planConfirmation => _PlanConfirmationForm(
        payload: payload,
      ),
    };
  }
}

class _ToolApprovalForm extends ConsumerStatefulWidget {
  const _ToolApprovalForm({required this.payload});

  final _InteractionPayloadSnapshot payload;

  @override
  ConsumerState<_ToolApprovalForm> createState() => _ToolApprovalFormState();
}

class _ToolApprovalFormState extends ConsumerState<_ToolApprovalForm> {
  late final TextEditingController _reasonController;

  @override
  void initState() {
    super.initState();
    _reasonController = TextEditingController();
  }

  @override
  void dispose() {
    _reasonController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final payload = widget.payload;
    final arguments = payload.formattedArguments;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            _InfoChip(icon: Icons.build_outlined, label: payload.toolName),
            if (payload.workingDirectory.isNotEmpty)
              _InfoChip(
                icon: Icons.folder_outlined,
                label: payload.workingDirectory,
              ),
          ],
        ),
        if (arguments.isNotEmpty) ...[
          const SizedBox(height: 10),
          _CodeBlock(text: arguments),
        ],
        const SizedBox(height: 10),
        TextField(
          controller: _reasonController,
          minLines: 1,
          maxLines: 3,
          decoration: const InputDecoration(
            labelText: 'Reason',
            prefixIcon: Icon(Icons.chat_bubble_outline),
          ),
          onChanged: (_) => setState(() {}),
        ),
        const SizedBox(height: 10),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            FilledButton.icon(
              icon: const Icon(Icons.check),
              label: const Text('Approve'),
              onPressed: () => _resolve('approved'),
            ),
            OutlinedButton.icon(
              icon: const Icon(Icons.close),
              label: const Text('Deny'),
              onPressed: _reasonController.text.trim().isEmpty
                  ? null
                  : () => _resolve('denied'),
            ),
          ],
        ),
      ],
    );
  }

  void _resolve(String decision) {
    final reason = _reasonController.text.trim();
    ref.read(studioControllerProvider.notifier).resolveActiveInteraction({
      'type': 'toolApproval',
      'decision': decision,
      if (reason.isNotEmpty) 'reason': reason,
    });
  }
}

class _UserInputForm extends ConsumerStatefulWidget {
  const _UserInputForm({required this.payload});

  final _InteractionPayloadSnapshot payload;

  @override
  ConsumerState<_UserInputForm> createState() => _UserInputFormState();
}

class _UserInputFormState extends ConsumerState<_UserInputForm> {
  late final TextEditingController _fallbackController;
  final Map<String, TextEditingController> _textControllers = {};
  final Map<String, String?> _selectedOptions = {};

  @override
  void initState() {
    super.initState();
    _fallbackController = TextEditingController();
    for (final question in widget.payload.questions) {
      _textControllers[question.id] = TextEditingController();
      if (question.options.isNotEmpty) {
        _selectedOptions[question.id] = question.options.first.label;
      }
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
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (widget.payload.rawBody.isNotEmpty) ...[
            _CodeBlock(text: widget.payload.rawBody),
            const SizedBox(height: 10),
          ],
          TextField(
            controller: _fallbackController,
            minLines: 1,
            maxLines: 4,
            decoration: const InputDecoration(
              labelText: 'Answer',
              prefixIcon: Icon(Icons.short_text_outlined),
            ),
            onChanged: (_) => setState(() {}),
          ),
          const SizedBox(height: 10),
          FilledButton.icon(
            icon: const Icon(Icons.reply),
            label: const Text('Answer'),
            onPressed: _answers().isEmpty ? null : _submitAnswers,
          ),
        ],
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (final question in questions) ...[
          _QuestionField(
            question: question,
            controller: _textControllers[question.id]!,
            selected: _selectedOptions[question.id],
            onOptionSelected: (value) {
              setState(() => _selectedOptions[question.id] = value);
            },
            onTextChanged: (_) => setState(() {}),
          ),
          const SizedBox(height: 10),
        ],
        FilledButton.icon(
          icon: const Icon(Icons.reply),
          label: const Text('Submit'),
          onPressed: _answers().isEmpty ? null : _submitAnswers,
        ),
      ],
    );
  }

  Map<String, Object> _answers() {
    final answers = <String, Object>{};
    if (widget.payload.questions.isEmpty) {
      final text = _fallbackController.text.trim();
      if (text.isNotEmpty) {
        answers['answer'] = {
          'answers': [text],
        };
      }
      return answers;
    }
    for (final question in widget.payload.questions) {
      final values = <String>[];
      final selected = _selectedOptions[question.id];
      final text = _textControllers[question.id]?.text.trim() ?? '';
      if (selected != null && selected.isNotEmpty) {
        values.add(selected);
      }
      if (text.isNotEmpty) {
        values.add(text);
      }
      if (values.isNotEmpty) {
        answers[question.id] = {'answers': values};
      }
    }
    return answers;
  }

  void _submitAnswers() {
    ref.read(studioControllerProvider.notifier).resolveActiveInteraction({
      'type': 'userInput',
      'answers': _answers(),
    });
  }
}

class _PlanConfirmationForm extends ConsumerStatefulWidget {
  const _PlanConfirmationForm({required this.payload});

  final _InteractionPayloadSnapshot payload;

  @override
  ConsumerState<_PlanConfirmationForm> createState() =>
      _PlanConfirmationFormState();
}

class _PlanConfirmationFormState extends ConsumerState<_PlanConfirmationForm> {
  late final TextEditingController _contentController;
  late final TextEditingController _reasonController;

  @override
  void initState() {
    super.initState();
    _contentController = TextEditingController(
      text: widget.payload.planContent,
    );
    _reasonController = TextEditingController();
  }

  @override
  void dispose() {
    _contentController.dispose();
    _reasonController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        TextField(
          controller: _contentController,
          minLines: 4,
          maxLines: 10,
          decoration: const InputDecoration(
            labelText: 'Plan content',
            alignLabelWithHint: true,
            prefixIcon: Icon(Icons.route_outlined),
          ),
          onChanged: (_) => setState(() {}),
        ),
        const SizedBox(height: 10),
        TextField(
          controller: _reasonController,
          minLines: 1,
          maxLines: 3,
          decoration: const InputDecoration(
            labelText: 'Note',
            prefixIcon: Icon(Icons.edit_note),
          ),
          onChanged: (_) => setState(() {}),
        ),
        const SizedBox(height: 10),
        Wrap(
          spacing: 8,
          runSpacing: 8,
          children: [
            FilledButton.icon(
              icon: const Icon(Icons.play_arrow),
              label: const Text('Implement'),
              onPressed: _contentController.text.trim().isEmpty
                  ? null
                  : () => _resolve('implementFreshContext'),
            ),
            OutlinedButton.icon(
              icon: const Icon(Icons.edit_note),
              label: const Text('Continue'),
              onPressed: _contentController.text.trim().isEmpty
                  ? null
                  : () => _resolve('continuePlanning'),
            ),
            TextButton.icon(
              icon: const Icon(Icons.block),
              label: const Text('Dismiss'),
              onPressed: () => _resolve('dismiss'),
            ),
          ],
        ),
      ],
    );
  }

  void _resolve(String decision) {
    final content = _contentController.text.trim();
    final reason = _reasonController.text.trim();
    ref.read(studioControllerProvider.notifier).resolveActiveInteraction({
      'type': 'planConfirmation',
      'decision': decision,
      if (content.isNotEmpty) 'content': content,
      if (reason.isNotEmpty) 'reason': reason,
    });
  }
}

class _QuestionField extends StatelessWidget {
  const _QuestionField({
    required this.question,
    required this.controller,
    required this.selected,
    required this.onOptionSelected,
    required this.onTextChanged,
  });

  final _UserQuestionView question;
  final TextEditingController controller;
  final String? selected;
  final ValueChanged<String?> onOptionSelected;
  final ValueChanged<String> onTextChanged;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(question.header, style: Theme.of(context).textTheme.labelLarge),
        const SizedBox(height: 4),
        Text(question.question),
        if (question.options.isNotEmpty) ...[
          const SizedBox(height: 8),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              for (final option in question.options)
                ChoiceChip(
                  label: Text(option.label),
                  selected: selected == option.label,
                  tooltip: option.description.isEmpty
                      ? null
                      : option.description,
                  onSelected: (isSelected) {
                    onOptionSelected(isSelected ? option.label : null);
                  },
                ),
            ],
          ),
        ],
        if (question.isOther || question.options.isEmpty) ...[
          const SizedBox(height: 8),
          TextField(
            controller: controller,
            obscureText: question.isSecret,
            minLines: 1,
            maxLines: question.isSecret ? 1 : 3,
            decoration: InputDecoration(
              labelText: question.isOther ? 'Other' : 'Answer',
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

class _InfoChip extends StatelessWidget {
  const _InfoChip({required this.icon, required this.label});

  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    return Chip(
      avatar: Icon(icon, size: 18),
      label: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 520),
        child: Text(label, overflow: TextOverflow.ellipsis),
      ),
    );
  }
}

class _CodeBlock extends StatelessWidget {
  const _CodeBlock({required this.text});

  final String text;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Container(
      width: double.infinity,
      constraints: const BoxConstraints(maxHeight: 180),
      decoration: BoxDecoration(
        color: colors.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(8),
      ),
      padding: const EdgeInsets.all(10),
      child: SingleChildScrollView(
        child: SelectableText(
          text,
          style: Theme.of(
            context,
          ).textTheme.bodySmall?.copyWith(fontFamily: 'monospace'),
        ),
      ),
    );
  }
}

class _InteractionPayloadSnapshot {
  const _InteractionPayloadSnapshot({
    required this.rawBody,
    required this.summary,
    required this.toolName,
    required this.formattedArguments,
    required this.workingDirectory,
    required this.questions,
    required this.planContent,
  });

  final String rawBody;
  final String summary;
  final String toolName;
  final String formattedArguments;
  final String workingDirectory;
  final List<_UserQuestionView> questions;
  final String planContent;

  static _InteractionPayloadSnapshot from(PendingInteraction interaction) {
    final decoded = interaction.payload.isEmpty
        ? _decodeObject(interaction.body)
        : interaction.payload;
    final payload = _payloadObject(decoded);
    return switch (interaction.kind) {
      InteractionKind.toolApproval => _tool(interaction, payload),
      InteractionKind.userInput => _userInput(interaction, payload),
      InteractionKind.planConfirmation => _plan(interaction, payload),
    };
  }

  static _InteractionPayloadSnapshot _tool(
    PendingInteraction interaction,
    Map<String, Object?> payload,
  ) {
    final name = _stringValue(payload, ['name', 'toolName', 'title']);
    final arguments =
        payload['arguments'] ?? payload['args'] ?? payload['input'];
    final workingDirectory = _stringValue(payload, [
      'workingDirectory',
      'working_directory',
      'cwd',
    ]);
    return _InteractionPayloadSnapshot(
      rawBody: interaction.body,
      summary: interaction.body,
      toolName: name.isEmpty ? interaction.title : name,
      formattedArguments: _formatJson(arguments),
      workingDirectory: workingDirectory,
      questions: const [],
      planContent: '',
    );
  }

  static _InteractionPayloadSnapshot _userInput(
    PendingInteraction interaction,
    Map<String, Object?> payload,
  ) {
    final questionsValue = payload['questions'];
    final questions = questionsValue is List
        ? questionsValue
              .whereType<Map>()
              .map(
                (value) =>
                    _UserQuestionView.from(value.cast<String, Object?>()),
              )
              .toList()
        : const <_UserQuestionView>[];
    return _InteractionPayloadSnapshot(
      rawBody: interaction.body,
      summary: interaction.body,
      toolName: '',
      formattedArguments: '',
      workingDirectory: '',
      questions: questions,
      planContent: '',
    );
  }

  static _InteractionPayloadSnapshot _plan(
    PendingInteraction interaction,
    Map<String, Object?> payload,
  ) {
    final content = _stringValue(payload, ['content', 'plan', 'body']);
    return _InteractionPayloadSnapshot(
      rawBody: interaction.body,
      summary: interaction.title,
      toolName: '',
      formattedArguments: '',
      workingDirectory: '',
      questions: const [],
      planContent: content.isEmpty ? interaction.body : content,
    );
  }

  static Map<String, Object?> _decodeObject(String body) {
    try {
      final decoded = jsonDecode(body);
      if (decoded is Map) {
        return decoded.cast<String, Object?>();
      }
    } on FormatException {
      return const {};
    }
    return const {};
  }

  static Map<String, Object?> _payloadObject(Map<String, Object?> value) {
    final payload = value['payload'];
    if (payload is Map) {
      return payload.cast<String, Object?>();
    }
    return value;
  }

  static String _stringValue(Map<String, Object?> value, List<String> keys) {
    for (final key in keys) {
      final candidate = value[key];
      if (candidate is String && candidate.trim().isNotEmpty) {
        return candidate.trim();
      }
    }
    return '';
  }

  static String _formatJson(Object? value) {
    if (value == null) {
      return '';
    }
    try {
      return const JsonEncoder.withIndent('  ').convert(value);
    } on JsonUnsupportedObjectError {
      return value.toString();
    }
  }
}

class _UserQuestionView {
  const _UserQuestionView({
    required this.id,
    required this.header,
    required this.question,
    required this.isOther,
    required this.isSecret,
    required this.options,
  });

  final String id;
  final String header;
  final String question;
  final bool isOther;
  final bool isSecret;
  final List<_UserQuestionOptionView> options;

  static _UserQuestionView from(Map<String, Object?> value) {
    final optionsValue = value['options'];
    final options = optionsValue is List
        ? optionsValue
              .whereType<Map>()
              .map(
                (option) => _UserQuestionOptionView.from(
                  option.cast<String, Object?>(),
                ),
              )
              .toList()
        : const <_UserQuestionOptionView>[];
    final id = value['id']?.toString().trim() ?? '';
    return _UserQuestionView(
      id: id.isEmpty ? 'answer' : id,
      header: value['header']?.toString() ?? 'Input',
      question: value['question']?.toString() ?? '',
      isOther: value['isOther'] == true || value['is_other'] == true,
      isSecret: value['isSecret'] == true || value['is_secret'] == true,
      options: options,
    );
  }
}

class _UserQuestionOptionView {
  const _UserQuestionOptionView({
    required this.label,
    required this.description,
  });

  final String label;
  final String description;

  static _UserQuestionOptionView from(Map<String, Object?> value) {
    return _UserQuestionOptionView(
      label: value['label']?.toString() ?? '',
      description: value['description']?.toString() ?? '',
    );
  }
}
