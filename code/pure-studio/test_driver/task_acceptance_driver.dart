import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'task_progress.dart';

Future<void> main(List<String> arguments) async {
  final options = _DriverOptions.parse(arguments);
  final prompt = await File(options.promptFile).readAsString();
  final snapshots = File(options.snapshotOutput);
  await snapshots.parent.create(recursive: true);

  FlutterDriver? driver;
  try {
    driver = await FlutterDriver.connect(
      dartVmServiceUrl: options.vmServiceUrl,
      printCommunication: false,
      logCommunicationToFile: false,
    ).timeout(const Duration(seconds: 30));
    await _driverCommand(
      driver.checkHealth(),
      'health check',
      const Duration(seconds: 15),
    );
    await _openWorkspace(driver, options.workspace);
    await _selectTaskMode(driver);
    await _submitPrompt(driver, prompt);
    await _waitForSnapshot(
      driver,
      'plan confirmation',
      (snapshot) {
        final planContent = snapshot['planContent'];
        final workspace = snapshot['workspace'];
        final interaction = workspace is Map<String, dynamic>
            ? workspace['activeInteraction']
            : null;
        return planContent is String &&
            planContent.isNotEmpty &&
            interaction is Map<String, dynamic> &&
            interaction['kind'] == 'planConfirmation';
      },
      timeout: options.planTimeout,
      output: snapshots,
    );
    await _driverCommand(
      driver.waitFor(
        find.byValueKey('plan-implement'),
        timeout: const Duration(seconds: 30),
      ),
      'plan confirmation',
      const Duration(seconds: 30),
    );

    await _driverCommand(
      driver.tap(find.byValueKey('plan-implement')),
      'implement plan tap',
    );
    final finalSnapshot = await _waitForTaskCompletion(
      driver,
      snapshots,
      timeout: options.taskTimeout,
      stallTimeout: options.stallTimeout,
    );
    validateTaskCompletion(finalSnapshot);
    stdout.writeln(jsonEncode({'result': 'completed', ...finalSnapshot}));
  } catch (error, stackTrace) {
    stderr.writeln('Task Driver failed: $error');
    stderr.writeln(stackTrace);
    if (driver != null) {
      try {
        final snapshot = await _requestSnapshot(driver);
        await snapshots.writeAsString(
          '${jsonEncode({'capturedAt': DateTime.now().toUtc().toIso8601String(), ...snapshot})}\n',
          mode: FileMode.append,
          flush: true,
        );
      } on Object {
        // Preserve the original Driver failure.
      }
      try {
        final tree = await _driverCommand(
          driver.getRenderTree(),
          'render tree capture',
        );
        await File(
          '${options.snapshotOutput}.render-tree.txt',
        ).writeAsString(tree.tree ?? '', flush: true);
      } on Object {
        // Preserve the original Driver failure.
      }
    }
    rethrow;
  } finally {
    if (driver != null) {
      try {
        await _driverCommand(
          driver.requestData('shutdown'),
          'runtime shutdown',
          const Duration(seconds: 20),
        );
      } on Object {
        // The harness terminates the owned process tree after this attempt.
      }
      await driver.close().timeout(const Duration(seconds: 10));
    }
  }
}

Future<void> _openWorkspace(FlutterDriver driver, String workspace) async {
  await _driverCommand(
    driver.waitFor(
      find.byValueKey('studio-shell'),
      timeout: const Duration(minutes: 2),
    ),
    'Studio shell',
    const Duration(minutes: 2),
  );
  await _driverCommand(
    driver.tap(find.byValueKey('sidebar-open-project')),
    'open project tap',
  );
  await _driverCommand(
    driver.waitFor(find.byValueKey('project-path-dialog')),
    'project path dialog',
  );
  await _driverCommand(
    driver.tap(find.byValueKey('project-path-input')),
    'project path input tap',
  );
  await _driverCommand(driver.enterText(workspace), 'project path entry');
  await _driverCommand(
    driver.waitUntilNoTransientCallbacks(timeout: const Duration(seconds: 5)),
    'project path input settled',
    const Duration(seconds: 10),
  );
  await _driverCommand(
    driver.sendTextInputAction(
      TextInputAction.done,
      timeout: const Duration(seconds: 5),
    ),
    'project path submit action',
    const Duration(seconds: 10),
  );
  await _driverCommand(
    driver.waitFor(
      find.byValueKey('composer-input'),
      timeout: const Duration(minutes: 1),
    ),
    'composer after project open',
    const Duration(minutes: 1),
  );
}

Future<void> _selectTaskMode(FlutterDriver driver) async {
  await _driverCommand(
    driver.tap(find.byValueKey('session-mode-selector')),
    'session mode selector tap',
  );
  await _driverCommand(
    driver.waitFor(find.byValueKey('session-mode-task')),
    'Task mode option',
  );
  await _driverCommand(
    driver.tap(find.byValueKey('session-mode-task')),
    'Task mode option tap',
  );
  await _waitForSnapshot(driver, 'Task mode projection', (snapshot) {
    final workspace = snapshot['workspace'];
    return workspace is Map<String, dynamic> &&
        workspace['threadMode'] == 'task';
  });
}

Future<void> _submitPrompt(FlutterDriver driver, String prompt) async {
  await _driverCommand(
    driver.tap(find.byValueKey('composer-input')),
    'composer input tap',
  );
  await _driverCommand(driver.enterText(prompt.trim()), 'prompt entry');
  await _driverCommand(
    driver.tap(find.byValueKey('composer-submit')),
    'prompt submit',
  );
}

Future<Map<String, dynamic>> _waitForTaskCompletion(
  FlutterDriver driver,
  File snapshots, {
  required Duration timeout,
  required Duration stallTimeout,
}) async {
  final deadline = DateTime.now().add(timeout);
  var lastProgressAt = DateTime.now();
  String? lastProgress;
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await _snapshot(driver, snapshots);
    final task = snapshot['task'];
    if (task is Map<String, dynamic>) {
      final phase = task['phase'] as String? ?? '';
      if (phase == 'completed') {
        return snapshot;
      }
      if (const {'blocked', 'failed', 'cancelled'}.contains(phase)) {
        throw StateError(
          'Task entered terminal failure phase $phase: ${task['statusMessage']}',
        );
      }
      final progress = taskProgressFingerprint(snapshot);
      if (progress != lastProgress) {
        lastProgress = progress;
        lastProgressAt = DateTime.now();
      } else if (DateTime.now().difference(lastProgressAt) > stallTimeout) {
        throw StateError('Task made no observable progress for $stallTimeout');
      }
    }
    await Future<void>.delayed(const Duration(seconds: 1));
  }
  throw TimeoutException('Task did not complete within $timeout');
}

Future<Map<String, dynamic>> _snapshot(
  FlutterDriver driver,
  File output,
) async {
  final snapshot = await _requestSnapshot(driver);
  final record = {
    'capturedAt': DateTime.now().toUtc().toIso8601String(),
    ...snapshot,
  };
  await output.writeAsString(
    '${jsonEncode(record)}\n',
    mode: FileMode.append,
    flush: true,
  );
  return snapshot;
}

Future<Map<String, dynamic>> _requestSnapshot(FlutterDriver driver) async {
  final raw = await _driverCommand(
    driver.requestData('snapshot', timeout: const Duration(seconds: 15)),
    'snapshot request',
    const Duration(seconds: 15),
  );
  return jsonDecode(raw) as Map<String, dynamic>;
}

Future<Map<String, dynamic>> _waitForSnapshot(
  FlutterDriver driver,
  String description,
  bool Function(Map<String, dynamic> snapshot) predicate, {
  Duration timeout = const Duration(seconds: 30),
  File? output,
}) async {
  final deadline = DateTime.now().add(timeout);
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = output == null
        ? await _requestSnapshot(driver)
        : await _snapshot(driver, output);
    if (predicate(snapshot)) {
      return snapshot;
    }
    await Future<void>.delayed(const Duration(milliseconds: 200));
  }
  throw TimeoutException('Flutter Driver snapshot timed out: $description');
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

class _DriverOptions {
  const _DriverOptions({
    required this.vmServiceUrl,
    required this.workspace,
    required this.promptFile,
    required this.snapshotOutput,
    required this.planTimeout,
    required this.taskTimeout,
    required this.stallTimeout,
  });

  final String vmServiceUrl;
  final String workspace;
  final String promptFile;
  final String snapshotOutput;
  final Duration planTimeout;
  final Duration taskTimeout;
  final Duration stallTimeout;

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

    int seconds(String name, int fallback) =>
        int.tryParse(values[name]?.lastOrNull ?? '') ?? fallback;

    return _DriverOptions(
      vmServiceUrl: required('vm-service-url'),
      workspace: required('workspace'),
      promptFile: required('prompt-file'),
      snapshotOutput: required('snapshot-output'),
      planTimeout: Duration(seconds: seconds('plan-timeout-seconds', 300)),
      taskTimeout: Duration(seconds: seconds('task-timeout-seconds', 3600)),
      stallTimeout: Duration(seconds: seconds('stall-timeout-seconds', 300)),
    );
  }
}
