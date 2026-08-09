import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';
import 'task_progress.dart';

Future<void> main(List<String> arguments) async {
  final options = _DriverOptions.parse(arguments);
  final snapshots = File(options.snapshotOutput);
  final progressState = _ProgressState(File(options.progressStateOutput));
  await snapshots.parent.create(recursive: true);

  FlutterDriverSession? session;
  var recoveryApplied = false;
  try {
    session = await FlutterDriverSession.connect(
      vmServiceUrl: options.vmServiceUrl,
      onReconnect: (event) => _appendRecord(snapshots, event.toJson()),
    );
    switch (options.mode) {
      case AcceptanceDriverMode.newRun:
        final prompt = await File(options.promptFile!).readAsString();
        await _startNewTask(session, options, snapshots, prompt);
      case AcceptanceDriverMode.observe:
        final observedSnapshot = await _waitForSnapshot(
          session,
          'durable Task observation',
          (snapshot) {
            final workspace = snapshot['workspace'];
            return workspace is Map<String, dynamic> &&
                workspace['threadId'] is String;
          },
          deadline: options.taskDeadline,
          output: snapshots,
          options: options,
        );
        stdout.writeln(
          jsonEncode({
            'result': 'observed',
            'mode': options.mode.label,
            'attempt': options.attempt,
            ...observedSnapshot,
          }),
        );
        return;
      case AcceptanceDriverMode.resume:
        recoveryApplied = await _resumeTaskIfPaused(
          session,
          options,
          snapshots,
        );
        if (recoveryApplied) await progressState.resetAfterRecovery();
    }

    final finalSnapshot = await _waitForTaskCompletion(
      session,
      snapshots,
      progressState,
      deadline: options.taskDeadline,
      stallTimeout: options.stallTimeout,
      options: options,
    );
    if (options.stopAtRecoveryPause &&
        _hasFailedExecutorRecoveryCandidate(finalSnapshot)) {
      stdout.writeln(
        jsonEncode({
          'event': 'taskPausedForRecovery',
          'attempt': options.attempt,
          'capturedAt': DateTime.now().toUtc().toIso8601String(),
          'task': finalSnapshot['task'],
          'workspace': finalSnapshot['workspace'],
        }),
      );
      stdout.writeln(
        jsonEncode({
          'result': 'paused',
          'mode': options.mode.label,
          'attempt': options.attempt,
          'recoveryApplied': recoveryApplied,
          ...finalSnapshot,
        }),
      );
      return;
    }
    validateTaskCompletion(finalSnapshot);
    stdout.writeln(
      jsonEncode({
        'result': 'completed',
        'mode': options.mode.label,
        'attempt': options.attempt,
        'recoveryApplied': recoveryApplied,
        ...finalSnapshot,
      }),
    );
  } catch (error, stackTrace) {
    stderr.writeln('Task Driver failed: $error');
    stderr.writeln(stackTrace);
    if (session != null) {
      try {
        await _snapshot(session, snapshots, options);
      } on Object {
        // Preserve the original Driver failure.
      }
      try {
        final tree = await _driverCommand(
          session.renderTree(),
          'render tree capture',
        );
        await File(
          '${options.snapshotOutput}.render-tree.txt',
        ).writeAsString(tree, flush: true);
      } on Object {
        // Preserve the original Driver failure.
      }
    }
    rethrow;
  } finally {
    if (session != null) {
      try {
        await session.close().timeout(const Duration(seconds: 10));
      } on Object {
        // The harness owns and terminates the GUI process tree.
      }
    }
  }
}

Future<void> _startNewTask(
  FlutterDriverSession session,
  _DriverOptions options,
  File snapshots,
  String prompt,
) async {
  await _openWorkspace(session, options.workspace!);
  await _selectTaskMode(session, snapshots, options);
  await _submitPrompt(session, snapshots, options, prompt);
  await _waitForSnapshot(
    session,
    'plan confirmation',
    _hasPlanConfirmation,
    deadline: _earlierDeadline(
      DateTime.now().add(options.planTimeout),
      options.taskDeadline,
    ),
    output: snapshots,
    options: options,
  );
  await _driverCommand(
    session.waitFor(
      find.byValueKey('plan-implement'),
      timeout: const Duration(seconds: 30),
    ),
    'plan confirmation',
    const Duration(seconds: 30),
  );
  await _sideEffectOnce(
    session,
    snapshots,
    options,
    description: 'implement plan tap',
    action: () => session.tap(find.byValueKey('plan-implement')),
    postcondition: (snapshot) => !_hasPlanConfirmation(snapshot),
  );
}

Future<bool> _resumeTaskIfPaused(
  FlutterDriverSession session,
  _DriverOptions options,
  File snapshots,
) async {
  final snapshot = await _waitForSnapshot(
    session,
    'durable Task resume state',
    (snapshot) => snapshot['task'] is Map<String, dynamic>,
    deadline: options.taskDeadline,
    output: snapshots,
    options: options,
  );
  final task = snapshot['task'] as Map<String, dynamic>;
  if (task['phase'] == 'completed' || !_isTaskPaused(snapshot)) return false;
  if (options.recoveryCount >= 3) {
    throw StateError(
      'Task recovery loop limit reached before a fourth recovery',
    );
  }

  await _driverCommand(
    session.tap(find.byValueKey('task-resume')),
    'open Task recovery',
  );
  await _driverCommand(
    session.waitFor(
      find.byValueKey('task-recovery-dialog'),
      timeout: const Duration(seconds: 30),
    ),
    'Task recovery dialog',
  );
  await _selectRecoveryMode(session, options.recoveryMode);
  await _driverCommand(
    session.tap(find.byValueKey('task-recovery-confirm')),
    'Task recovery impact confirmation',
  );
  await _driverCommand(
    session.waitFor(
      find.byValueKey('task-recovery-apply'),
      timeout: const Duration(seconds: 30),
    ),
    'Task recovery final confirmation',
  );
  final recoverySnapshot = await _sideEffectOnce(
    session,
    snapshots,
    options,
    description: 'Task recovery apply',
    action: () => session.tap(find.byValueKey('task-recovery-apply')),
    postcondition: (snapshot) => !_isTaskPaused(snapshot),
    deadline: options.taskDeadline,
  );
  stdout.writeln(
    jsonEncode({
      'event': 'taskRecoveryApplied',
      'attempt': options.attempt,
      'capturedAt': DateTime.now().toUtc().toIso8601String(),
      'recovery': recoverySnapshot['taskRecovery'],
      'task': recoverySnapshot['task'],
    }),
  );
  return true;
}

Future<void> _selectRecoveryMode(
  FlutterDriverSession session,
  AcceptanceRecoveryMode recoveryMode,
) async {
  if (recoveryMode == AcceptanceRecoveryMode.auto) return;
  final option = find.byValueKey(
    'task-recovery-mode-${recoveryMode.protocolName}',
  );
  await _driverCommand(
    session.tap(find.byValueKey('task-recovery-mode')),
    'open Task recovery mode selector',
  );
  await _driverCommand(
    session.waitFor(option, timeout: const Duration(seconds: 30)),
    'Task recovery mode option',
  );
  await _driverCommand(
    session.tap(option),
    'select Task recovery mode ${recoveryMode.protocolName}',
  );
}

Future<void> _openWorkspace(
  FlutterDriverSession session,
  String workspace,
) async {
  await _driverCommand(
    session.waitFor(
      find.byValueKey('studio-shell'),
      timeout: const Duration(minutes: 2),
    ),
    'Studio shell',
    const Duration(minutes: 2),
  );
  await _driverCommand(
    session.tap(find.byValueKey('sidebar-open-project')),
    'open project tap',
  );
  await _driverCommand(
    session.waitFor(find.byValueKey('project-path-dialog')),
    'project path dialog',
  );
  await _driverCommand(
    session.tap(find.byValueKey('project-path-input')),
    'project path input tap',
  );
  await _driverCommand(session.enterText(workspace), 'project path entry');
  await _driverCommand(
    session.waitUntilNoTransientCallbacks(timeout: const Duration(seconds: 5)),
    'project path input settled',
    const Duration(seconds: 10),
  );
  await _driverCommand(
    session.sendTextInputAction(
      TextInputAction.done,
      timeout: const Duration(seconds: 5),
    ),
    'project path submit action',
    const Duration(seconds: 10),
  );
  await _driverCommand(
    session.waitFor(
      find.byValueKey('composer-input'),
      timeout: const Duration(minutes: 1),
    ),
    'composer after project open',
    const Duration(minutes: 1),
  );
}

Future<void> _selectTaskMode(
  FlutterDriverSession session,
  File snapshots,
  _DriverOptions options,
) async {
  await _driverCommand(
    session.tap(find.byValueKey('session-mode-selector')),
    'session mode selector tap',
  );
  await _driverCommand(
    session.waitFor(find.byValueKey('session-mode-task')),
    'Task mode option',
  );
  await _sideEffectOnce(
    session,
    snapshots,
    options,
    description: 'Task mode option tap',
    action: () => session.tap(find.byValueKey('session-mode-task')),
    postcondition: (snapshot) {
      final workspace = snapshot['workspace'];
      return workspace is Map<String, dynamic> &&
          workspace['threadMode'] == 'task';
    },
  );
}

Future<void> _submitPrompt(
  FlutterDriverSession session,
  File snapshots,
  _DriverOptions options,
  String prompt,
) async {
  await _driverCommand(
    session.tap(find.byValueKey('composer-input')),
    'composer input tap',
  );
  await _driverCommand(session.enterText(prompt.trim()), 'prompt entry');
  await _sideEffectOnce(
    session,
    snapshots,
    options,
    description: 'prompt submit',
    action: () => session.tap(find.byValueKey('composer-submit')),
    onActionDispatched: () async {
      stdout.writeln(
        jsonEncode({
          'event': 'originalPromptSubmitted',
          'attempt': options.attempt,
          'capturedAt': DateTime.now().toUtc().toIso8601String(),
        }),
      );
      if (options.injectSnapshotDisconnect) {
        await session.disconnectObservationForAcceptance();
      }
    },
    postcondition: hasSubmittedTaskPrompt,
  );
}

Future<Map<String, dynamic>> _sideEffectOnce(
  FlutterDriverSession session,
  File snapshots,
  _DriverOptions options, {
  required String description,
  required Future<void> Function() action,
  required bool Function(Map<String, dynamic>) postcondition,
  FutureOr<void> Function()? onActionDispatched,
  DateTime? deadline,
}) async {
  try {
    await _driverCommand(action(), description);
  } catch (error) {
    if (!isReconnectableReadFailure(error)) rethrow;
  }
  if (onActionDispatched != null) await onActionDispatched();
  return _waitForSnapshot(
    session,
    '$description postcondition',
    postcondition,
    deadline: deadline ?? DateTime.now().add(const Duration(seconds: 30)),
    output: snapshots,
    options: options,
  );
}

bool hasSubmittedTaskPrompt(Map<String, dynamic> snapshot) {
  final workspace = snapshot['workspace'];
  if (workspace is! Map<String, dynamic>) return false;
  if (workspace['turn'] != null || snapshot['task'] is Map<String, dynamic>) {
    return true;
  }
  final planContent = snapshot['planContent'];
  final interaction = workspace['activeInteraction'];
  return planContent is String &&
      planContent.isNotEmpty &&
      interaction is Map<String, dynamic> &&
      interaction['kind'] == 'planConfirmation';
}

Future<Map<String, dynamic>> _waitForTaskCompletion(
  FlutterDriverSession session,
  File snapshots,
  _ProgressState progressState, {
  required DateTime deadline,
  required Duration stallTimeout,
  required _DriverOptions options,
}) async {
  final progress = await progressState.load();
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await _snapshot(session, snapshots, options);
    final task = snapshot['task'];
    if (task is Map<String, dynamic>) {
      final phase = task['phase'] as String? ?? '';
      if (phase == 'completed') return snapshot;
      if (options.stopAtRecoveryPause &&
          _hasFailedExecutorRecoveryCandidate(snapshot)) {
        return snapshot;
      }
      if (const {'blocked', 'failed', 'cancelled'}.contains(phase)) {
        throw StateError(
          'Task entered terminal failure phase $phase: ${task['statusMessage']}',
        );
      }
      final fingerprint = taskProgressFingerprint(snapshot);
      if (fingerprint != progress.fingerprint) {
        progress
          ..fingerprint = fingerprint
          ..lastProgressAt = DateTime.now();
        await progressState.save(progress);
      } else if (DateTime.now().difference(progress.lastProgressAt) >
          stallTimeout) {
        throw StateError('Task made no observable progress for $stallTimeout');
      }
    }
    await Future<void>.delayed(const Duration(seconds: 1));
  }
  throw TimeoutException(
    'Task did not complete before ${deadline.toUtc().toIso8601String()}',
  );
}

Future<Map<String, dynamic>> _snapshot(
  FlutterDriverSession session,
  File output,
  _DriverOptions options,
) async {
  final snapshot = await session.readSnapshot();
  await _appendRecord(output, {
    'kind': 'snapshot',
    'capturedAt': DateTime.now().toUtc().toIso8601String(),
    'mode': options.mode.label,
    'attempt': options.attempt,
    ...snapshot,
  });
  return snapshot;
}

Future<Map<String, dynamic>> _waitForSnapshot(
  FlutterDriverSession session,
  String description,
  bool Function(Map<String, dynamic> snapshot) predicate, {
  required DateTime deadline,
  required _DriverOptions options,
  File? output,
}) async {
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = output == null
        ? await session.readSnapshot()
        : await _snapshot(session, output, options);
    if (predicate(snapshot)) return snapshot;
    await Future<void>.delayed(const Duration(milliseconds: 200));
  }
  throw TimeoutException('Flutter Driver snapshot timed out: $description');
}

bool _hasPlanConfirmation(Map<String, dynamic> snapshot) {
  final planContent = snapshot['planContent'];
  final workspace = snapshot['workspace'];
  final interaction = workspace is Map<String, dynamic>
      ? workspace['activeInteraction']
      : null;
  return planContent is String &&
      planContent.isNotEmpty &&
      interaction is Map<String, dynamic> &&
      interaction['kind'] == 'planConfirmation';
}

bool _isTaskPaused(Map<String, dynamic> snapshot) {
  final workspace = snapshot['workspace'];
  return workspace is Map<String, dynamic> && workspace['isTaskPaused'] == true;
}

bool _hasFailedExecutorRecoveryCandidate(Map<String, dynamic> snapshot) {
  final task = snapshot['task'];
  if (task is! Map<String, dynamic> || task['phase'] == 'completed') {
    return false;
  }
  final workUnits = task['workUnits'];
  return workUnits is List<dynamic> &&
      workUnits.whereType<Map<String, dynamic>>().any(
        (unit) =>
            const {'failed', 'interrupted'}.contains(unit['executionStatus']),
      );
}

DateTime _earlierDeadline(DateTime first, DateTime second) {
  return first.isBefore(second) ? first : second;
}

Future<void> _appendRecord(File output, Map<String, Object?> record) {
  return output.writeAsString(
    '${jsonEncode(record)}\n',
    mode: FileMode.append,
    flush: true,
  );
}

Future<T> _driverCommand<T>(
  Future<T> command,
  String description, [
  Duration timeout = const Duration(seconds: 30),
]) {
  return command.timeout(
    timeout,
    onTimeout: () => throw TimeoutException(
      'Flutter Driver command timed out: $description',
      timeout,
    ),
  );
}

enum AcceptanceDriverMode {
  newRun('new'),
  observe('observe'),
  resume('resume');

  const AcceptanceDriverMode(this.label);

  final String label;

  static AcceptanceDriverMode parse(String value) {
    return values.firstWhere(
      (mode) => mode.label == value.toLowerCase(),
      orElse: () => throw ArgumentError('unsupported --mode $value'),
    );
  }
}

enum AcceptanceRecoveryMode {
  auto('auto'),
  rewindTail('rewindTail'),
  rebuildThread('rebuildThread');

  const AcceptanceRecoveryMode(this.protocolName);

  final String protocolName;

  static AcceptanceRecoveryMode parse(String value) {
    return AcceptanceRecoveryMode.values.firstWhere(
      (mode) => mode.protocolName.toLowerCase() == value.toLowerCase(),
      orElse: () => throw ArgumentError('unsupported --recovery-mode $value'),
    );
  }
}

class _DriverOptions {
  const _DriverOptions({
    required this.mode,
    required this.vmServiceUrl,
    required this.workspace,
    required this.promptFile,
    required this.snapshotOutput,
    required this.progressStateOutput,
    required this.planTimeout,
    required this.taskDeadline,
    required this.stallTimeout,
    required this.attempt,
    required this.recoveryCount,
    required this.recoveryMode,
    required this.injectSnapshotDisconnect,
    required this.stopAtRecoveryPause,
  });

  final AcceptanceDriverMode mode;
  final String vmServiceUrl;
  final String? workspace;
  final String? promptFile;
  final String snapshotOutput;
  final String progressStateOutput;
  final Duration planTimeout;
  final DateTime taskDeadline;
  final Duration stallTimeout;
  final int attempt;
  final int recoveryCount;
  final AcceptanceRecoveryMode recoveryMode;
  final bool injectSnapshotDisconnect;
  final bool stopAtRecoveryPause;

  static _DriverOptions parse(List<String> arguments) {
    final values = <String, List<String>>{};
    for (var index = 0; index < arguments.length; index += 2) {
      if (index + 1 >= arguments.length || !arguments[index].startsWith('--')) {
        throw ArgumentError('expected --name value arguments');
      }
      values
          .putIfAbsent(arguments[index].substring(2), () => <String>[])
          .add(arguments[index + 1]);
    }
    String required(String name) {
      final value = values[name]?.lastOrNull;
      if (value == null || value.isEmpty) {
        throw ArgumentError('missing --$name');
      }
      return value;
    }

    String? optional(String name) {
      final value = values[name]?.lastOrNull;
      return value == null || value.isEmpty ? null : value;
    }

    int integer(String name, int fallback) =>
        int.tryParse(values[name]?.lastOrNull ?? '') ?? fallback;

    bool boolean(String name) =>
        (values[name]?.lastOrNull ?? '').toLowerCase() == 'true';

    final mode = AcceptanceDriverMode.parse(optional('mode') ?? 'new');
    final workspace = optional('workspace');
    final promptFile = optional('prompt-file');
    if (mode == AcceptanceDriverMode.newRun &&
        (workspace == null || promptFile == null)) {
      throw ArgumentError('New mode requires --workspace and --prompt-file');
    }
    final taskTimeout = Duration(
      seconds: integer('task-timeout-seconds', 3600),
    );
    final deadlineValue = optional('deadline-utc');
    final deadline = deadlineValue == null
        ? DateTime.now().add(taskTimeout)
        : DateTime.parse(deadlineValue).toUtc();
    return _DriverOptions(
      mode: mode,
      vmServiceUrl: required('vm-service-url'),
      workspace: workspace,
      promptFile: promptFile,
      snapshotOutput: required('snapshot-output'),
      progressStateOutput: required('progress-state-output'),
      planTimeout: Duration(seconds: integer('plan-timeout-seconds', 300)),
      taskDeadline: deadline,
      stallTimeout: Duration(seconds: integer('stall-timeout-seconds', 300)),
      attempt: integer('attempt', 1),
      recoveryCount: integer('recovery-count', 0),
      recoveryMode: AcceptanceRecoveryMode.parse(
        optional('recovery-mode') ?? 'auto',
      ),
      injectSnapshotDisconnect: boolean('inject-snapshot-disconnect'),
      stopAtRecoveryPause: boolean('stop-at-recovery-pause'),
    );
  }
}

class _ProgressState {
  const _ProgressState(this.file);

  final File file;

  Future<_MutableProgress> load() async {
    if (!await file.exists()) {
      return _MutableProgress(
        fingerprint: null,
        lastProgressAt: DateTime.now(),
      );
    }
    final decoded = jsonDecode(await file.readAsString());
    if (decoded is! Map<String, dynamic>) {
      throw const FormatException('progress state must be a JSON object');
    }
    return _MutableProgress(
      fingerprint: decoded['fingerprint'] as String?,
      lastProgressAt: DateTime.parse(decoded['lastProgressAt'] as String),
    );
  }

  Future<void> save(_MutableProgress progress) async {
    await file.parent.create(recursive: true);
    await file.writeAsString(
      jsonEncode({
        'fingerprint': progress.fingerprint,
        'lastProgressAt': progress.lastProgressAt.toUtc().toIso8601String(),
      }),
      flush: true,
    );
  }

  Future<void> resetAfterRecovery() {
    return save(
      _MutableProgress(fingerprint: null, lastProgressAt: DateTime.now()),
    );
  }
}

class _MutableProgress {
  _MutableProgress({required this.fingerprint, required this.lastProgressAt});

  String? fingerprint;
  DateTime lastProgressAt;
}
