import 'dart:convert';

import '../../domain/models/studio_models.dart';

class InteractionPayloadSnapshot {
  const InteractionPayloadSnapshot({
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
  final List<UserQuestionView> questions;
  final String planContent;

  static InteractionPayloadSnapshot from(PendingInteraction interaction) {
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

  static InteractionPayloadSnapshot _tool(
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
    return InteractionPayloadSnapshot(
      rawBody: interaction.body,
      summary: interaction.body,
      toolName: name.isEmpty ? interaction.title : name,
      formattedArguments: _formatJson(arguments),
      workingDirectory: workingDirectory,
      questions: const [],
      planContent: '',
    );
  }

  static InteractionPayloadSnapshot _userInput(
    PendingInteraction interaction,
    Map<String, Object?> payload,
  ) {
    final questions = _list(
      payload['questions'],
    ).map(_map).map(UserQuestionView.from).toList();
    return InteractionPayloadSnapshot(
      rawBody: interaction.body,
      summary: interaction.body,
      toolName: '',
      formattedArguments: '',
      workingDirectory: '',
      questions: questions,
      planContent: '',
    );
  }

  static InteractionPayloadSnapshot _plan(
    PendingInteraction interaction,
    Map<String, Object?> payload,
  ) {
    final content = _stringValue(payload, ['content', 'plan', 'body']);
    return InteractionPayloadSnapshot(
      rawBody: interaction.body,
      summary: interaction.title,
      toolName: '',
      formattedArguments: '',
      workingDirectory: '',
      questions: const [],
      planContent: content.isEmpty ? interaction.body : content,
    );
  }
}

class UserQuestionView {
  const UserQuestionView({
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
  final List<UserQuestionOptionView> options;

  static UserQuestionView from(Map<String, Object?> value) {
    final options = _list(value['options'])
        .map(_map)
        .map(UserQuestionOptionView.from)
        .where((option) => option.label.isNotEmpty)
        .toList();
    final id = value['id']?.toString().trim() ?? '';
    return UserQuestionView(
      id: id,
      header: value['header']?.toString() ?? 'Input',
      question: value['question']?.toString() ?? '',
      isOther: value['isOther'] == true || value['is_other'] == true,
      isSecret: value['isSecret'] == true || value['is_secret'] == true,
      options: options,
    );
  }
}

class UserQuestionOptionView {
  const UserQuestionOptionView({
    required this.label,
    required this.description,
  });

  final String label;
  final String description;

  static UserQuestionOptionView from(Map<String, Object?> value) {
    return UserQuestionOptionView(
      label: value['label']?.toString() ?? '',
      description: value['description']?.toString() ?? '',
    );
  }
}

Map<String, Object?> _decodeObject(String body) {
  try {
    final decoded = jsonDecode(body);
    if (decoded is Map) {
      return decoded.map((key, value) => MapEntry(key.toString(), value));
    }
  } on FormatException {
    return const {};
  }
  return const {};
}

Map<String, Object?> _payloadObject(Map<String, Object?> value) {
  final payload = value['payload'];
  if (payload is Map) {
    return payload.map((key, value) => MapEntry(key.toString(), value));
  }
  return value;
}

Map<String, Object?> _map(Object? value) {
  if (value is Map<String, Object?>) {
    return value;
  }
  if (value is Map) {
    return value.map((key, value) => MapEntry(key.toString(), value));
  }
  return const {};
}

List<Object?> _list(Object? value) {
  if (value is List) {
    return value.cast<Object?>();
  }
  return const [];
}

String _stringValue(Map<String, Object?> value, List<String> keys) {
  for (final key in keys) {
    final candidate = value[key];
    if (candidate is String && candidate.trim().isNotEmpty) {
      return candidate.trim();
    }
  }
  return '';
}

String _formatJson(Object? value) {
  if (value == null) {
    return '';
  }
  try {
    return const JsonEncoder.withIndent('  ').convert(value);
  } on JsonUnsupportedObjectError {
    return value.toString();
  }
}
