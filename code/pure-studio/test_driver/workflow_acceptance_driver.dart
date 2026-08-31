//! Driver for the real, unified workflow acceptance flow.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

Future<void> main(List<String> arguments) async {
  final options = _Options.parse(arguments);
  final snapshots = File(options.snapshotOutput);
  await snapshots.parent.create(recursive: true);
  FlutterDriverSession? session;
  try {
    session = await FlutterDriverSession.connect(
      vmServiceUrl: options.vmServiceUrl,
      onReconnect: (event) => _append(snapshots, event.toJson()),
    );
    if (options.mode == 'new') {
      await _startNewWorkflow(session, options, snapshots);
    }
    final finalSnapshot = options.studioMode == 'mode.simple'
        ? await _waitForSimpleCompletion(session, options, snapshots)
        : await _waitForTerminal(session, options, snapshots, 'completed');
    final workflow = _workflow(finalSnapshot);
    if (options.studioMode == 'mode.simple') {
      if (workflow != null) {
        throw StateError(
          'mode.simple unexpectedly exposed a workflow: $workflow',
        );
      }
    } else {
      final run = workflow?['currentRun'];
      if (run is! Map<String, dynamic> ||
          run['currentStageId'] != 'completed' ||
          run['terminal'] != true) {
        throw StateError(
          'workflow did not reach completed terminal: $workflow',
        );
      }
      final history = (run['history'] as List<dynamic>? ?? const [])
          .whereType<Map<String, dynamic>>()
          .toList();
      final visited = <String>{
        if (run['currentStageId'] is String) run['currentStageId'] as String,
        if (run['stages'] is List<dynamic> &&
            (run['stages'] as List<dynamic>).isNotEmpty)
          ((run['stages'] as List<dynamic>).first as Map)['id'] as String,
        for (final entry in history)
          if (entry['fromStageId'] is String) entry['fromStageId'] as String,
        for (final entry in history)
          if (entry['toStageId'] is String) entry['toStageId'] as String,
      };
      for (final stage in [
        'planning',
        'awaiting_confirmation',
        'editing_documents',
        'working',
        'integrating',
        'reviewing',
        'completed',
      ]) {
        if (!visited.contains(stage)) {
          throw StateError('workflow history is missing $stage: $visited');
        }
      }
    }
    final timeline =
        ((finalSnapshot['workspace'] as Map?)?['timeline'] as List? ?? const [])
            .whereType<Map>()
            .map((row) => row['text'])
            .whereType<String>()
            .join('\n');
    if (!timeline.contains('PURE_WORKFLOW_GUI_VERIFY_OK') ||
        !timeline.contains('cargo test')) {
      throw StateError(
        'final response does not report the verifier marker and cargo test',
      );
    }
    await File('${options.snapshotOutput}.png')
        .writeAsBytes(await session.screenshot(), flush: true);
    await File('${options.snapshotOutput}.render-tree.txt')
        .writeAsString(await session.renderTree(), flush: true);
    final renderTree = await session.renderTree();
    for (final legacyKey in [
      'task-recovery',
      'work-unit',
      'delivery-review',
      'merge-record',
    ]) {
      if (renderTree.contains(legacyKey)) {
        throw StateError('legacy Task UI is still rendered: $legacyKey');
      }
    }
    stdout.writeln(
      jsonEncode({
        'result': 'completed',
        'mode': options.mode,
        'attempt': options.attempt,
        'workflow': workflow,
        'workspace': finalSnapshot['workspace'],
      }),
    );
    if (options.shutdownAfterCompletion) {
      final response = await session.requestData(
        'shutdown-await',
        timeout: const Duration(minutes: 2),
      );
      final decoded = jsonDecode(response);
      if (decoded is! Map<String, dynamic> ||
          decoded['shutdown'] != 'completed') {
        throw StateError('unexpected Studio shutdown response: $response');
      }
      stdout.writeln(
        jsonEncode({
          'event': 'studioShutdownCompleted',
          'attempt': options.attempt,
        }),
      );
    }
  } catch (error, stackTrace) {
    stderr.writeln('Workflow Driver failed: $error');
    stderr.writeln(stackTrace);
    if (session != null) {
      try {
        await _appendSnapshot(session, snapshots, 'failure');
        await File('${options.snapshotOutput}.failure.png')
            .writeAsBytes(await session.screenshot(), flush: true);
      } on Object {
        // Preserve the original driver failure.
      }
    }
    rethrow;
  } finally {
    await session?.close();
  }
}

Future<void> _startNewWorkflow(
  FlutterDriverSession session,
  _Options options,
  File snapshots,
) async {
  final protectedFiles = await _protectedFileSnapshot(options.workspace!);
  await session.waitFor(
    find.byValueKey('studio-shell'),
    timeout: const Duration(minutes: 2),
  );
  await session.tap(find.byValueKey('sidebar-open-project'));
  await session.waitFor(find.byValueKey('project-path-dialog'));
  await session.tap(find.byValueKey('project-path-input'));
  await session.enterText(options.workspace!);
  await session.sendTextInputAction(
    TextInputAction.done,
    timeout: const Duration(seconds: 10),
  );
  await session.waitForAbsent(
    find.byValueKey('project-path-dialog'),
    timeout: const Duration(seconds: 30),
  );
  await session.waitFor(find.byValueKey('sidebar-new-session'));
  await session.tap(find.byValueKey('sidebar-new-session'));
  await session.tap(find.byValueKey('session-mode-selector'));
  await session.waitFor(find.byValueKey('session-mode-${options.studioMode}'));
  await session.tap(find.byValueKey('session-mode-${options.studioMode}'));
  await session.waitForNoPendingFrame(timeout: const Duration(seconds: 10));
  await _waitForSnapshot(
    session,
    snapshots,
    'mode-selected',
    (snapshot) =>
        (snapshot['workspace'] as Map?)?['threadMode'] == options.studioMode ||
        (snapshot['navigation'] as Map?)?['newThreadMode'] ==
            options.studioMode,
    timeout: const Duration(seconds: 30),
  );
  await session.tap(find.byValueKey('composer-input'));
  await session.enterText(await File(options.promptFile!).readAsString());
  await session.waitForNoPendingFrame(timeout: const Duration(seconds: 20));
  await session.tap(find.byValueKey('composer-submit'));
  if (options.studioMode == 'mode.task') {
    await _waitForSnapshot(
      session,
      snapshots,
      'submitted',
      (snapshot) => _workflow(snapshot) != null,
      timeout: const Duration(minutes: 10),
    );
    await _resolveVisibleInteractionUntilStage(
      session,
      snapshots,
      options,
      'awaiting_confirmation',
      protectedFiles,
    );
  } else {
    await _waitForSnapshot(
      session,
      snapshots,
      'submitted',
      (snapshot) =>
          _workflow(snapshot) == null &&
          (snapshot['workspace'] as Map?)?['turn'] != null,
      timeout: const Duration(minutes: 2),
    );
  }
}

Future<Map<String, dynamic>> _waitForSimpleCompletion(
  FlutterDriverSession session,
  _Options options,
  File snapshots,
) async {
  final deadline = DateTime.now().add(options.workflowTimeout);
  final progress = _ProgressWatch();
  Map<String, dynamic>? last;
  while (DateTime.now().isBefore(deadline)) {
    last = await _appendSnapshot(session, snapshots, 'simple-completed');
    progress.observe(last, options.stallTimeout, 'simple completion');
    final workspace = last['workspace'] as Map?;
    final lastTurn = workspace?['lastTurn'] as Map?;
    if (_workflow(last) == null &&
        lastTurn?['status'] == 'completed' &&
        _hasSuccessfulComplete(last)) {
      return last;
    }
    final interaction = workspace?['activeInteraction'];
    if (interaction is Map && interaction['kind'] == 'toolApproval') {
      await session.tap(find.byValueKey('tool-approve'));
    } else if (interaction is Map && interaction['kind'] == 'userInput') {
      throw StateError('mode.simple requested unexpected user input');
    }
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw StateError('simple completion timed out; last=$last');
}

Future<void> _resolveVisibleInteractionUntilStage(
  FlutterDriverSession session,
  File snapshots,
  _Options options,
  String stage,
  Map<String, String?> protectedFiles,
) async {
  final deadline = DateTime.now().add(options.workflowTimeout);
  final progress = _ProgressWatch();
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await _appendSnapshot(session, snapshots, 'interaction');
    progress.observe(snapshot, options.stallTimeout, 'plan confirmation');
    final workflow = _workflow(snapshot);
    final current = (workflow?['currentRun'] as Map?)?['currentStageId'];
    final interaction = snapshot['workspace'] is Map
        ? (snapshot['workspace'] as Map)['activeInteraction']
        : null;
    if (current == 'planning' &&
        interaction is Map &&
        interaction['kind'] == 'userInput') {
      _assertPlanVisible(snapshot);
      await _tapFirstUserInputOption(session);
      await _waitForSnapshot(session, snapshots, 'confirmed', (next) {
        final nextStage =
            (_workflow(next)?['currentRun'] as Map?)?['currentStageId'];
        return nextStage != 'planning';
      }, timeout: const Duration(minutes: 2));
      return;
    }
    if (current == stage) {
      if (interaction is! Map || interaction['kind'] != 'userInput') {
        await Future<void>.delayed(const Duration(milliseconds: 250));
        continue;
      }
      await _assertProtectedFilesUnchanged(options.workspace!, protectedFiles);
      _assertPlanVisible(snapshot);
      await _tapFirstUserInputOption(session);
      await _waitForSnapshot(session, snapshots, 'confirmed', (next) {
        final nextStage =
            (_workflow(next)?['currentRun'] as Map?)?['currentStageId'];
        return nextStage != 'awaiting_confirmation';
      }, timeout: const Duration(minutes: 2));
      return;
    }
    if (current == 'editing_documents' || current == 'working') {
      throw StateError('workflow skipped visible plan confirmation');
    }
    if (interaction is Map && interaction['kind'] == 'userInput') {
      throw StateError(
        'workflow requested unnecessary clarification before its plan',
      );
    } else if (interaction is Map && interaction['kind'] == 'toolApproval') {
      await session.tap(find.byValueKey('tool-approve'));
    }
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw StateError('workflow never reached $stage');
}

Future<Map<String, dynamic>> _waitForTerminal(
  FlutterDriverSession session,
  _Options options,
  File snapshots,
  String label,
) async {
  final deadline = DateTime.now().add(options.workflowTimeout);
  final progress = _ProgressWatch();
  Map<String, dynamic>? last;
  while (DateTime.now().isBefore(deadline)) {
    last = await _appendSnapshot(session, snapshots, label);
    progress.observe(last, options.stallTimeout, label);
    final workflow = _workflow(last);
    final run = workflow?['currentRun'];
    if (run is Map<String, dynamic> &&
        run['terminal'] == true &&
        run['currentStageId'] == 'completed' &&
        _hasSuccessfulComplete(last)) {
      return last;
    }
    final interaction = (last['workspace'] as Map?)?['activeInteraction'];
    if (interaction is Map && interaction['kind'] == 'toolApproval') {
      await session.tap(find.byValueKey('tool-approve'));
    } else if (interaction is Map && interaction['kind'] == 'userInput') {
      throw StateError(
        'workflow requested unexpected input after plan confirmation',
      );
    }
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw StateError('$label timed out; last=$last');
}

bool _hasSuccessfulComplete(Map<String, dynamic> snapshot) {
  final timeline =
      ((snapshot['workspace'] as Map?)?['timeline'] as List? ?? const [])
          .whereType<Map>();
  for (final row in timeline) {
    final tools = row['tools'];
    if (tools is! List) continue;
    for (final tool in tools.whereType<Map>()) {
      if (tool['name'] == 'complete' && tool['status'] == 'succeeded') {
        return true;
      }
    }
  }
  return false;
}

Future<Map<String, String?>> _protectedFileSnapshot(String workspace) async {
  final result = <String, String?>{};
  for (final relative in [
    'design/task-workflows.md',
    'src/normalize.rs',
    'src/validate.rs',
    'tests/normalize.rs',
    'tests/validate.rs',
  ]) {
    final file = File('$workspace/$relative');
    result[relative] = await file.exists()
        ? base64Encode(await file.readAsBytes())
        : null;
  }
  return result;
}

Future<void> _assertProtectedFilesUnchanged(
  String workspace,
  Map<String, String?> expected,
) async {
  final actual = await _protectedFileSnapshot(workspace);
  if (!_mapEquals(expected, actual)) {
    throw StateError('implementation files changed before plan confirmation');
  }
}

bool _mapEquals(Map<String, String?> left, Map<String, String?> right) {
  if (left.length != right.length) return false;
  return left.entries.every((entry) => right[entry.key] == entry.value);
}

void _assertPlanVisible(Map<String, dynamic> snapshot) {
  final timeline =
      ((snapshot['workspace'] as Map?)?['timeline'] as List? ?? const [])
          .whereType<Map>()
          .map((row) => row['text'])
          .whereType<String>()
          .where((text) => text.trim().isNotEmpty)
          .toList();
  if (timeline.isEmpty) {
    throw StateError('awaiting_confirmation has no visible plan text');
  }
}

Future<void> _tapFirstUserInputOption(FlutterDriverSession session) async {
  if (await _isVisible(session, 'user-input-first-option')) {
    await session.tap(find.byValueKey('user-input-first-option'));
    await session.tap(find.byValueKey('user-input-submit'));
  } else if (await _isVisible(session, 'user-input-first-text')) {
    await session.tap(find.byValueKey('user-input-first-text'));
    await session.enterText('确认');
    await session.tap(find.byValueKey('user-input-submit'));
  } else {
    final input = find.byValueKey('fallback-user-input');
    await session.tap(input);
    await session.enterText('确认');
    await session.tap(find.byValueKey('fallback-user-input-submit'));
  }
}

Future<bool> _isVisible(FlutterDriverSession session, String key) async {
  try {
    await session.waitFor(
      find.byValueKey(key),
      timeout: const Duration(seconds: 5),
    );
    return true;
  } on Object {
    return false;
  }
}

Future<Map<String, dynamic>> _waitForSnapshot(
  FlutterDriverSession session,
  File output,
  String label,
  bool Function(Map<String, dynamic>) predicate, {
  Duration timeout = const Duration(seconds: 30),
}) async {
  final deadline = DateTime.now().add(timeout);
  Map<String, dynamic>? last;
  while (DateTime.now().isBefore(deadline)) {
    last = await _appendSnapshot(session, output, label);
    if (predicate(last)) return last;
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw StateError('$label timed out; last=$last');
}

Future<Map<String, dynamic>> _appendSnapshot(
  FlutterDriverSession session,
  File output,
  String label,
) async {
  final snapshot = await session.readSnapshot();
  await _append(output, {'stage': label, 'snapshot': snapshot});
  return snapshot;
}

Future<void> _append(File output, Object value) => output.writeAsString(
  '${jsonEncode(value)}\n',
  mode: FileMode.append,
  flush: true,
);

Map<String, dynamic>? _workflow(Map<String, dynamic> snapshot) {
  final workflow = snapshot['workflow'];
  return workflow is Map<String, dynamic> ? workflow : null;
}

class _Options {
  _Options({
    required this.vmServiceUrl,
    required this.mode,
    required this.studioMode,
    required this.workspace,
    required this.promptFile,
    required this.snapshotOutput,
    required this.attempt,
    required this.shutdownAfterCompletion,
    required this.workflowTimeout,
    required this.stallTimeout,
  });

  final String vmServiceUrl;
  final String mode;
  final String studioMode;
  final String? workspace;
  final String? promptFile;
  final String snapshotOutput;
  final int attempt;
  final bool shutdownAfterCompletion;
  final Duration workflowTimeout;
  final Duration stallTimeout;

  static _Options parse(List<String> args) {
    String? value(String name) {
      final index = args.indexOf(name);
      return index < 0 || index + 1 >= args.length ? null : args[index + 1];
    }

    final vm = value('--vm-service-url');
    final mode = value('--mode') ?? 'new';
    final studioMode = value('--studio-mode') ?? 'mode.task';
    if (studioMode != 'mode.simple' && studioMode != 'mode.task') {
      throw ArgumentError('--studio-mode must be mode.simple or mode.task');
    }
    final output = value('--snapshot-output');
    if (vm == null || output == null) {
      throw ArgumentError(
        '--vm-service-url and --snapshot-output are required',
      );
    }
    final workspace = value('--workspace');
    final prompt = value('--prompt-file');
    if (mode == 'new' && (workspace == null || prompt == null)) {
      throw ArgumentError(
        '--workspace and --prompt-file are required for new mode',
      );
    }
    return _Options(
      vmServiceUrl: vm,
      mode: mode,
      studioMode: studioMode,
      workspace: workspace,
      promptFile: prompt,
      snapshotOutput: output,
      attempt: int.tryParse(value('--attempt') ?? '1') ?? 1,
      shutdownAfterCompletion: value('--shutdown-after-completion') != 'false',
      workflowTimeout: Duration(
        seconds:
            int.tryParse(value('--workflow-timeout-seconds') ?? '1800') ?? 1800,
      ),
      stallTimeout: Duration(
        seconds: int.tryParse(value('--stall-timeout-seconds') ?? '600') ?? 600,
      ),
    );
  }
}

class _ProgressWatch {
  String? _fingerprint;
  DateTime _changedAt = DateTime.now();

  void observe(
    Map<String, dynamic> snapshot,
    Duration stallTimeout,
    String label,
  ) {
    final workspace = snapshot['workspace'] as Map?;
    final timeline = workspace?['timeline'] as List? ?? const [];
    final fingerprint = jsonEncode({
      'workflow': snapshot['workflow'],
      'interaction': workspace?['activeInteraction'],
      'timeline': timeline,
    });
    if (_fingerprint != fingerprint) {
      _fingerprint = fingerprint;
      _changedAt = DateTime.now();
      return;
    }
    if (DateTime.now().difference(_changedAt) >= stallTimeout) {
      throw StateError(
        '$label made no model, tool, state, or GUI progress for '
        '${stallTimeout.inMinutes} minutes',
      );
    }
  }
}
