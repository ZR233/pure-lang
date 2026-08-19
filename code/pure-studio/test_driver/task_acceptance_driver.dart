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
  late final _TaskObservationTarget taskTarget;
  try {
    session = await FlutterDriverSession.connect(
      vmServiceUrl: options.vmServiceUrl,
      onReconnect: (event) => _appendRecord(snapshots, event.toJson()),
    );
    switch (options.mode) {
      case AcceptanceDriverMode.newRun:
        final prompt = await File(options.promptFile!).readAsString();
        taskTarget = await _startNewTask(session, options, snapshots, prompt);
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
        final resume = await _resumeTaskIfPaused(session, options, snapshots);
        recoveryApplied = resume.recoveryApplied;
        taskTarget = resume.target;
        if (recoveryApplied) await progressState.resetAfterRecovery();
    }

    final budgetRecoveryEvidence = options.expectBudgetRecovery
        ? BudgetRecoveryEvidence()
        : null;
    final finalSnapshot = await _waitForTaskCompletion(
      session,
      snapshots,
      progressState,
      target: taskTarget,
      deadline: options.taskDeadline,
      stallTimeout: options.stallTimeout,
      options: options,
      budgetRecoveryEvidence: budgetRecoveryEvidence,
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
    if (options.expectedTaskPhase == 'failed') {
      validateFatalTaskFailure(finalSnapshot);
      await _openFatalTaskFailureDetail(session, finalSnapshot);
    } else {
      validateTaskCompletion(finalSnapshot);
    }
    if (budgetRecoveryEvidence != null) {
      stdout.writeln(
        jsonEncode({
          'event': 'budgetRecoveryObserved',
          'attempt': options.attempt,
          'capturedAt': DateTime.now().toUtc().toIso8601String(),
          'evidence': budgetRecoveryEvidence.validate(),
        }),
      );
    }
    await File(
      '${options.snapshotOutput}.png',
    ).writeAsBytes(await session.screenshot(), flush: true);
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
      try {
        await File(
          '${options.snapshotOutput}.failure.png',
        ).writeAsBytes(await session.screenshot(), flush: true);
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

Future<void> _openFatalTaskFailureDetail(
  FlutterDriverSession session,
  Map<String, dynamic> snapshot,
) async {
  final task = snapshot['task'] as Map<String, dynamic>;
  final runId = task['runId'] as String;
  final failure = task['terminalFailure'] as Map<String, dynamic>;
  final failureId = failure['id'] as String;
  final phaseReadout = find.byValueKey('task-runtime-$runId-phase-failed');

  await _driverCommand(
    session.waitFor(phaseReadout, timeout: const Duration(seconds: 30)),
    'fatal Task status readout',
  );
  await _driverCommand(
    session.tap(phaseReadout),
    'open fatal Task failure detail',
  );
  await _driverCommand(
    session.waitFor(
      find.byValueKey('task-failure-$failureId'),
      timeout: const Duration(seconds: 30),
    ),
    'fatal Task failure detail',
  );
  await _driverCommand(
    session.waitUntilNoTransientCallbacks(timeout: const Duration(seconds: 30)),
    'fatal Task failure detail stabilization',
  );
}

Future<_TaskObservationTarget> _startNewTask(
  FlutterDriverSession session,
  _DriverOptions options,
  File snapshots,
  String prompt,
) async {
  await _openWorkspace(session, snapshots, options, options.workspace!);
  await _selectTaskMode(session, snapshots, options);
  final threadId = await _submitPrompt(session, snapshots, options, prompt);
  await _waitForSnapshot(
    session,
    'plan confirmation',
    (snapshot) =>
        isSelectedProjectWorkspace(snapshot, options.workspace!) &&
        isTaskThread(snapshot, threadId) &&
        _hasPlanConfirmation(snapshot),
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
  final implementationSnapshot = await _sideEffectOnce(
    session,
    snapshots,
    options,
    description: 'implement plan tap',
    action: () => session.tap(find.byValueKey('plan-implement')),
    postcondition: (snapshot) =>
        isSelectedProjectWorkspace(snapshot, options.workspace!) &&
        isTaskThread(snapshot, threadId) &&
        !_hasPlanConfirmation(snapshot) &&
        snapshot['task'] is Map<String, dynamic>,
  );
  return _TaskObservationTarget.fromSnapshot(implementationSnapshot);
}

Future<({bool recoveryApplied, _TaskObservationTarget target})>
_resumeTaskIfPaused(
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
  final target = _TaskObservationTarget.fromSnapshot(snapshot);
  if (task['phase'] == 'completed' || !_isTaskPaused(snapshot)) {
    return (recoveryApplied: false, target: target);
  }
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
    postcondition: (snapshot) =>
        target.matches(snapshot) && !_isTaskPaused(snapshot),
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
  return (recoveryApplied: true, target: target);
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
  File snapshots,
  _DriverOptions options,
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
    session.waitForAbsent(
      find.byValueKey('project-path-dialog'),
      timeout: const Duration(seconds: 30),
    ),
    'project path dialog close',
  );
  await _waitForSnapshot(
    session,
    'selected project',
    (snapshot) => isSelectedProject(snapshot, workspace),
    deadline: DateTime.now().add(const Duration(minutes: 2)),
    output: snapshots,
    options: options,
  );
  await _driverCommand(
    session.waitFor(
      find.byValueKey('sidebar-new-session'),
      timeout: const Duration(minutes: 1),
    ),
    'new session after project open',
    const Duration(minutes: 1),
  );
  await _sideEffectOnce(
    session,
    snapshots,
    options,
    description: 'new session after project open',
    action: () => session.tap(find.byValueKey('sidebar-new-session')),
    postcondition: (snapshot) =>
        isSelectedProjectStartPage(snapshot, workspace) &&
        _newThreadComposer(snapshot)['phase'] == 'idle',
    deadline: DateTime.now().add(const Duration(minutes: 1)),
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
  await _driverCommand(
    session.waitUntilNoTransientCallbacks(timeout: const Duration(seconds: 5)),
    'Task mode menu settled',
    const Duration(seconds: 10),
  );
  await _sideEffectOnce(
    session,
    snapshots,
    options,
    description: 'Task mode option tap',
    action: () => session.tap(find.byValueKey('session-mode-task')),
    postcondition: (snapshot) =>
        isTransientTaskDraft(snapshot, options.workspace!),
  );
}

Future<String> _submitPrompt(
  FlutterDriverSession session,
  File snapshots,
  _DriverOptions options,
  String prompt,
) async {
  await _waitForSnapshot(
    session,
    'transient Task draft before prompt entry',
    (snapshot) => isTransientTaskDraft(snapshot, options.workspace!),
    deadline: DateTime.now().add(const Duration(seconds: 30)),
    output: snapshots,
    options: options,
  );
  final composerInput = find.byValueKey('composer-input');
  final expectedPrompt = prompt.trim();
  await _driverCommand(session.tap(composerInput), 'composer input tap');
  await _driverCommand(session.enterText(expectedPrompt), 'prompt entry');
  final enteredPrompt = await _driverCommand(
    session.getText(composerInput),
    'prompt read-back',
  );
  if (enteredPrompt != expectedPrompt) {
    throw StateError(
      'prompt read-back mismatch: expected ${expectedPrompt.length} chars, '
      'received ${enteredPrompt.length}',
    );
  }
  await _driverCommand(
    session.waitUntilNoTransientCallbacks(timeout: const Duration(seconds: 5)),
    'prompt input settled',
    const Duration(seconds: 10),
  );
  // Wait for the draft change to be rebuilt so the submit button is actually
  // enabled before tapping; on slow CI runners the tap could otherwise land on
  // a still-disabled button and silently do nothing.
  await _driverCommand(
    session.waitForNoPendingFrame(timeout: const Duration(seconds: 15)),
    'prompt submit rebuild',
    const Duration(seconds: 20),
  );
  await _waitForSnapshot(
    session,
    'transient Task draft before prompt submit',
    (snapshot) => isTransientTaskDraft(snapshot, options.workspace!),
    deadline: DateTime.now().add(const Duration(seconds: 30)),
    output: snapshots,
    options: options,
  );
  final submitted = await _sideEffectOnce(
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
    postcondition: (snapshot) =>
        isSelectedProjectWorkspace(snapshot, options.workspace!) &&
        _workspaceMode(snapshot) == 'task' &&
        hasSubmittedTaskPrompt(snapshot),
  );
  return _snapshotThreadId(submitted);
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

Map<String, dynamic> _navigation(Map<String, dynamic> snapshot) {
  final navigation = snapshot['navigation'];
  return navigation is Map<String, dynamic>
      ? navigation
      : const <String, dynamic>{};
}

Map<String, dynamic> _newThreadComposer(Map<String, dynamic> snapshot) {
  final composer = _navigation(snapshot)['newThreadComposer'];
  return composer is Map<String, dynamic>
      ? composer
      : const <String, dynamic>{};
}

String? _workspaceMode(Map<String, dynamic> snapshot) {
  final workspace = snapshot['workspace'];
  return workspace is Map<String, dynamic>
      ? workspace['threadMode'] as String?
      : null;
}

bool isSelectedProject(Map<String, dynamic> snapshot, String expectedPath) {
  final project = snapshot['project'];
  if (project is! Map<String, dynamic>) return false;
  final projectId = project['id'];
  final projectPath = project['path'];
  return projectId is String &&
      projectPath is String &&
      _normalizedPath(projectPath) == _normalizedPath(expectedPath);
}

bool isSelectedProjectStartPage(
  Map<String, dynamic> snapshot,
  String expectedPath,
) {
  final navigation = _navigation(snapshot);
  return isSelectedProject(snapshot, expectedPath) &&
      navigation['selectedThreadId'] == null &&
      navigation['isStartPage'] == true &&
      snapshot['workspace'] == null;
}

bool isTransientTaskDraft(Map<String, dynamic> snapshot, String expectedPath) =>
    isSelectedProjectStartPage(snapshot, expectedPath) &&
    _navigation(snapshot)['newThreadMode'] == 'task';

bool isSelectedProjectWorkspace(
  Map<String, dynamic> snapshot,
  String expectedPath,
) {
  final project = snapshot['project'];
  final workspace = snapshot['workspace'];
  if (!isSelectedProject(snapshot, expectedPath) ||
      project is! Map<String, dynamic> ||
      workspace is! Map<String, dynamic>) {
    return false;
  }
  final projectId = project['id'];
  return projectId is String && workspace['projectId'] == projectId;
}

bool isTaskThread(Map<String, dynamic> snapshot, String threadId) {
  final workspace = snapshot['workspace'];
  return workspace is Map<String, dynamic> &&
      workspace['threadId'] == threadId &&
      workspace['threadMode'] == 'task';
}

bool isTaskRunOnTarget(
  Map<String, dynamic> snapshot, {
  required String projectId,
  required String projectPath,
  required String threadId,
  required String runId,
}) {
  final project = snapshot['project'];
  final task = snapshot['task'];
  return project is Map<String, dynamic> &&
      project['id'] == projectId &&
      isSelectedProjectWorkspace(snapshot, projectPath) &&
      isTaskThread(snapshot, threadId) &&
      task is Map<String, dynamic> &&
      task['runId'] == runId;
}

class _TaskObservationTarget {
  const _TaskObservationTarget({
    required this.projectId,
    required this.projectPath,
    required this.threadId,
    required this.runId,
  });

  factory _TaskObservationTarget.fromSnapshot(Map<String, dynamic> snapshot) {
    final project = snapshot['project'];
    final workspace = snapshot['workspace'];
    final task = snapshot['task'];
    final projectId = project is Map<String, dynamic> ? project['id'] : null;
    final projectPath = project is Map<String, dynamic>
        ? project['path']
        : null;
    final threadId = workspace is Map<String, dynamic>
        ? workspace['threadId']
        : null;
    final runId = task is Map<String, dynamic> ? task['runId'] : null;
    if (projectId is! String ||
        projectId.isEmpty ||
        projectPath is! String ||
        projectPath.isEmpty ||
        threadId is! String ||
        threadId.isEmpty ||
        runId is! String ||
        runId.isEmpty ||
        !isTaskRunOnTarget(
          snapshot,
          projectId: projectId,
          projectPath: projectPath,
          threadId: threadId,
          runId: runId,
        )) {
      throw StateError(
        'Flutter Driver snapshot does not identify one selected Task target',
      );
    }
    return _TaskObservationTarget(
      projectId: projectId,
      projectPath: projectPath,
      threadId: threadId,
      runId: runId,
    );
  }

  final String projectId;
  final String projectPath;
  final String threadId;
  final String runId;

  bool matches(Map<String, dynamic> snapshot) => isTaskRunOnTarget(
    snapshot,
    projectId: projectId,
    projectPath: projectPath,
    threadId: threadId,
    runId: runId,
  );
}

String _snapshotThreadId(Map<String, dynamic> snapshot) {
  final workspace = snapshot['workspace'];
  final threadId = workspace is Map<String, dynamic>
      ? workspace['threadId']
      : null;
  if (threadId is String && threadId.isNotEmpty) return threadId;
  throw StateError('Flutter Driver snapshot has no selected thread');
}

String _normalizedPath(String path) {
  var normalized = File(path).absolute.path.replaceAll('\\', '/');
  while (normalized.length > 1 && normalized.endsWith('/')) {
    normalized = normalized.substring(0, normalized.length - 1);
  }
  return Platform.isWindows ? normalized.toLowerCase() : normalized;
}

Future<Map<String, dynamic>> _waitForTaskCompletion(
  FlutterDriverSession session,
  File snapshots,
  _ProgressState progressState, {
  required _TaskObservationTarget target,
  required DateTime deadline,
  required Duration stallTimeout,
  required _DriverOptions options,
  BudgetRecoveryEvidence? budgetRecoveryEvidence,
}) async {
  final progress = await progressState.load();
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await _snapshot(session, snapshots, options);
    budgetRecoveryEvidence?.observe(snapshot);
    if (!target.matches(snapshot)) {
      throw StateError(
        'Flutter Driver selection moved away from Task '
        '${target.runId} on thread ${target.threadId}',
      );
    }
    final task = snapshot['task'];
    if (task is Map<String, dynamic>) {
      final phase = task['phase'] as String? ?? '';
      if (phase == options.expectedTaskPhase) return snapshot;
      if (options.stopAtRecoveryPause &&
          _hasFailedExecutorRecoveryCandidate(snapshot)) {
        return snapshot;
      }
      if (const {
        'completed',
        'blocked',
        'failed',
        'cancelled',
      }.contains(phase)) {
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
    required this.expectBudgetRecovery,
    required this.expectedTaskPhase,
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
  final bool expectBudgetRecovery;
  final String expectedTaskPhase;

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
    final expectedTaskPhase = optional('expected-task-phase') ?? 'completed';
    if (!const {'completed', 'failed'}.contains(expectedTaskPhase)) {
      throw ArgumentError(
        'unsupported --expected-task-phase $expectedTaskPhase',
      );
    }
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
      expectBudgetRecovery: boolean('expect-budget-recovery'),
      expectedTaskPhase: expectedTaskPhase,
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
