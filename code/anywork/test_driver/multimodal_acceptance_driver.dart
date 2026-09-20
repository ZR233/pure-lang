import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

Future<void> main(List<String> arguments) async {
  final options = _DriverOptions.parse(arguments);
  final snapshots = File(options.snapshotOutput);
  await snapshots.parent.create(recursive: true);

  FlutterDriverSession? session;
  try {
    session = await FlutterDriverSession.connect(
      vmServiceUrl: options.vmServiceUrl,
    );
    await _command(
      session.waitFor(
        find.byValueKey('studio-shell'),
        timeout: const Duration(minutes: 2),
      ),
      'Studio shell',
      const Duration(minutes: 2),
    );
    await _openWorkspace(session, snapshots, options);
    await _assertModelCapabilities(session, snapshots, options);
    if (!options.activeViewImage) {
      if (options.pasteSignalOutput case final signal?) {
        await _admitPastedImage(session, snapshots, options, signal, 1);
        await File(options.previewScreenshotOutput)
            .writeAsBytes(await session.screenshot(), flush: true);
        await _removePastedImage(session, snapshots);
        await File(options.removedScreenshotOutput)
            .writeAsBytes(await session.screenshot(), flush: true);
        await _admitPastedImage(session, snapshots, options, signal, 2);
      } else {
        await _admitImage(session, snapshots, options);
        await File(options.previewScreenshotOutput)
            .writeAsBytes(await session.screenshot(), flush: true);
      }
    }
    await _submitPrompt(session, snapshots, options);
    final completed = await _waitForRealCompletion(session, snapshots, options);
    final viewImage = options.activeViewImage
        ? await _captureViewImageUi(session, completed, options)
        : null;
    if (!options.activeViewImage) {
      await File(options.screenshotOutput)
          .writeAsBytes(await session.screenshot(), flush: true);
    }
    stdout.writeln(
      jsonEncode({
        'result': 'completed',
        'activeViewImage': options.activeViewImage,
        'model': options.expectedModel,
        'marker': options.expectedMarker,
        'answer': _finalAnswerText(completed),
        'modelScreenshot': options.modelScreenshotOutput,
        'previewScreenshot': options.previewScreenshotOutput,
        'removedScreenshot': options.removedScreenshotOutput,
        'screenshot': options.screenshotOutput,
        'threadId': _workspace(completed)?['threadId'],
        'viewImage': viewImage,
      }),
    );
  } catch (error, stackTrace) {
    stderr.writeln('Multimodal Driver failed: $error');
    stderr.writeln(stackTrace);
    if (session != null) {
      try {
        await _snapshot(session, snapshots, kind: 'failure');
      } on Object {
        // Preserve the original acceptance failure.
      }
      try {
        await File('${options.screenshotOutput}.failure.png')
            .writeAsBytes(await session.screenshot(), flush: true);
      } on Object {
        // Preserve the original acceptance failure.
      }
    }
    rethrow;
  } finally {
    if (session != null) {
      try {
        await session.close().timeout(const Duration(seconds: 10));
      } on Object {
        // The caller owns the native GUI process.
      }
    }
  }
}

Future<void> _openWorkspace(
  FlutterDriverSession session,
  File snapshots,
  _DriverOptions options,
) async {
  var snapshot = await _snapshot(session, snapshots, kind: 'bootstrap');
  if (_selectedProjectPath(snapshot) != options.workspace) {
    await _command(
      session.tap(find.byValueKey('sidebar-open-project')),
      'open project',
    );
    await _command(
      session.tap(find.byValueKey('add-project-local')),
      'select local project',
    );
    await _command(
      session.waitForNoPendingFrame(timeout: const Duration(seconds: 10)),
      'settle local project selection',
    );
    await _command(
      session.tap(find.byValueKey('add-project-continue')),
      'continue local project',
    );
    await _command(
      session.waitFor(
        find.byValueKey('project-path-dialog'),
        timeout: const Duration(seconds: 30),
      ),
      'project path dialog',
    );
    await _command(
      session.tap(find.byValueKey('project-path-input')),
      'focus project path',
    );
    await _command(session.enterText(options.workspace), 'enter project path');
    await _command(
      session.sendTextInputAction(
        TextInputAction.done,
        timeout: const Duration(seconds: 10),
      ),
      'submit project path',
    );
    snapshot = await _waitForSnapshot(
      session,
      snapshots,
      'selected project',
      (value) => _selectedProjectPath(value) == options.workspace,
      timeout: const Duration(minutes: 2),
    );
  }
  if (snapshot['navigation'] case {'isStartPage': false}) {
    await _command(
      session.tap(find.byValueKey('sidebar-new-session')),
      'open new Thread Composer',
    );
  }
  await _waitForSnapshot(
    session,
    snapshots,
    'new Thread Composer',
    (value) =>
        _selectedProjectPath(value) == options.workspace &&
        (value['navigation'] as Map<String, dynamic>?)?['isStartPage'] == true,
    timeout: options.pasteSignalOutput == null
        ? const Duration(minutes: 1)
        : const Duration(seconds: 20),
  );
  await _command(
    session.waitFor(
      find.byValueKey('composer-input'),
      timeout: const Duration(minutes: 1),
    ),
    'Composer input',
    const Duration(minutes: 1),
  );
}

Future<void> _assertModelCapabilities(
  FlutterDriverSession session,
  File snapshots,
  _DriverOptions options,
) async {
  await _command(
    session.tap(find.byValueKey('model-selector')),
    'open model selector',
  );
  final option = find.byValueKey(
    'model-${options.providerId}-${options.expectedModel}',
  );
  await _command(
    session.waitFor(option, timeout: const Duration(seconds: 30)),
    'expected model option',
  );
  final capabilityFinder = find.byValueKey(
    'model-${options.providerId}-${options.expectedModel}-capabilities',
  );
  final capabilities = await _command(
    session.getText(capabilityFinder),
    'read model capabilities',
  );
  if (capabilities != '文本 · 视觉' && capabilities != 'Text · Vision') {
    throw StateError(
      'expected a text and vision capability label for '
      '${options.expectedModel}, got $capabilities',
    );
  }
  await File(options.modelScreenshotOutput)
      .writeAsBytes(await session.screenshot(), flush: true);
  await _command(session.tap(option), 'select expected model');
  await _snapshot(session, snapshots, kind: 'modelSelected');
}

Future<void> _admitImage(
  FlutterDriverSession session,
  File snapshots,
  _DriverOptions options,
) async {
  await _command(
    session.tap(find.byValueKey('composer-attachment-entry')),
    'open attachment menu',
  );
  final local = find.byValueKey('composer-attachment-local');
  await _command(
    session.waitFor(local, timeout: const Duration(seconds: 30)),
    'local attachment option',
  );
  await _command(session.tap(local), 'select local image');
  await _waitForAdmittedImage(session, snapshots, options);
}

Future<void> _admitPastedImage(
  FlutterDriverSession session,
  File snapshots,
  _DriverOptions options,
  String signalOutput,
  int sequence,
) async {
  await _command(
    session.tap(find.byValueKey('composer-input')),
    'focus Composer for native paste',
  );
  final signal = File(signalOutput);
  await signal.parent.create(recursive: true);
  await signal.writeAsString(
    jsonEncode({
      'sequence': sequence,
      'readyAt': DateTime.now().toUtc().toIso8601String(),
    }),
    flush: true,
  );
  await _waitForAdmittedImage(session, snapshots, options);
}

Future<Map<String, dynamic>> _waitForAdmittedImage(
  FlutterDriverSession session,
  File snapshots,
  _DriverOptions options,
) async {
  final admitted = await _waitForSnapshot(
    session,
    snapshots,
    'local image admission',
    (snapshot) {
      final attachments = _newComposer(snapshot)?['attachments'];
      return attachments is List<dynamic> &&
          attachments.length == 1 &&
          attachments.single is Map<String, dynamic> &&
          (attachments.single as Map<String, dynamic>)['modality'] == 'image';
    },
    timeout: const Duration(minutes: 1),
  );
  final attachment =
      (_newComposer(admitted)!['attachments'] as List<dynamic>).single
          as Map<String, dynamic>;
  if (attachment['filename'] != options.expectedFilename) {
    throw StateError(
      'admitted unexpected file ${attachment['filename']}; expected ${options.expectedFilename}',
    );
  }
  await _command(
    session.waitFor(
      find.byValueKey('attachment-draft-rail'),
      timeout: const Duration(seconds: 30),
    ),
    'attachment preview rail',
  );
  return attachment;
}

Future<void> _removePastedImage(
  FlutterDriverSession session,
  File snapshots,
) async {
  final before = await _snapshot(session, snapshots, kind: 'beforeRemove');
  final attachments = _newComposer(before)?['attachments'];
  if (attachments is! List<dynamic> || attachments.length != 1) {
    throw StateError('expected one pasted attachment before removal');
  }
  final attachment = attachments.single;
  if (attachment is! Map<String, dynamic> || attachment['id'] is! String) {
    throw StateError('pasted attachment is missing its draft id');
  }
  final draftId = attachment['id'] as String;
  await _command(
    session.tap(find.byValueKey('attachment-remove-$draftId')),
    'remove pasted attachment',
  );
  await _waitForSnapshot(session, snapshots, 'pasted image removal', (
    snapshot,
  ) {
    final remaining = _newComposer(snapshot)?['attachments'];
    return remaining is List<dynamic> && remaining.isEmpty;
  }, timeout: const Duration(minutes: 1));
  await _command(
    session.waitForAbsent(
      find.byValueKey('attachment-draft-rail'),
      timeout: const Duration(seconds: 30),
    ),
    'removed attachment preview rail',
  );
}

Future<void> _submitPrompt(
  FlutterDriverSession session,
  File snapshots,
  _DriverOptions options,
) async {
  await _snapshot(session, snapshots, kind: 'beforeSubmit');
  final input = find.byValueKey('composer-input');
  await _command(session.tap(input), 'focus Composer');
  await _command(session.enterText(options.prompt), 'enter image prompt');
  final entered = await _command(session.getText(input), 'read image prompt');
  if (entered != options.prompt) {
    throw StateError('Composer prompt read-back mismatch');
  }
  await _command(
    session.waitForNoPendingFrame(timeout: const Duration(seconds: 15)),
    'Composer rebuild',
  );
  await _command(
    session.tap(find.byValueKey('composer-submit')),
    'submit image prompt',
  );
  await _waitForSnapshot(session, snapshots, 'accepted image prompt', (
    snapshot,
  ) {
    final workspace = _workspace(snapshot);
    if (options.activeViewImage) {
      final timeline = workspace?['timeline'];
      return workspace?['model'] == options.expectedModel &&
          workspace?['threadId'] is String &&
          (workspace?['isBusy'] == true ||
              timeline is List<dynamic> && timeline.isNotEmpty);
    }
    final attachments = workspace?['historyAttachments'];
    return workspace?['model'] == options.expectedModel &&
        attachments is List<dynamic> &&
        attachments.any(
          (attachment) =>
              attachment is Map<String, dynamic> &&
              attachment['modality'] == 'image' &&
              attachment['filename'] == options.expectedFilename,
        );
  }, timeout: const Duration(minutes: 1));
}

bool _completedWithMarker(
  Map<String, dynamic> snapshot,
  _DriverOptions options,
) {
  final workspace = _workspace(snapshot);
  if (workspace == null ||
      workspace['model'] != options.expectedModel ||
      workspace['isBusy'] != false) {
    return false;
  }
  final capabilities = workspace['modelCapabilities'];
  if (capabilities is! List<dynamic> ||
      !capabilities.contains('text') ||
      !capabilities.contains('image')) {
    return false;
  }
  final timeline = workspace['timeline'];
  final hasAnswer =
      timeline is List<dynamic> &&
      timeline.any(
        (row) =>
            row is Map<String, dynamic> &&
            row['type'] == 'finalAnswer' &&
            _answerMatches(row['text'] as String? ?? '', options),
      );
  return hasAnswer &&
      (!options.activeViewImage ||
          _viewImageReceipt(snapshot, options) != null);
}

bool _answerMatches(String answer, _DriverOptions options) {
  if (!answer.contains(options.expectedMarker)) return false;
  final pattern = options.expectedAnswerPattern;
  return pattern == null ||
      RegExp(pattern, caseSensitive: false, dotAll: true).hasMatch(answer);
}

String? _finalAnswerText(Map<String, dynamic> snapshot) {
  final timeline = _workspace(snapshot)?['timeline'];
  if (timeline is! List<dynamic>) return null;
  for (final row in timeline.reversed) {
    if (row is Map<String, dynamic> && row['type'] == 'finalAnswer') {
      return row['text'] as String?;
    }
  }
  return null;
}

Map<String, String>? _viewImageReceipt(
  Map<String, dynamic> snapshot,
  _DriverOptions options,
) {
  final timeline = _workspace(snapshot)?['timeline'];
  if (timeline is! List<dynamic>) return null;
  for (final row in timeline) {
    if (row is! Map<String, dynamic>) continue;
    final groupId = row['id'];
    final tools = row['tools'];
    if (tools is! List<dynamic>) continue;
    for (final tool in tools) {
      if (tool is! Map<String, dynamic> || tool['name'] != 'view_image') {
        continue;
      }
      final callId = tool['callId'];
      final attachments = tool['attachments'];
      if (groupId is! String ||
          callId is! String ||
          attachments is! List<dynamic>) {
        continue;
      }
      for (final attachment in attachments) {
        if (attachment is Map<String, dynamic> &&
            attachment['modality'] == 'image' &&
            attachment['filename'] == options.expectedFilename &&
            attachment['id'] is String) {
          return {
            'groupId': groupId,
            'callId': callId,
            'attachmentId': attachment['id'] as String,
          };
        }
      }
    }
  }
  return null;
}

Future<Map<String, String>> _captureViewImageUi(
  FlutterDriverSession session,
  Map<String, dynamic> snapshot,
  _DriverOptions options,
) async {
  final receipt = _viewImageReceipt(snapshot, options);
  if (receipt == null) {
    throw StateError('completed turn has no typed view_image attachment');
  }
  final groupId = receipt['groupId']!;
  final callId = receipt['callId']!;
  final attachmentId = receipt['attachmentId']!;
  // 条目身份 = owning 调用 callId + 附件 id；资源内容寻址，同一图片可能被多个调用
  // 读取，必须叠加调用身份才能唯一定位本调用的文字入口/图片/弹窗。
  final entryId = '$callId:$attachmentId';
  final summary = find.byValueKey('timeline-tool-group-summary-$groupId');
  await _command(
    session.waitFor(summary, timeout: const Duration(seconds: 30)),
    'view_image tool group summary',
  );
  final toggle = find.byValueKey('view-image-toggle-$entryId');
  await _command(
    session.waitFor(toggle, timeout: const Duration(seconds: 30)),
    'collapsed view_image read label',
  );
  await File(options.previewScreenshotOutput)
      .writeAsBytes(await session.screenshot(), flush: true);
  await _command(session.tap(summary), 'expand view_image tool group');
  await _command(
    session.waitFor(
      find.byValueKey('view-image-tool-$callId'),
      timeout: const Duration(seconds: 30),
    ),
    'expanded view_image tool row',
  );
  await _command(session.tap(toggle), 'expand view_image inline preview');
  final thumbnail = find.byValueKey('view-image-thumbnail-$entryId');
  await _command(
    session.waitFor(thumbnail, timeout: const Duration(seconds: 30)),
    'inline view_image thumbnail',
  );
  await _command(session.tap(thumbnail), 'open view_image preview');
  await _command(
    session.waitFor(
      find.byValueKey('view-image-dialog-$entryId'),
      timeout: const Duration(seconds: 30),
    ),
    'view_image preview dialog',
  );
  await File(options.screenshotOutput)
      .writeAsBytes(await session.screenshot(), flush: true);
  return receipt;
}

Future<Map<String, dynamic>> _waitForRealCompletion(
  FlutterDriverSession session,
  File output,
  _DriverOptions options,
) async {
  final deadline = DateTime.now().add(options.turnTimeout);
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await _snapshot(session, output, kind: 'observation');
    if (_completedWithMarker(snapshot, options)) return snapshot;
    final workspace = _workspace(snapshot);
    final lastTurn = workspace?['lastTurn'];
    if (workspace?['isBusy'] == false &&
        lastTurn is Map<String, dynamic> &&
        const {
          'failed',
          'cancelled',
          'interrupted',
          'budgetLimited',
        }.contains(lastTurn['status'])) {
      throw StateError(
        'real multimodal turn ended as ${lastTurn['status']}: ${lastTurn['reason']}',
      );
    }
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw TimeoutException(
    'Flutter Driver timed out waiting for real multimodal response',
  );
}

Map<String, dynamic>? _workspace(Map<String, dynamic> snapshot) =>
    snapshot['workspace'] as Map<String, dynamic>?;

Map<String, dynamic>? _newComposer(Map<String, dynamic> snapshot) {
  final navigation = snapshot['navigation'];
  return navigation is Map<String, dynamic>
      ? navigation['newThreadComposer'] as Map<String, dynamic>?
      : null;
}

String? _selectedProjectPath(Map<String, dynamic> snapshot) {
  final project = snapshot['project'];
  return project is Map<String, dynamic> ? project['path'] as String? : null;
}

Future<Map<String, dynamic>> _waitForSnapshot(
  FlutterDriverSession session,
  File output,
  String description,
  bool Function(Map<String, dynamic> snapshot) predicate, {
  required Duration timeout,
}) async {
  final deadline = DateTime.now().add(timeout);
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await _snapshot(session, output, kind: 'observation');
    if (predicate(snapshot)) return snapshot;
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw TimeoutException('Flutter Driver snapshot timed out: $description');
}

Future<Map<String, dynamic>> _snapshot(
  FlutterDriverSession session,
  File output, {
  required String kind,
}) async {
  final snapshot = await session.readSnapshot();
  await output.writeAsString(
    '${jsonEncode({'kind': kind, 'capturedAt': DateTime.now().toUtc().toIso8601String(), ...snapshot})}\n',
    mode: FileMode.append,
    flush: true,
  );
  return snapshot;
}

Future<T> _command<T>(
  Future<T> command,
  String description, [
  Duration timeout = const Duration(seconds: 30),
]) => command.timeout(
  timeout,
  onTimeout: () =>
      throw TimeoutException('Flutter Driver command timed out: $description'),
);

class _DriverOptions {
  const _DriverOptions({
    required this.vmServiceUrl,
    required this.workspace,
    required this.snapshotOutput,
    required this.modelScreenshotOutput,
    required this.previewScreenshotOutput,
    required this.removedScreenshotOutput,
    required this.screenshotOutput,
    required this.providerId,
    required this.expectedModel,
    required this.expectedMarker,
    required this.expectedAnswerPattern,
    required this.expectedFilename,
    required this.prompt,
    required this.turnTimeout,
    required this.activeViewImage,
    required this.pasteSignalOutput,
  });

  final String vmServiceUrl;
  final String workspace;
  final String snapshotOutput;
  final String modelScreenshotOutput;
  final String previewScreenshotOutput;
  final String removedScreenshotOutput;
  final String screenshotOutput;
  final String providerId;
  final String expectedModel;
  final String expectedMarker;
  final String? expectedAnswerPattern;
  final String expectedFilename;
  final String prompt;
  final Duration turnTimeout;
  final bool activeViewImage;
  final String? pasteSignalOutput;

  static _DriverOptions parse(List<String> arguments) {
    final values = <String, String>{};
    for (var index = 0; index < arguments.length; index += 2) {
      if (index + 1 >= arguments.length || !arguments[index].startsWith('--')) {
        throw ArgumentError('expected --name value arguments');
      }
      values[arguments[index].substring(2)] = arguments[index + 1];
    }
    String required(String name) {
      final value = values[name];
      if (value == null || value.isEmpty) {
        throw ArgumentError('missing --$name');
      }
      return value;
    }

    final previewScreenshotOutput = required('preview-screenshot-output');
    return _DriverOptions(
      vmServiceUrl: required('vm-service-url'),
      workspace: required('workspace'),
      snapshotOutput: required('snapshot-output'),
      modelScreenshotOutput: required('model-screenshot-output'),
      previewScreenshotOutput: previewScreenshotOutput,
      removedScreenshotOutput:
          values['removed-screenshot-output'] ??
          '$previewScreenshotOutput.removed.png',
      screenshotOutput: required('screenshot-output'),
      providerId: values['provider-id'] ?? 'zhipu',
      expectedModel: values['expected-model'] ?? 'glm-5.3-flash',
      expectedMarker: values['expected-marker'] ?? 'PURE-7429',
      expectedAnswerPattern: values['expected-answer-pattern'],
      expectedFilename: required('expected-filename'),
      prompt: values['prompt'] ?? '只输出图片中央的字符，不要解释',
      turnTimeout: Duration(
        seconds: int.tryParse(values['turn-timeout-seconds'] ?? '') ?? 300,
      ),
      activeViewImage: values['active-view-image'] == 'true',
      pasteSignalOutput: values['paste-signal-output'],
    );
  }
}
