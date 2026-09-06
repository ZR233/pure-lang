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
      onReconnect: (event) => _appendSnapshot(snapshots, event.toJson()),
    );
    stdout.writeln(jsonEncode({'event': 'driverReady', 'health': 'ok'}));
    await _command(
      session.waitFor(
        find.byValueKey('studio-shell'),
        timeout: const Duration(minutes: 2),
      ),
      'Studio shell',
      const Duration(minutes: 2),
    );

    await _resolveToolApproval(session, snapshots);
    await _resolveUserInput(session, snapshots);
    await _submitCompletedTurn(session, snapshots, options.promptFile);

    await session.requestData('prepare-persistence-failure-demo');
    await _waitForSnapshot(session, snapshots, 'save failure banner', (
      snapshot,
    ) {
      return (snapshot['persistence'] as Map?)?['kind'] == 'degraded';
    });
    await _submitCompletedTurn(
      session,
      snapshots,
      options.promptFile,
      expectedStatus: 'failed',
    );
    await _submitCompletedTurn(session, snapshots, options.promptFile);
    final finalSnapshot = await _snapshot(session, snapshots);
    if ((finalSnapshot['persistence'] as Map?)?['kind'] != 'degraded') {
      throw StateError('saving must remain degraded throughout both turns');
    }
    await File(options.screenshotOutput)
        .writeAsBytes(await session.screenshot(), flush: true);
    stdout.writeln(
      jsonEncode({
        'result': 'completed',
        'screenshot': options.screenshotOutput,
        ...finalSnapshot,
      }),
    );
  } catch (error, stackTrace) {
    stderr.writeln('Session-state Driver failed: $error');
    stderr.writeln(stackTrace);
    if (session != null) {
      try {
        await _snapshot(session, snapshots);
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
        // The PowerShell harness owns the GUI process tree.
      }
    }
  }
}

Future<void> _resolveToolApproval(
  FlutterDriverSession session,
  File snapshots,
) async {
  final snapshot = await _waitForInteraction(
    session,
    snapshots,
    id: 'driver-tool',
    kind: 'toolApproval',
  );
  _expectLockedCompletedOrigin(snapshot);
  await _command(
    session.tap(find.byValueKey('tool-approve')),
    'approve deterministic tool',
  );
}

Future<void> _resolveUserInput(
  FlutterDriverSession session,
  File snapshots,
) async {
  final snapshot = await _waitForInteraction(
    session,
    snapshots,
    id: 'driver-input',
    kind: 'userInput',
  );
  _expectLockedCompletedOrigin(snapshot);
  final input = find.byValueKey('user-input-first-text');
  await _command(session.tap(input), 'focus deterministic user input');
  await _command(session.enterText('Continue'), 'enter deterministic answer');
  final answer = await _command(
    session.getText(input),
    'read back deterministic answer',
  );
  if (answer != 'Continue') {
    throw StateError('user input read-back mismatch: $answer');
  }
  await _command(
    session.tap(find.byValueKey('user-input-submit')),
    'submit deterministic answer',
  );
}

Future<void> _submitCompletedTurn(
  FlutterDriverSession session,
  File snapshots,
  String promptFile, {
  String expectedStatus = 'completed',
}) async {
  await _waitForSnapshot(session, snapshots, 'composer restoration', (
    snapshot,
  ) {
    final workspace = _workspace(snapshot);
    final composer = workspace?['composer'];
    return workspace?['activeInteraction'] == null &&
        composer is Map<String, dynamic> &&
        composer['lockedByInteraction'] == false;
  });
  final input = find.byValueKey('composer-input');
  await _command(
    session.waitFor(input, timeout: const Duration(seconds: 30)),
    'restored composer',
  );
  final prompt = (await File(promptFile).readAsString()).trim();
  if (prompt.isEmpty) throw StateError('normal-turn prompt fixture is empty');
  await _command(session.tap(input), 'focus composer');
  await _command(session.enterText(prompt), 'enter normal-turn prompt');
  final entered = await _command(session.getText(input), 'read back prompt');
  if (entered != prompt) {
    throw StateError(
      'prompt read-back mismatch: expected ${prompt.length}, received ${entered.length}',
    );
  }
  // The submit button is enabled only after the draft change is rebuilt into
  // the widget tree; on slow or frame-throttled CI runners the tap could
  // otherwise land on a still-disabled button and silently do nothing.
  await _command(
    session.waitForNoPendingFrame(timeout: const Duration(seconds: 15)),
    'composer rebuild after prompt entry',
    const Duration(seconds: 20),
  );
  await _command(
    session.tap(find.byValueKey('composer-submit')),
    'submit normal turn',
  );
  final running = await _waitForSnapshot(
    session,
    snapshots,
    'normal turn start',
    (snapshot) {
      final turn = _workspace(snapshot)?['turn'];
      return turn is Map<String, dynamic> && turn['status'] == 'running';
    },
  );
  final turnId = (_workspace(running)!['turn'] as Map<String, dynamic>)['id'];
  await _waitForSnapshot(session, snapshots, 'normal turn completion', (
    snapshot,
  ) {
    final workspace = _workspace(snapshot);
    final lastTurn = workspace?['lastTurn'];
    return workspace?['activeInteraction'] == null &&
        workspace?['isBusy'] == false &&
        lastTurn is Map<String, dynamic> &&
        lastTurn['id'] == turnId &&
        lastTurn['status'] == expectedStatus;
  }, timeout: const Duration(seconds: 45));
}

Future<Map<String, dynamic>> _waitForInteraction(
  FlutterDriverSession session,
  File snapshots, {
  required String id,
  required String kind,
}) {
  return _waitForSnapshot(session, snapshots, '$kind interaction $id', (
    snapshot,
  ) {
    final interaction = _workspace(snapshot)?['activeInteraction'];
    return interaction is Map<String, dynamic> &&
        interaction['id'] == id &&
        interaction['kind'] == kind;
  });
}

void _expectLockedCompletedOrigin(Map<String, dynamic> snapshot) {
  final workspace = _workspace(snapshot);
  final composer = workspace?['composer'];
  final interaction = workspace?['activeInteraction'];
  final lastTurn = workspace?['lastTurn'];
  if (composer is! Map<String, dynamic> ||
      composer['lockedByInteraction'] != true ||
      interaction is! Map<String, dynamic> ||
      lastTurn is! Map<String, dynamic> ||
      lastTurn['id'] != interaction['turnId'] ||
      lastTurn['status'] != 'completed') {
    throw StateError(
      'pending interaction must lock Composer while its origin turn is completed',
    );
  }
}

Map<String, dynamic>? _workspace(Map<String, dynamic> snapshot) {
  final workspace = snapshot['workspace'];
  return workspace is Map<String, dynamic> ? workspace : null;
}

Future<Map<String, dynamic>> _waitForSnapshot(
  FlutterDriverSession session,
  File snapshots,
  String description,
  bool Function(Map<String, dynamic> snapshot) predicate, {
  Duration timeout = const Duration(seconds: 30),
}) async {
  final deadline = DateTime.now().add(timeout);
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await _snapshot(session, snapshots);
    if (predicate(snapshot)) return snapshot;
    await Future<void>.delayed(const Duration(milliseconds: 150));
  }
  throw TimeoutException('Flutter Driver snapshot timed out: $description');
}

Future<Map<String, dynamic>> _snapshot(
  FlutterDriverSession session,
  File snapshots,
) async {
  final snapshot = await session.readSnapshot();
  await _appendSnapshot(snapshots, {
    'kind': 'snapshot',
    'capturedAt': DateTime.now().toUtc().toIso8601String(),
    ...snapshot,
  });
  return snapshot;
}

Future<void> _appendSnapshot(File output, Map<String, Object?> record) {
  return output.writeAsString(
    '${jsonEncode(record)}\n',
    mode: FileMode.append,
    flush: true,
  );
}

Future<T> _command<T>(
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
    required this.promptFile,
    required this.snapshotOutput,
    required this.screenshotOutput,
  });

  final String vmServiceUrl;
  final String promptFile;
  final String snapshotOutput;
  final String screenshotOutput;

  static _DriverOptions parse(List<String> arguments) {
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

    return _DriverOptions(
      vmServiceUrl: required('vm-service-url'),
      promptFile: required('prompt-file'),
      snapshotOutput: required('snapshot-output'),
      screenshotOutput: required('screenshot-output'),
    );
  }
}
