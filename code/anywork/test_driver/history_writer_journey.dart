import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';
// Public snapshot helpers shared with the realtime journey: this journey reads
// the same Driver projection (workspace/timeline/conversationActivity) and must
// not fork a second reading of it.
import 'realtime_journey.dart';

/// The exact final answer the history-lock fixture streams.
///
/// It matches `GUI_HISTORY_LOCK_ANSWER` in the fixture crate. Declared here as
/// the expected value so the durable content check is an equality, not a
/// substring guess.
const _answer = 'history lock answer complete';

/// The user prompt that starts the turn observed while the writer is locked. It
/// matches `GUI_HISTORY_LOCK_PROMPT` in the fixture crate.
const _lockPrompt = 'Local GUI history lock fixture';

/// The priming prompt; it matches `GUI_HISTORY_LOCK_PRIME_PROMPT`.
const _primePrompt = 'Local GUI history lock prime';

/// Stage markers the coordinator reads while it owns the history write lock.
const _stageReady = 'history_lock_ready';
const _stageTerminal = 'history_lock_terminal_while_locked';
const _stageComplete = 'history_lock_complete';

/// Marker files the coordinator writes when it acquires/releases the lock.
const _acquiredMarker = 'history-lock-acquired';
const _releasedMarker = 'history-lock-released';

const _terminalTurnStatuses = <String>{
  'completed',
  'failed',
  'cancelled',
  'budgetLimited',
};

/// Live pointer-free progress token for the current turn.
///
/// Grows whenever assistant text is appended or a row is added, so a later
/// observation is provably a *new* update and never a re-read of the same
/// snapshot. Evidence only: it is never a performance measurement.
int _streamingRevision(Map<String, dynamic> snapshot) {
  var revision = 0;
  final delivery = contentDelivery(snapshot);
  if (delivery != null && delivery['enabled'] == true) {
    final patches = delivery['patches'];
    final changes = delivery['contentChanges'];
    if (patches is int) revision += patches;
    if (changes is int) revision += changes;
  }
  final rows = timelineRows(snapshot);
  revision += rows.length;
  for (final row in rows) {
    final text = row['text'];
    if (text is String) revision += text.length;
  }
  return revision;
}

Future<void> main(List<String> args) async {
  if (args.length != 4) {
    stderr.writeln(
      'usage: history_writer_journey.dart VM_URL PROJECT_DIR OUTPUT_DIR COORD_DIR',
    );
    exitCode = 64;
    return;
  }
  final output = Directory(args[2]);
  final coord = Directory(args[3]);
  final FlutterDriverSession driver;
  try {
    driver = await FlutterDriverSession.connect(vmServiceUrl: args[0]);
  } catch (error, stackTrace) {
    await _recordConnectFailure(coord, output, error);
    // Never returns: the journey below can then rely on `driver` being set.
    Error.throwWithStackTrace(error, stackTrace);
  }
  final journey = HistoryLockJourney(
    driver: driver,
    project: args[1],
    output: output,
    coord: coord,
  );
  Object? failure;
  StackTrace? failureStack;
  try {
    await journey.run();
  } catch (error, stackTrace) {
    failure = error;
    failureStack = stackTrace;
    // Capture the failing observation before shutdown; the original error is
    // always preserved.
    await journey.captureFailure();
  }
  Object? shutdownFailure;
  try {
    await journey.shutdown();
  } catch (error) {
    shutdownFailure = error;
    journey.shutdownError ??= '$error';
  }
  if (failure == null) {
    if (shutdownFailure != null || journey.shutdownError != null) {
      journey.stage = 'shutdown_failed';
    } else if (journey.failed) {
      journey.stage = 'checks_failed';
    } else {
      journey.stage = 'complete';
    }
  }
  await journey.writeSummary(
    failure: failure,
    shutdownFailure: shutdownFailure,
  );
  await journey.mark(journey.stage);
  if (failure != null) {
    Error.throwWithStackTrace(failure, failureStack!);
  }
  journey.raiseIfFailed(shutdownFailure);
}

/// Records a stage marker and a minimal summary when the Driver cannot connect.
Future<void> _recordConnectFailure(
  Directory coord,
  Directory output,
  Object error,
) async {
  try {
    await File('${coord.path}/history-lock-stage')
        .writeAsString('connect_failed');
    final summary = <String, Object?>{
      'scenario': 'history-lock',
      'verdict': 'pending',
      'status': 'failed',
      'stage': 'connect_failed',
      'error': '$error',
      'shutdownError': null,
      'failedChecks': const <String>[],
      'checks': const <String, Object?>{},
      'timings': const <String, Object?>{},
      'counts': const <String, Object?>{},
      'observations': const <String, Object?>{},
      'completedAt': DateTime.now().toUtc().toIso8601String(),
      'pendingEvidence': const <String>[],
    };
    await File(
      '${output.path}/history-lock-summary.json',
    ).writeAsString('${const JsonEncoder.withIndent('  ').convert(summary)}\n');
  } catch (_) {
    // The coordinator still records the exit status and the sanitized Driver log.
  }
}

/// Acceptance journey for a paused history writer.
///
/// While the coordinator holds an exclusive external write transaction on the
/// real per-session `history.sqlite`, this journey proves the GUI's *live*
/// content keeps growing and the current activity stays visible, that the turn
/// reaches its terminal state without a durable commit, and that once the lock
/// is released the durable history catches up with the answer exactly once.
///
/// It never fakes a fault: if the pause only ever produced ordinary SQLite
/// blocking, the summary says so and the typed-fault/retry variant stays pending
/// the pending runtime bridge (recorded by the coordinator, not guessed here).
class HistoryLockJourney {
  HistoryLockJourney({
    required this.driver,
    required this.project,
    required this.output,
    required this.coord,
  });

  final FlutterDriverSession driver;
  final String project;
  final Directory output;
  final Directory coord;
  final Map<String, Object?> checks = <String, Object?>{};
  final Map<String, Object?> timings = <String, Object?>{};
  final Map<String, Object?> counts = <String, Object?>{};
  final Map<String, Object?> observations = <String, Object?>{};
  String stage = 'created';
  String? shutdownError;

  /// Observations that could not be asserted yet, kept explicit so a partial
  /// run is never reported as a pass.
  final List<String> pendingEvidence = <String>[
    'scope: this history-lock scenario only holds the real write lock for its ordinary window, '
        'which the runtime absorbs as normal SQLite blocking — it does not cross the '
        'retryable-conflict window, so no hard typed history fault is triggered here. The typed '
        'writeFailed fault, its generation and the explicit retry/resume are exercised by the '
        'separate history-fault scenario (typed storageRecovery is now exposed by the integrated '
        'Driver build)',
    'the coordinator holds the lock, so the pause duration and the raw lock outcomes live in '
        'history-lock-lock.json rather than the Driver wall clock alone',
  ];

  File get stageFile => File('${coord.path}/history-lock-stage');

  bool get failed => checks.values.any((value) => value != true);

  Future<void> run() async {
    await mark('connected');
    await _openProject();
    await observe('project_opened');

    // 1. Prime a real turn first, so the per-session history library really
    // exists before the coordinator locks it. A missing library would otherwise
    // make the lock meaningless.
    final prime = await _submitAndSettle(_primePrompt, 'prime');
    final primeTurn = turnId(prime);
    checks['primingTurnDurable'] = primeTurn != null;
    counts['primeTurnId'] = primeTurn;
    await shot('primed');
    await mark(_stageReady);

    // 2. Wait for the coordinator's real exclusive write lock.
    await _awaitMarker(
      _acquiredMarker,
      'history_lock_acquired',
      const Duration(seconds: 180),
    );
    final lockedAt = DateTime.now().millisecondsSinceEpoch;

    // 3. Submit the locked turn and prove the live content really grows while
    // the durable writer cannot commit.
    await _submit(_lockPrompt);
    final running = await waitFor(
      (snapshot) =>
          turnId(snapshot) != primeTurn &&
          isBusy(snapshot) &&
          timelineRows(snapshot).isNotEmpty,
      'history_lock_turn_running',
      timeout: const Duration(seconds: 90),
      abort: threadFaulted,
    );
    final turn = turnId(running)!;
    counts['lockedTurnId'] = turn;
    timings['lockedSubmittedAt'] = lockedAt;
    final samples = <Map<String, Object?>>[];
    var revision = _streamingRevision(running);
    var growth = 0;
    for (var step = 0; step < 8 && growth < 3; step++) {
      final next = await waitFor(
        (snapshot) =>
            turnId(snapshot) == turn && _streamingRevision(snapshot) > revision,
        'history_lock_live_increment',
        timeout: const Duration(seconds: 45),
        abort: turnFailed,
      );
      revision = _streamingRevision(next);
      growth += 1;
      final activity = conversationActivity(next);
      samples.add(<String, Object?>{
        'step': growth,
        'revision': revision,
        'rows': timelineRows(next).length,
        'busy': isBusy(next),
        'activityIdentity': activity?['identity'],
        'activityKind': activity?['kind'],
      });
    }
    checks['liveContentGrowsWhileWriterLocked'] = growth >= 3;
    counts['liveGrowthSamples'] = samples.length;
    observations['liveGrowth'] = samples;
    final liveSnapshot = await snapshot();
    // The live activity is always observable as a busy live Turn; the typed
    // activity identity is only asserted when the integrated Driver build
    // exposes it, and is pending on a baseline build instead of being faked.
    final activity = conversationActivity(liveSnapshot);
    if (liveSnapshot.containsKey('conversationActivity')) {
      checks['activityVisibleWhileWriterLocked'] =
          activity != null && activity['identity'] is String;
    } else {
      checks['activityVisibleWhileWriterLocked'] = isBusy(liveSnapshot);
      pendingEvidence.add(
        'activity bar: the snapshot exposes no conversationActivity field, so the typed '
        'current-activity identity is pending an integrated Driver build; while the writer '
        'was locked the live activity is evidenced by the busy live Turn and the '
        'history-lock-locked-live screenshot',
      );
    }
    // No tool may start on the locked turn: the fault-safe boundary forbids new
    // model/tool work while persistence cannot commit.
    checks['noToolStartedWhileWriterLocked'] = timelineRows(liveSnapshot)
        .where((row) => row['type'] == 'toolGroup')
        .isEmpty;
    await shot('locked-live');

    // 4. The turn must reach its terminal state while the writer is still
    // locked, so the blocked commit is a real one, not a stream that simply had
    // not finished yet.
    final terminal = await waitFor(
      (snapshot) =>
          turnId(snapshot) == turn &&
          _terminalTurnStatuses.contains(turnStatus(snapshot)),
      'history_lock_terminal_while_locked',
      timeout: const Duration(seconds: 180),
      abort: threadFaulted,
    );
    checks['turnTerminalWhileWriterLocked'] = true;
    counts['lockedTerminalStatus'] = turnStatus(terminal);
    timings['lockedTerminalAt'] = DateTime.now().millisecondsSinceEpoch;
    await shot('locked-terminal');
    await _writeJson(
      '${coord.path}/history-lock-observed.json',
      <String, Object?>{
        'threadId': workspaceOf(terminal)?['threadId'],
        'primeTurnId': primeTurn,
        'turnId': turn,
        'terminalStatus': turnStatus(terminal),
        'liveRevision': revision,
        'liveGrowthSamples': samples.length,
        'activityIdentity': conversationActivity(terminal)?['identity'],
        'answerLengthBeforeRelease': [
          for (final row in timelineRows(terminal))
            if (row['type'] == 'finalAnswer')
              (row['text'] as String? ?? '').length,
        ],
      },
    );
    await mark(_stageTerminal);

    // 5. Wait for the coordinator to release the real write lock.
    await _awaitMarker(
      _releasedMarker,
      'history_lock_released',
      const Duration(seconds: 180),
    );

    // 6. The durable save must catch up: settled projection, the answer present
    // exactly once and byte-identical, and no duplicated or lost content.
    final settled = await waitFor(
      isDurableSettled,
      'history_lock_durable_settled',
      timeout: const Duration(seconds: 180),
      abort: threadFaulted,
    );
    checks['durableSettledAfterRelease'] = true;
    final matches = answerMatchCount(settled, _answer);
    checks['answerPresentExactlyOnce'] = matches == 1;
    counts['answerMatches'] = matches;
    // Counted, not just "contains": the priming turn is also a final answer, so
    // only the locked answer may match exactly once. Any second copy is a
    // duplicate, not a pass.
    final exactRows = timelineRows(settled)
        .where(
          (row) =>
              row['type'] == 'finalAnswer' &&
              (row['text'] as String? ?? '').trim() == _answer,
        )
        .length;
    checks['answerTextExactAfterRelease'] = exactRows == 1;
    checks['noDuplicatedAnswer'] = exactRows == 1;
    counts['exactAnswerRows'] = exactRows;
    observations['settled'] = summarize(settled);
    await shot('settled');
    await mark(_stageComplete);
  }

  Future<void> _openProject() async {
    await driver.waitFor(
      find.byValueKey('sidebar-open-project'),
      timeout: const Duration(seconds: 60),
    );
    await _tap('sidebar-open-project');
    await _tap('add-project-local');
    await driver.waitFor(
      find.byValueKey('add-project-continue-ready'),
      timeout: const Duration(seconds: 30),
    );
    await _tap('add-project-continue-ready');
    await driver.waitFor(
      find.byValueKey('project-path-input'),
      timeout: const Duration(seconds: 30),
    );
    await _tap('project-path-input');
    await driver.enterText(project);
    await driver.waitFor(
      find.byValueKey('project-path-submit'),
      timeout: const Duration(seconds: 30),
    );
    await _tap('project-path-submit');
    await driver.waitFor(
      find.byValueKey('composer-input'),
      timeout: const Duration(seconds: 60),
    );
  }

  Future<void> _submit(String prompt) async {
    await driver.waitFor(
      find.byValueKey('composer-input'),
      timeout: const Duration(seconds: 30),
    );
    await _tap('composer-input');
    await driver.enterText(prompt);
    await driver.waitFor(
      find.byValueKey('composer-submit'),
      timeout: const Duration(seconds: 30),
    );
    await _tap('composer-submit');
  }

  /// Submits [prompt] and waits for the durable settled projection.
  Future<Map<String, dynamic>> _submitAndSettle(
    String prompt,
    String label,
  ) async {
    final before = turnId(await snapshot());
    await _submit(prompt);
    final terminal = await waitFor(
      (snapshot) =>
          turnId(snapshot) != before &&
          _terminalTurnStatuses.contains(turnStatus(snapshot)),
      '${label}_terminal',
      timeout: const Duration(seconds: 120),
      abort: turnFailed,
    );
    return waitFor(
      (snapshot) =>
          turnId(snapshot) == turnId(terminal) && isDurableSettled(snapshot),
      '${label}_settled',
      timeout: const Duration(seconds: 120),
      abort: turnFailed,
    );
  }

  Future<void> _tap(String key) async {
    await driver.rawTap(
      find.byValueKey(key),
      timeout: const Duration(seconds: 30),
    );
  }

  /// Waits, bounded, for a marker file the coordinator writes.
  Future<void> _awaitMarker(String name, String label, Duration timeout) async {
    final deadline = DateTime.now().add(timeout);
    final file = File('${coord.path}/$name');
    while (DateTime.now().isBefore(deadline)) {
      if (await file.exists()) return;
      await Future<void>.delayed(const Duration(milliseconds: 200));
    }
    throw StateError(
      'timed out waiting for the coordinator marker $name ($label)',
    );
  }

  Future<void> mark(String value) async {
    stage = value;
    await stageFile.writeAsString(value);
  }

  Future<Map<String, dynamic>> snapshot() => driver.readSnapshot();

  Future<Map<String, dynamic>> waitFor(
    bool Function(Map<String, dynamic>) predicate,
    String label, {
    Duration timeout = const Duration(seconds: 60),
    bool Function(Map<String, dynamic>)? abort,
  }) async {
    final deadline = DateTime.now().add(timeout);
    Map<String, dynamic>? last;
    while (DateTime.now().isBefore(deadline)) {
      final current = await snapshot();
      last = current;
      if (predicate(current)) return current;
      // A terminal failure makes the awaited state unreachable; report the real
      // observation immediately instead of burning the whole timeout.
      if (abort != null && abort(current)) {
        throw StateError('$label aborted: ${summarize(current)}');
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    throw StateError(
      'timed out waiting for $label: ${last == null ? const {} : summarize(last)}',
    );
  }

  Future<void> observe(String name, [Map<String, dynamic>? current]) async {
    final snapshot = current ?? await this.snapshot();
    observations[name] = summarize(snapshot);
  }

  Future<void> shot(String name) async {
    await File('${output.path}/history-lock-$name.png')
        .writeAsBytes(await driver.screenshot());
  }

  Future<void> _writeJson(String path, Object? data) async {
    try {
      await File(
        path,
      ).writeAsString('${const JsonEncoder.withIndent('  ').convert(data)}\n');
    } on Object {
      // Evidence writes must never mask the observation they document.
    }
  }

  /// Saves the last snapshot and a screenshot for a failed journey.
  Future<void> captureFailure() async {
    try {
      await _writeJson(
        '${output.path}/history-lock-failure-snapshot.json',
        await driver.readSnapshot().timeout(const Duration(seconds: 10)),
      );
    } on Object {
      // Ignored: the summary still carries the original error and stage.
    }
    try {
      final png = await driver.screenshot().timeout(
        const Duration(seconds: 20),
      );
      await File('${output.path}/history-lock-failure.png').writeAsBytes(png);
    } on Object {
      // Ignored: see above.
    }
  }

  Future<void> shutdown() async {
    try {
      final result = jsonDecode(
        await driver
            .requestData('shutdown', timeout: const Duration(seconds: 60))
            .timeout(const Duration(seconds: 65)),
      );
      if (result is! Map || result['shutdown'] != 'completed') {
        throw StateError('native GUI shutdown did not report completion');
      }
    } finally {
      try {
        await driver.close().timeout(const Duration(seconds: 5));
      } catch (_) {
        // Native shutdown can close the observation connection first.
      }
    }
  }

  Future<void> writeSummary({
    required Object? failure,
    required Object? shutdownFailure,
  }) async {
    final failedChecks = checks.entries
        .where((entry) => entry.value != true)
        .map((entry) => entry.key)
        .toList(growable: false);
    final summary = <String, Object?>{
      'scenario': 'history-lock',
      'verdict': 'pending',
      'status': failure == null && failedChecks.isEmpty ? 'complete' : 'failed',
      'stage': stage,
      'error': failure?.toString(),
      'shutdownError': shutdownFailure?.toString() ?? shutdownError,
      'failedChecks': failedChecks,
      'checks': checks,
      'timings': timings,
      'counts': counts,
      'observations': observations,
      'completedAt': DateTime.now().toUtc().toIso8601String(),
      'pendingEvidence': pendingEvidence,
    };
    await File(
      '${output.path}/history-lock-summary.json',
    ).writeAsString('${const JsonEncoder.withIndent('  ').convert(summary)}\n');
  }

  void raiseIfFailed(Object? shutdownFailure) {
    final failedChecks = checks.entries
        .where((entry) => entry.value != true)
        .map((entry) => entry.key)
        .toList(growable: false);
    if (failedChecks.isNotEmpty) {
      throw StateError(
        'history-lock checks failed: ${failedChecks.join(', ')}',
      );
    }
    if (shutdownFailure != null || shutdownError != null) {
      throw StateError(
        'history-lock native GUI shutdown failed: ${shutdownFailure ?? shutdownError}',
      );
    }
  }
}
