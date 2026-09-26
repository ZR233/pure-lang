import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';
// Public snapshot helpers shared with the realtime/history-lock journeys: this
// journey reads the same Driver projection and must not fork a second reading.
import 'realtime_journey.dart';

/// The exact final answer the history-fault fixture streams.
///
/// It matches `GUI_HISTORY_FAULT_ANSWER` in the fixture crate. Declared here as
/// the expected value so the durable content check is an equality, not a
/// substring guess.
const _answer = 'history fault answer complete';

/// The committed output of the safe `exec` the fault turn schedules.
///
/// It matches `GUI_HISTORY_FAULT_TOOL_MARKER`; the post-resume step can only be
/// satisfied by that call identity carrying this exact output.
const _toolMarker = 'history-fault-tool-marker';

/// Fixed call identity of the safe exec, matching `GUI_HISTORY_FAULT_TOOL_CALL_ID`.
const _toolCallId = 'history-fault-call';

/// The prompt that starts the turn observed while the writer is locked.
const _faultPrompt = 'Local GUI history fault fixture';

/// The priming prompt; it matches `GUI_HISTORY_FAULT_PRIME_PROMPT`.
const _primePrompt = 'Local GUI history fault prime';

/// Stage markers the coordinator reads while it owns the history write lock.
const _stageReady = 'history_fault_ready';
const _stageSafeBoundary = 'history_fault_safe_boundary';
const _stageRetryReady = 'history_fault_retry_ready';
const _stageComplete = 'history_fault_complete';

/// Marker files the coordinator writes when it acquires/releases the lock.
const _acquiredMarker = 'history-fault-acquired';
const _releasedMarker = 'history-fault-released';

const _terminalTurnStatuses = <String>{
  'completed',
  'failed',
  'cancelled',
  'budgetLimited',
};

/// Statuses that prove a tool is genuinely executing, not merely scheduled.
const _liveToolStatuses = <String>{'started', 'streaming', 'running'};

/// Canonical tool states that prove a call is registered but has *not* begun.
///
/// The runtime admits a scheduled call as `queued`; `awaitingApproval` is the
/// other pre-start state. Live, terminal, `null` and unknown strings are all
/// excluded, so a completed or unprojected call can never be read as "not
/// started".
const _deferredToolStatuses = <String>{'queued', 'awaitingApproval'};

/// The typed history fault that a pause past the retryable window must surface.
const _expectedFault = 'writeFailed';

/// Whether [status] is an explicit pre-execution state (registered, not begun).
bool _isDeferredPreExecution(Object? status) =>
    status is String && _deferredToolStatuses.contains(status);

/// Whether [tool] already carries the committed safe-tool marker output.
bool _toolHasMarker(Map<String, dynamic>? tool) =>
    tool != null && (tool['result'] as String? ?? '').contains(_toolMarker);

/// Whether [tool] has genuinely begun or already produced the committed marker.
///
/// A millisecond echo can finish between polls, so the resume step accepts the
/// exact call either live or already `succeeded` with the marker; any other
/// identity or status is rejected.
bool _toolRanOrDelivered(Map<String, dynamic>? tool) =>
    tool != null &&
    (_liveToolStatuses.contains(tool['status']) ||
        (tool['status'] == 'succeeded' &&
            (tool['result'] as String? ?? '').contains(_toolMarker)));

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

/// Whether the integrated Driver build exposes the typed storage-recovery array.
bool _hasStorageRecovery(Map<String, dynamic> snapshot) =>
    snapshot.containsKey('storageRecovery');

/// The typed storage-recovery entry for [threadId], when the GUI exposes it.
Map<String, dynamic>? _storageRecovery(
  Map<String, dynamic> snapshot,
  String threadId,
) {
  final list = snapshot['storageRecovery'];
  if (list is! List) return null;
  for (final entry in list) {
    if (entry is Map && entry['threadId'] == threadId) {
      return entry.cast<String, dynamic>();
    }
  }
  return null;
}

/// The projected exec tool for [callId], when it has been scheduled at all.
Map<String, dynamic>? _execTool(Map<String, dynamic> snapshot, String callId) {
  for (final tool in execTools(snapshot)) {
    if (tool['callId'] == callId) return tool;
  }
  return null;
}

/// Final-answer rows committed for [turn], scoped by the turn id embedded in the
/// timeline row identities.
///
/// The timeline spans the whole session, so a session-wide `finalAnswer` count
/// includes the priming turn's answer. Scoping to the faulted turn keeps the
/// "no new work" checks about *this* turn without ignoring any answer: the
/// priming turn's legitimate answer is simply not part of the count.
int _answerCountForTurn(Map<String, dynamic> snapshot, String turn) =>
    timelineRows(snapshot)
        .where(
          (row) =>
              row['type'] == 'finalAnswer' &&
              (row['id'] as String? ?? '').contains(turn),
        )
        .length;

Future<void> main(List<String> args) async {
  if (args.length != 4) {
    stderr.writeln(
      'usage: history_fault_journey.dart VM_URL PROJECT_DIR OUTPUT_DIR COORD_DIR',
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
  final journey = HistoryFaultJourney(
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
    await File('${coord.path}/history-fault-stage')
        .writeAsString('connect_failed');
    final summary = <String, Object?>{
      'scenario': 'history-fault',
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
      '${output.path}/history-fault-summary.json',
    ).writeAsString('${const JsonEncoder.withIndent('  ').convert(summary)}\n');
  } catch (_) {
    // The coordinator still records the exit status and the sanitized Driver log.
  }
}

/// Acceptance journey for a typed history fault and its explicit retry/resume.
///
/// While the coordinator holds an exclusive external write transaction on the
/// real per-session `history.sqlite` past the runtime's retryable-conflict
/// window, this journey proves the GUI's *live* content keeps growing, that the
/// durable writer surfaces a typed `writeFailed` fault with a stable generation,
/// that the fault reaches its canonical safe boundary (continuation deferred:
/// `resumeRequired` with `canResume` false, the safe exec positively held in its
/// explicit pre-execution state with no committed marker, this turn with no
/// answer, no follow-up model request), and that only the explicit
/// retry-then-resume path ever runs the deferred tool and completes the answer
/// exactly once. The generation is proved stable across the deterministic
/// fault/boundary/release/retry snapshots rather than by waiting on a live
/// stream update.
///
/// The integrated GUI now projects a running tool's in-flight progress
/// (`output`/`progressBytes`/`itemRevision`); this fault journey does not depend
/// on it, because the deferred-tool proof is the typed pre-execution status
/// before resume and the committed `result` is the post-resume identity. A
/// baseline build without the typed `storageRecovery` array fails with the
/// precise gap instead of passing.
class HistoryFaultJourney {
  HistoryFaultJourney({
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

  /// Observations that could not be asserted, kept explicit so a partial run is
  /// never reported as a pass.
  final List<String> pendingEvidence = <String>[
    'typed tool in-flight progress now exists in the integrated GUI: a running '
        'tool projects `output`/`progressBytes`/`itemRevision` (see '
        'realtime_journey.dart), so in-flight stdout is observable. This fault '
        'journey does not depend on it — the deferred-tool proof is the typed '
        'pre-execution status before resume and the committed `result` after — so '
        'no in-flight field is required here (see history-fault-interface-needs.json).',
    'the coordinator holds the lock, so the raw lock outcome and the pause '
        'duration live in history-fault-lock.json rather than the Driver clock alone.',
  ];

  File get stageFile => File('${coord.path}/history-fault-stage');

  bool get failed => checks.values.any((value) => value != true);

  Future<void> run() async {
    await mark('connected');
    await _openProject();
    await observe('project_opened');

    // 1. Prime a real turn first, so the per-session history library really
    // exists before the coordinator locks it.
    final prime = await _submitAndSettle(_primePrompt, 'prime');
    final primeTurn = turnId(prime);
    checks['primingTurnDurable'] = primeTurn != null;
    counts['primeTurnId'] = primeTurn;
    await shot('primed');
    await mark(_stageReady);

    // 2. Wait for the coordinator's real exclusive write lock.
    await _awaitMarker(
      _acquiredMarker,
      'history_fault_acquired',
      const Duration(seconds: 300),
    );
    final threadId = workspaceOf(await snapshot())?['threadId'];
    if (threadId is! String) {
      throw StateError(
        'history-fault: the Driver snapshot exposes no threadId to target the '
        'typed recovery controls',
      );
    }
    if (threadId.isEmpty) {
      throw StateError(
        'history-fault: the Driver snapshot exposes an empty threadId, so the '
        'typed recovery controls cannot be addressed',
      );
    }
    counts['threadId'] = threadId;
    final lockedAt = DateTime.now().millisecondsSinceEpoch;

    // The typed recovery contract is required for this acceptance: without it the
    // fault, its generation and the retry/resume controls cannot be observed. A
    // baseline build records the precise gap and fails rather than passing.
    if (!_hasStorageRecovery(await snapshot())) {
      checks['typedRecoveryAvailable'] = false;
      pendingEvidence.add(
        'storageRecovery: this GUI build exposes no typed storage-recovery array, '
        'so the fault, its generation and the retry/resume controls cannot be '
        'observed. The acceptance is pending the integrated runtime bridge '
        '(see history-fault-interface-needs.json).',
      );
      await mark('history_fault_recovery_fields_pending');
      throw StateError(
        'history-fault: this GUI build exposes no typed storageRecovery array, so '
        'the typed retry/resume acceptance cannot run; see '
        'history-fault-interface-needs.json for the exact contract',
      );
    }
    checks['typedRecoveryAvailable'] = true;

    // 3. Submit the fault turn and prove the live content really grows while the
    // durable writer cannot commit.
    await _submit(_faultPrompt);
    final running = await waitFor(
      (snapshot) =>
          turnId(snapshot) != primeTurn &&
          isBusy(snapshot) &&
          timelineRows(snapshot).isNotEmpty,
      'history_fault_turn_running',
      timeout: const Duration(seconds: 90),
    );
    final turn = turnId(running)!;
    counts['faultTurnId'] = turn;
    timings['lockedSubmittedAt'] = lockedAt;
    final samples = <Map<String, Object?>>[];
    final generations = <Object?>[];
    var faultObserved = false;
    // Records the fault generation only from the fault episode onward: before
    // the fault the field is the un-faulted baseline (0), which is not part of
    // the generation the retry control is bound to.
    void recordGeneration(Map<String, dynamic> snapshot) {
      final recovery = _storageRecovery(snapshot, threadId);
      if (recovery == null) return;
      if (recovery['fault'] != null) faultObserved = true;
      final generation = recovery['faultGeneration'];
      if (faultObserved && generation != null) generations.add(generation);
    }

    var revision = _streamingRevision(running);
    var growth = 0;
    for (var step = 0; step < 8 && growth < 3; step++) {
      final next = await waitFor(
        (snapshot) =>
            turnId(snapshot) == turn && _streamingRevision(snapshot) > revision,
        'history_fault_live_increment',
        timeout: const Duration(seconds: 90),
      );
      revision = _streamingRevision(next);
      growth += 1;
      recordGeneration(next);
      final recovery = _storageRecovery(next, threadId);
      samples.add(<String, Object?>{
        'step': growth,
        'revision': revision,
        'rows': timelineRows(next).length,
        'busy': isBusy(next),
        'fault': recovery?['fault'],
        'faultGeneration': recovery?['faultGeneration'],
      });
    }
    checks['liveContentGrowsWhileWriterLocked'] = growth >= 3;
    counts['liveGrowthSamples'] = samples.length;
    observations['liveGrowth'] = samples;
    await shot('live-growth');

    // 4. The writer must surface a typed fault, not just ordinary blocking, and
    // the fault generation must stay stable across the fault, the safe boundary,
    // the release and the retry stages that follow.
    final faulted = await waitFor(
      (snapshot) {
        final recovery = _storageRecovery(snapshot, threadId);
        return recovery != null && recovery['fault'] != null;
      },
      'history_fault_typed',
      timeout: const Duration(seconds: 240),
    );
    final faultState = _storageRecovery(faulted, threadId)!;
    checks['typedFaultWhileWriterLocked'] = faultState['fault'] != null;
    checks['typedFaultIsWriteFailed'] = faultState['fault'] == _expectedFault;
    counts['fault'] = faultState['fault'];
    counts['faultGeneration'] = faultState['faultGeneration'];
    counts['lastError'] = faultState['lastError'];
    counts['resumeRequiredAtFault'] = faultState['resumeRequired'];
    counts['blocksContinuationAtFault'] = faultState['blocksContinuation'];
    counts['executionAtFault'] = faultState['execution'];
    recordGeneration(faulted);
    await shot('typed');
    await mark('history_fault_typed');

    // 5. The safe boundary: the fault turn scheduled the safe exec, and the fault
    // must hold it back — never started, no follow-up model request, and no answer
    // for *this* turn. Scheduling alone is a transient (the runtime has registered
    // the call before it reaches its next admission boundary), so the boundary is
    // the canonical latched state: continuation is deferred (`resumeRequired`)
    // with no resume yet verified (`canResume` false). A plain `queued` transient
    // is never accepted as the boundary.
    final scheduled = await waitFor(
      (snapshot) => _execTool(snapshot, _toolCallId) != null,
      'history_fault_safe_tool_scheduled',
      timeout: const Duration(seconds: 240),
    );
    recordGeneration(scheduled);
    final scheduledTool = _execTool(scheduled, _toolCallId)!;
    checks['safeToolScheduled'] = scheduledTool['callId'] == _toolCallId;
    counts['safeToolStatusAtScheduled'] = scheduledTool['status'];
    // Bounded wait for the canonical latched safe boundary. If the runtime never
    // reaches `resumeRequired && !canResume` the wait reports the real observation
    // and the check fails; the transient is never reported as the boundary. The
    // latch check is defaulted to false so a timeout is an explicit failed check
    // rather than an absent one.
    checks['faultLatchesContinuation'] = false;
    final boundary = await waitFor(
      (snapshot) {
        final state = _storageRecovery(snapshot, threadId);
        return state != null &&
            state['resumeRequired'] == true &&
            state['canResume'] == false;
      },
      'history_fault_safe_boundary_latched',
      timeout: const Duration(seconds: 180),
    );
    recordGeneration(boundary);
    // At the latched boundary the fault must be a real latch on continuation: the
    // tool has been scheduled, storage admission has been attempted, and the typed
    // execution phase reflects the storage pause.
    final boundaryState = _storageRecovery(boundary, threadId)!;
    checks['faultLatchesContinuation'] =
        boundaryState['resumeRequired'] == true &&
        boundaryState['blocksContinuation'] == true;
    counts['resumeRequiredAtBoundary'] = boundaryState['resumeRequired'];
    counts['canResumeAtBoundary'] = boundaryState['canResume'];
    counts['blocksContinuationAtBoundary'] =
        boundaryState['blocksContinuation'];
    counts['executionAtBoundary'] = boundaryState['execution'];
    // Positive proof the fault held the scheduled call back: at the latched
    // boundary it is in an explicit pre-execution state, not merely "not one of
    // the live statuses", and it carries no committed marker. A null, unknown or
    // terminal status can never satisfy this.
    final boundaryTool = _execTool(boundary, _toolCallId);
    checks['safeToolNotStartedAtSafetyBoundary'] =
        _isDeferredPreExecution(boundaryTool?['status']) &&
        !_toolHasMarker(boundaryTool);
    counts['safeToolStatusAtBoundary'] = boundaryTool?['status'];
    // The pause reason the UI renders comes from these typed facts; the banner is
    // the same blocked-continuation banner, never inferred from an error string.
    try {
      await driver.waitFor(
        find.byValueKey('persistence-state-banner'),
        timeout: const Duration(seconds: 60),
      );
      checks['pauseReasonVisible'] = true;
    } on Object {
      checks['pauseReasonVisible'] = false;
    }
    // The answer check is scoped to the faulted turn: the priming turn's
    // legitimate answer must not count, and no answer of *this* turn may exist.
    final answersAtBoundary = _answerCountForTurn(boundary, turn);
    checks['noCompletionBeforeResume'] =
        answersAtBoundary == 0 && answerMatchCount(boundary, _answer) == 0;
    counts['faultTurnAnswersAtBoundary'] = answersAtBoundary;
    counts['faultAnswerMatchesAtBoundary'] = answerMatchCount(
      boundary,
      _answer,
    );
    // The session-wide count proves the priming turn's answer is still present
    // and is deliberately *not* part of the fault-turn check, rather than being
    // ignored by weakening the assertion.
    counts['sessionAnswerRowsAtBoundary'] = answerCount(boundary);
    // No new model request is proved from the fixture side (a request issued
    // before the tool ran cannot satisfy the post-resume tool-output step and
    // would be rejected); here we record the exact pre-release observation.
    observations['safeBoundary'] = summarize(boundary);
    await shot('safe-boundary');
    await _writeJson(
      '${coord.path}/history-fault-observed.json',
      <String, Object?>{
        'threadId': threadId,
        'primeTurnId': primeTurn,
        'turnId': turn,
        'fault': faultState['fault'],
        'faultGeneration': faultState['faultGeneration'],
        'faultExecution': faultState['execution'],
        'faultLastError': faultState['lastError'],
        'safeToolCallId': _toolCallId,
        'safeToolStatus': boundaryTool?['status'],
        'safeToolMarker': _toolMarker,
        'resumeRequiredAtBoundary': boundaryState['resumeRequired'],
        'canResumeAtBoundary': boundaryState['canResume'],
        'liveRevision': revision,
        'liveGrowthSamples': samples.length,
        'faultTurnAnswersAtBoundary': answersAtBoundary,
        'rowTypesAtBoundary': [
          for (final row in timelineRows(boundary)) row['type'],
        ],
      },
    );
    await mark(_stageSafeBoundary);

    // 6. Wait for the coordinator to release the real write lock. It must not
    // recover the fault on its own: the hard latch stays set until the explicit
    // retry-save and continue.
    await _awaitMarker(
      _releasedMarker,
      'history_fault_released',
      const Duration(seconds: 300),
    );
    final released = await waitFor(
      (snapshot) {
        final recovery = _storageRecovery(snapshot, threadId);
        return recovery != null && recovery['resumeRequired'] == true;
      },
      'history_fault_fault_latched_after_release',
      timeout: const Duration(seconds: 120),
    );
    final releasedState = _storageRecovery(released, threadId)!;
    checks['faultLatchHeldAfterRelease'] =
        releasedState['resumeRequired'] == true;
    counts['canResumeBeforeRetry'] = releasedState['canResume'];
    recordGeneration(released);
    final releasedTool = _execTool(released, _toolCallId);
    // Even after the lock is gone the fault is only a latch: the same scheduled
    // call must still sit in its explicit pre-execution state with no marker,
    // which a null or terminal status can never satisfy.
    checks['noToolStartedAfterReleaseWithoutResume'] =
        _isDeferredPreExecution(releasedTool?['status']) &&
        !_toolHasMarker(releasedTool);
    final answersAfterRelease = _answerCountForTurn(released, turn);
    checks['noAnswerAfterReleaseWithoutResume'] =
        answersAfterRelease == 0 && answerMatchCount(released, _answer) == 0;
    counts['faultTurnAnswersAfterRelease'] = answersAfterRelease;
    counts['faultAnswerMatchesAfterRelease'] = answerMatchCount(
      released,
      _answer,
    );
    observations['released'] = summarize(released);
    await shot('released');
    await mark('history_fault_released');

    // 7. Explicit retry-save: wait for the backend-verified canResume while the
    // hard latch stays set, and prove no new work started in the meantime.
    await mark('history_fault_retrying');
    await driver.waitFor(
      find.byValueKey('history-retry-$threadId'),
      timeout: const Duration(seconds: 120),
    );
    await _tap('history-retry-$threadId');
    final retried = await waitFor(
      (snapshot) {
        final recovery = _storageRecovery(snapshot, threadId);
        return recovery != null &&
            recovery['canResume'] == true &&
            recovery['resumeRequired'] == true;
      },
      'history_fault_retry_ready',
      timeout: const Duration(seconds: 180),
    );
    final retriedState = _storageRecovery(retried, threadId)!;
    checks['retryReachesCanResume'] =
        retriedState['canResume'] == true &&
        retriedState['resumeRequired'] == true;
    recordGeneration(retried);
    final retriedTool = _execTool(retried, _toolCallId);
    // Retry-save must reach the backend-verified canResume without starting any
    // work: the deferred call is still in its explicit pre-execution state, it
    // carries no committed marker, and no answer exists yet.
    checks['noNewWorkDuringRetry'] =
        _isDeferredPreExecution(retriedTool?['status']) &&
        !_toolHasMarker(retriedTool) &&
        _answerCountForTurn(retried, turn) == 0 &&
        answerMatchCount(retried, _answer) == 0;
    counts['faultTurnAnswersAtRetry'] = _answerCountForTurn(retried, turn);
    counts['faultAnswerMatchesAtRetry'] = answerMatchCount(retried, _answer);
    counts['safeToolStatusAtRetry'] = retriedTool?['status'];
    counts['canResumeAfterRetry'] = retriedState['canResume'];
    // Deterministic generation stability: the same fault generation must span the
    // fault, the safe boundary, the release and the retry stages. Sampling those
    // real stage snapshots proves stability without waiting on a live stream
    // update, so a Driver error is never swallowed to claim it.
    final distinctGenerations = generations.toSet().toList();
    checks['faultGenerationStable'] =
        generations.length >= 2 && distinctGenerations.length == 1;
    counts['generationSamples'] = generations.length;
    observations['faultGenerations'] = [
      for (final generation in distinctGenerations) '$generation',
    ];
    observations['retried'] = summarize(retried);
    await shot('retry-ready');
    await mark(_stageRetryReady);

    // 8. Explicit continue: only now may the deferred safe exec run and the
    // follow-up model step complete.
    await _tap('history-resume-$threadId');
    // The safe exec is a millisecond echo, so the GUI is not required to be
    // polled while the call is Running. Accept the exact call either live or
    // already succeeded with the committed marker; `_toolRanOrDelivered` rejects
    // every other call identity and status. The early-execution proof is the
    // pre-resume deferred state above, not catching this transient.
    await waitFor(
      (snapshot) => _toolRanOrDelivered(_execTool(snapshot, _toolCallId)),
      'history_fault_resumed_tool_running',
      timeout: const Duration(seconds: 120),
    );
    final delivered = await waitFor(
      (snapshot) => execDelivered(snapshot, _toolCallId, _toolMarker),
      'history_fault_resumed_tool_delivered',
      timeout: const Duration(seconds: 180),
    );
    checks['resumeRunsDeferredTool'] = true;
    counts['resumedToolStatus'] = _execTool(delivered, _toolCallId)?['status'];
    await shot('resumed-tool');

    // 9. The durable save must settle with the answer exactly once, and the tool
    // result exactly once, so nothing was lost or duplicated across the fault.
    final settled = await waitFor(
      (snapshot) =>
          isDurableSettled(snapshot) &&
          answerMatchCount(snapshot, _answer) == 1,
      'history_fault_resumed_answer',
      timeout: const Duration(seconds: 240),
    );
    checks['durableSettledAfterResume'] = true;
    final answerMatches = answerMatchCount(settled, _answer);
    checks['resumedAnswerExactlyOnce'] = answerMatches == 1;
    counts['answerMatches'] = answerMatches;
    final markerTools = execTools(settled)
        .where(
          (tool) =>
              tool['callId'] == _toolCallId &&
              tool['status'] == 'succeeded' &&
              (tool['result'] as String? ?? '').contains(_toolMarker),
        )
        .length;
    checks['resumedToolResultExactlyOnce'] = markerTools == 1;
    counts['markerToolResults'] = markerTools;
    observations['settled'] = summarize(settled);
    await shot('complete');
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
    await File('${output.path}/history-fault-$name.png')
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
        '${output.path}/history-fault-failure-snapshot.json',
        await driver.readSnapshot().timeout(const Duration(seconds: 10)),
      );
    } on Object {
      // Ignored: the summary still carries the original error and stage.
    }
    try {
      final png = await driver.screenshot().timeout(
        const Duration(seconds: 20),
      );
      await File('${output.path}/history-fault-failure.png').writeAsBytes(png);
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
      'scenario': 'history-fault',
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
      '${output.path}/history-fault-summary.json',
    ).writeAsString('${const JsonEncoder.withIndent('  ').convert(summary)}\n');
  }

  void raiseIfFailed(Object? shutdownFailure) {
    final failedChecks = checks.entries
        .where((entry) => entry.value != true)
        .map((entry) => entry.key)
        .toList(growable: false);
    if (failedChecks.isNotEmpty) {
      throw StateError(
        'history-fault checks failed: ${failedChecks.join(', ')}',
      );
    }
    if (shutdownFailure != null || shutdownError != null) {
      throw StateError(
        'history-fault native GUI shutdown failed: ${shutdownFailure ?? shutdownError}',
      );
    }
  }
}
