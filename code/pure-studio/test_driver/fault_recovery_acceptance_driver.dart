// 历史故障会话恢复与继续任务的真实 Flutter Driver 验收。
//
// 用法：
//   dart run test_driver/fault_recovery_acceptance_driver.dart \
//     --vm-service-url <url> \
//     --thread-id <thread-id> \
//     --failed-run-id <old-run-id> \
//     --current-run-id <new-run-id> \
//     --snapshot-output <jsonl> \
//     --screenshot-output <png>

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
    await session.waitFor(
      find.byValueKey('studio-shell'),
      timeout: const Duration(minutes: 2),
    );
    await _selectThread(session, snapshots, options.threadId);

    final baseline = await _snapshot(session, snapshots, 'baseline');
    final workspace = _workspace(baseline);
    final task = _task(baseline);
    if (workspace['turn'] != null || workspace['isBusy'] == true) {
      throw StateError(
        'recovered Thread still exposes an active Turn: $workspace',
      );
    }
    if (_persistence(baseline)['acceptsNewWork'] != true) {
      throw StateError(
        'persistence admission is not ready: ${_persistence(baseline)}',
      );
    }
    if (task['runId'] == options.failedRunId) {
      throw StateError('the recovered fatal TaskRun is still selected: $task');
    }
    if (task['runId'] != options.currentRunId) {
      throw StateError(
        'expected current TaskRun ${options.currentRunId}, got ${task['runId']}',
      );
    }
    if (_isFatalTask(task)) {
      throw StateError('the current TaskRun is unexpectedly fatal: $task');
    }

    final baselineLastTurnId = _lastTurnId(workspace);
    final baselineInteraction = workspace['activeInteraction'];
    final baselineInteractionId = baselineInteraction is Map<String, dynamic>
        ? baselineInteraction['id'] as String?
        : null;
    await _submitContinue(session, workspace);

    final progressing = await _waitForSnapshot(
      session,
      snapshots,
      'new planner activity after Continue',
      (snapshot) {
        final nextWorkspace = _workspaceOrNull(snapshot);
        final nextTask = _taskOrNull(snapshot);
        if (nextWorkspace == null || nextTask == null) return false;
        final interaction = nextWorkspace['activeInteraction'];
        final interactionId = interaction is Map<String, dynamic>
            ? interaction['id']
            : null;
        return nextTask['runId'] == options.currentRunId &&
            !_isFatalTask(nextTask) &&
            (nextWorkspace['turn'] != null ||
                _lastTurnId(nextWorkspace) != baselineLastTurnId ||
                interactionId != baselineInteractionId);
      },
      timeout: const Duration(minutes: 2),
    );
    stdout.writeln(
      jsonEncode({
        'event': 'plannerProgressObserved',
        'task': progressing['task'],
        'workspace': progressing['workspace'],
      }),
    );

    final settled = await _waitForSnapshot(
      session,
      snapshots,
      'continued planner Turn settlement',
      (snapshot) {
        final nextWorkspace = _workspaceOrNull(snapshot);
        final nextTask = _taskOrNull(snapshot);
        if (nextWorkspace == null || nextTask == null) return false;
        if (nextTask['runId'] != options.currentRunId ||
            _isFatalTask(nextTask)) {
          return false;
        }
        final lastTurn = nextWorkspace['lastTurn'];
        if (lastTurn is Map<String, dynamic> &&
            lastTurn['id'] != baselineLastTurnId &&
            lastTurn['status'] == 'failed') {
          throw StateError('continued planner Turn failed: $lastTurn');
        }
        return nextWorkspace['turn'] == null &&
            nextWorkspace['isBusy'] == false &&
            lastTurn is Map<String, dynamic> &&
            lastTurn['id'] != baselineLastTurnId &&
            lastTurn['status'] == 'completed';
      },
      timeout: const Duration(minutes: 10),
    );

    await File(options.screenshotOutput)
        .writeAsBytes(await session.screenshot(), flush: true);
    await _shutdown(session, snapshots);
    stdout.writeln(
      jsonEncode({
        'result': 'completed',
        'failedRunId': options.failedRunId,
        'currentRunId': options.currentRunId,
        'finalTask': settled['task'],
        'finalWorkspace': settled['workspace'],
        'screenshot': options.screenshotOutput,
      }),
    );
  } catch (error, stackTrace) {
    stderr.writeln('Fault recovery Driver failed: $error');
    stderr.writeln(stackTrace);
    if (session != null) {
      try {
        await _snapshot(session, snapshots, 'failure');
      } on Object {
        // 保留原始验收错误。
      }
      try {
        await File('${options.screenshotOutput}.failure.png')
            .writeAsBytes(await session.screenshot(), flush: true);
      } on Object {
        // 保留原始验收错误。
      }
    }
    rethrow;
  } finally {
    if (session != null) {
      try {
        await session.close().timeout(const Duration(seconds: 10));
      } on Object {
        // GUI 进程树由 xtask 持有。
      }
    }
  }
}

Future<void> _selectThread(
  FlutterDriverSession session,
  File snapshots,
  String threadId,
) async {
  for (var page = 0; page < 20; page += 1) {
    final snapshot = await _snapshot(session, snapshots, 'directory-$page');
    final directory = snapshot['sidebarDirectory'];
    final ids = directory is Map<String, dynamic> ? directory['ids'] : null;
    if (ids is List && ids.contains(threadId)) break;
    if (directory is! Map<String, dynamic> || directory['hasMore'] != true) {
      throw StateError('Thread $threadId is absent from the loaded directory');
    }
    await session.requestData('sidebar-load-more');
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }

  await session.tap(find.byValueKey('thread-row-$threadId'));
  await _waitForSnapshot(
    session,
    snapshots,
    'select Thread $threadId',
    (snapshot) => _workspaceOrNull(snapshot)?['threadId'] == threadId,
  );
}

Future<void> _submitContinue(
  FlutterDriverSession session,
  Map<String, dynamic> workspace,
) async {
  final interaction = workspace['activeInteraction'];
  if (interaction is Map<String, dynamic> &&
      interaction['kind'] == 'planConfirmation') {
    final input = find.byValueKey('plan-adjustment-input');
    await session.waitFor(input, timeout: const Duration(seconds: 30));
    await session.tap(input);
    await session.enterText('继续');
    if (await session.getText(input) != '继续') {
      throw StateError('plan adjustment did not retain Continue prompt');
    }
    await session.waitForNoPendingFrame(timeout: const Duration(seconds: 15));
    await session.tap(find.byValueKey('plan-revise'));
    return;
  }
  if (interaction is Map<String, dynamic> &&
      interaction['kind'] == 'userInput') {
    final input = find.byValueKey('fallback-user-input');
    await session.waitFor(input, timeout: const Duration(seconds: 30));
    await session.tap(input);
    await session.enterText('继续');
    if (await session.getText(input) != '继续') {
      throw StateError('fallback input did not retain Continue prompt');
    }
    await session.waitForNoPendingFrame(timeout: const Duration(seconds: 15));
    await session.tap(find.byValueKey('fallback-user-input-submit'));
    return;
  }
  if (interaction != null) {
    throw StateError(
      'unsupported active interaction for Continue: $interaction',
    );
  }
  final input = find.byValueKey('composer-input');
  await session.waitFor(input, timeout: const Duration(seconds: 30));
  await session.tap(input);
  await session.enterText('继续');
  if (await session.getText(input) != '继续') {
    throw StateError('Composer did not retain Continue prompt');
  }
  await session.waitForNoPendingFrame(timeout: const Duration(seconds: 15));
  await session.tap(find.byValueKey('composer-submit'));
}

Future<void> _shutdown(FlutterDriverSession session, File snapshots) async {
  await session.requestData('shutdown-begin');
  final completed = await session.requestData(
    'shutdown-await',
    timeout: const Duration(minutes: 2),
  );
  final response = jsonDecode(completed);
  if (response is! Map<String, dynamic> ||
      response['shutdown'] != 'completed') {
    throw StateError('unexpected shutdown response: $completed');
  }
  final snapshot = await _snapshot(session, snapshots, 'shutdown');
  final phases = snapshot['shutdownPhases'];
  if (phases is! List || !phases.contains('flushingPersistence:0')) {
    throw StateError('shutdown did not flush persistence to zero: $phases');
  }
}

Future<Map<String, dynamic>> _waitForSnapshot(
  FlutterDriverSession session,
  File snapshots,
  String description,
  bool Function(Map<String, dynamic>) predicate, {
  Duration timeout = const Duration(seconds: 30),
}) async {
  final deadline = DateTime.now().add(timeout);
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await _snapshot(session, snapshots, description);
    if (predicate(snapshot)) return snapshot;
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw TimeoutException('snapshot timed out: $description', timeout);
}

Future<Map<String, dynamic>> _snapshot(
  FlutterDriverSession session,
  File snapshots,
  String phase,
) async {
  final snapshot = await session.readSnapshot();
  await _append(snapshots, {
    'kind': 'snapshot',
    'phase': phase,
    'capturedAt': DateTime.now().toUtc().toIso8601String(),
    ...snapshot,
  });
  return snapshot;
}

Future<void> _append(File output, Map<String, Object?> record) {
  return output.writeAsString(
    '${jsonEncode(record)}\n',
    mode: FileMode.append,
    flush: true,
  );
}

Map<String, dynamic> _workspace(Map<String, dynamic> snapshot) =>
    _workspaceOrNull(snapshot) ??
    (throw StateError('snapshot has no selected workspace'));

Map<String, dynamic>? _workspaceOrNull(Map<String, dynamic> snapshot) {
  final value = snapshot['workspace'];
  return value is Map<String, dynamic> ? value : null;
}

Map<String, dynamic> _task(Map<String, dynamic> snapshot) =>
    _taskOrNull(snapshot) ??
    (throw StateError('snapshot has no current TaskRun'));

Map<String, dynamic>? _taskOrNull(Map<String, dynamic> snapshot) {
  final value = snapshot['task'];
  return value is Map<String, dynamic> ? value : null;
}

Map<String, dynamic> _persistence(Map<String, dynamic> snapshot) {
  final value = snapshot['persistence'];
  return value is Map<String, dynamic> ? value : const {};
}

String? _lastTurnId(Map<String, dynamic> workspace) {
  final value = workspace['lastTurn'];
  return value is Map<String, dynamic> ? value['id'] as String? : null;
}

bool _isFatalTask(Map<String, dynamic> task) {
  final outcome = task['outcome'];
  return outcome is Map<String, dynamic> &&
      outcome['kind'] == 'failed' &&
      outcome['failureKind'] == 'fatal';
}

class _Options {
  const _Options({
    required this.vmServiceUrl,
    required this.threadId,
    required this.failedRunId,
    required this.currentRunId,
    required this.snapshotOutput,
    required this.screenshotOutput,
  });

  final String vmServiceUrl;
  final String threadId;
  final String failedRunId;
  final String currentRunId;
  final String snapshotOutput;
  final String screenshotOutput;

  static _Options parse(List<String> arguments) {
    if (arguments.length.isOdd) {
      throw ArgumentError('expected --name value arguments');
    }
    final values = <String, String>{};
    for (var index = 0; index < arguments.length; index += 2) {
      final name = arguments[index];
      if (!name.startsWith('--')) {
        throw ArgumentError('expected --name value arguments');
      }
      values[name.substring(2)] = arguments[index + 1];
    }
    String required(String name) {
      final value = values[name];
      if (value == null || value.isEmpty) {
        throw ArgumentError('missing --$name');
      }
      return value;
    }

    return _Options(
      vmServiceUrl: required('vm-service-url'),
      threadId: required('thread-id'),
      failedRunId: required('failed-run-id'),
      currentRunId: required('current-run-id'),
      snapshotOutput: required('snapshot-output'),
      screenshotOutput: required('screenshot-output'),
    );
  }
}
