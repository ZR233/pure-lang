import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';
// The multi-item stress journey already owns the session-switch flow helpers.
import 'stress_sessions.dart' show selectedThread, settled, waitForSnapshot;

/// Single-item long-body stress journey.
///
/// The fixture appends every increment to one stable item/part, so this journey
/// is recorded separately from the multi-item stress journey even though it
/// carries the same event count and nominal rate. It records only GUI-observed
/// timing and raw frame data: the synthetic provider's own byte count lives in
/// the fixture report and is never treated as an FRB transfer measurement.
const _prompt = 'Stream the local GUI stress body fixture';
const _largePrompt = 'Stream the local GUI stress body large fixture';
const _expectedTokens = 20000;

/// Every fixture increment is `body-%05d ` (5 + 5 + 1 = 11 UTF-16 code units),
/// so the strict full reply is [_expectedBodyCharacters]. A bounded preview such
/// as 8192 code units is not a delivered body and never counts as full.
const _bodyUnitCharacters = 11;
const _expectedBodyCharacters = _expectedTokens * _bodyUnitCharacters;

/// Large-body increments append [_largeBodyPad] `W` characters plus a trailing
/// space, so each increment is `_largeBodyUnitCharacters` (12 + pad) code units
/// and the full reply exceeds the native timeline body window. ASCII markers keep
/// code units and UTF-8 bytes equal.
const _largeBodyPad = 20;
const _largeBodyUnitCharacters = 12 + _largeBodyPad;
const _expectedLargeBodyCharacters = _expectedTokens * _largeBodyUnitCharacters;

/// The native timeline body window the large scenario must exceed so it can
/// exercise a body genuinely larger than the window.
const _nativeBodyWindowBytes = 256 * 1024;

/// Strict follow-up session prompt the body fixture script answers with a title.
const _secondSessionPrompt = 'Local GUI stress session 1';

/// Reading into the reply drags the pointer downward in fixed steps.
///
/// `FlutterDriver.scroll`/`scrollBy` take a *pointer* offset (`driver.dart`:
/// `[dx] and [dy] specify the total offset for the entire scrolling action`), so
/// a positive `dy` drags the finger down and reveals earlier content toward the
/// top of the history — the same convention `scrollUntilVisible` documents ("if
/// [item] is above, specify a positive value for [dyScroll]"). A negative `dy`
/// keeps pulling content from below and never leaves the pinned last line.
const _readScrollStep = 700.0;

/// Bounded read attempts: a view that refuses to leave the bottom becomes a
/// reported failure instead of an endless loop.
const _maxReadSteps = 24;

/// At least this many pixels of content must remain below the viewport for the
/// reading position to count as detached from the bottom.
const _minDetachedExtentAfter = 200.0;

Future<void> main(List<String> args) async {
  if (args.length != 4 && args.length != 5) {
    stderr.writeln(
      'usage: stress_body.dart VM_URL PROJECT_DIR OUTPUT_DIR COORD_DIR [SCENARIO]',
    );
    exitCode = 64;
    return;
  }
  // The same journey drives the default and the large-body scenario; the
  // scenario only selects the expected content and marker shape.
  final scenario = args.length == 5 ? args[4] : 'stress-body';
  final largeBody = scenario == 'stress-body-large';
  final expectedBodyCharacters = largeBody
      ? _expectedLargeBodyCharacters
      : _expectedBodyCharacters;
  final output = Directory(args[2]);
  final coord = Directory(args[3]);
  Future<void> mark(String value) =>
      File('${coord.path}/stress-body-stage').writeAsString(value);
  final FlutterDriverSession driver;
  try {
    driver = await FlutterDriverSession.connect(vmServiceUrl: args[0]);
  } catch (error, stackTrace) {
    await _writeReport(output, <String, Object?>{
      'scenario': scenario,
      'verdict': 'pending',
      'status': 'failed',
      'stage': 'connect_failed',
      'error': error.toString(),
      'shutdownError': null,
    });
    Error.throwWithStackTrace(error, stackTrace);
  }
  var stage = 'connected';
  // Records both the coordinator marker and the stage reported in the summary, so
  // a failure never reports a coarser stage than the marker file.
  Future<void> markStage(String value) async {
    stage = value;
    await mark(value);
  }

  Object? failure;
  StackTrace? failureStack;
  String? shutdownError;
  Map<String, dynamic>? frames;
  final report = <String, Object?>{};
  final checks = <String, Object?>{};
  final findings = <String, Object?>{};
  final pendingEvidence = <String>[
    'the fixture provider bytes come from the synthetic provider stream and are '
        'never an FRB transfer measurement',
  ];
  try {
    await mark(stage);
    await _openProject(driver, args[1]);
    stage = 'project_opened';
    await mark(stage);
    await driver.requestData(
      'frame-start',
      timeout: const Duration(seconds: 20),
    );
    final submittedAt = DateTime.now().millisecondsSinceEpoch;
    report['submittedAt'] = submittedAt;
    await _submit(driver, largeBody ? _largePrompt : _prompt);
    stage = 'prompt_submitted';
    await mark(stage);
    final streaming = await _waitFor(
      driver,
      (snapshot) => _answerTextLength(snapshot) > 0,
      'streaming',
      evidence: output,
    );
    final firstContentAt = DateTime.now().millisecondsSinceEpoch;
    report['firstContentAt'] = firstContentAt;
    report['timeToFirstContentMillis'] = firstContentAt - submittedAt;
    report['streamingOutputTokens'] = _outputTokens(streaming);
    report['streamingAnswerLength'] = _answerTextLength(streaming);
    stage = 'streaming';
    await mark(stage);
    await File('${output.path}/stress-body-streaming.png')
        .writeAsBytes(await driver.screenshot());
    // Large body only: prove the body really keeps following the stream while it
    // is still generating *above* the 256KiB window. This is a generation-time
    // observation, not a post-terminal one, and it never taps a load-full
    // affordance.
    if (largeBody) {
      final growing = await _sampleStreamingLargeBody(driver, output);
      report['streamingGrowth'] = growing;
      checks['largeBodyGrowingFullFollow'] =
          growing['windowPassed'] == true &&
          growing['identityStable'] == true &&
          (growing['growthFramesAboveWindow'] as int) >= 2;
    }
    final terminal = await _waitFor(
      driver,
      (snapshot) =>
          _outputTokens(snapshot) >= _expectedTokens && !_isBusy(snapshot),
      'terminal',
      evidence: output,
    );
    final terminalAt = DateTime.now().millisecondsSinceEpoch;
    report['terminalAt'] = terminalAt;
    report['terminalOutputTokens'] = _outputTokens(terminal);
    stage = 'terminal';
    await mark(stage);
    // The local snapshot must not be named `settled`: that would shadow the
    // imported `settled(snapshot, threadId)` predicate used for the reopen wait.
    var settledSnapshot = await _waitFor(
      driver,
      _settled,
      'settled',
      evidence: output,
    );
    report['durableSettledAt'] = DateTime.now().millisecondsSinceEpoch;
    report['settledOutputTokens'] = _outputTokens(settledSnapshot);
    final answers = _answerLengths(settledSnapshot);
    report['answerRowCount'] = answers.length;
    report['answerTextLengths'] = answers;
    stage = 'settled';
    await mark(stage);
    // Large body only: hold (bounded) until the full delivered body is actually
    // displayed before asserting, so a snapshot taken mid-render cannot falsely
    // fail and a genuinely truncated body still fails within the deadline. The
    // hold waits on a real rendered-content condition, never a fixed sleep.
    if (largeBody) {
      settledSnapshot = await _waitFor(
        driver,
        (snapshot) => _answerTextLength(snapshot) >= expectedBodyCharacters,
        'body_rendered',
        evidence: output,
      );
      report['bodyRenderedAt'] = DateTime.now().millisecondsSinceEpoch;
    }
    await File('${output.path}/stress-body-settled.png')
        .writeAsBytes(await driver.screenshot());

    // The strict fixture appends _expectedTokens increments to one stable row, so
    // the delivered assistant body must be the full [expectedBodyCharacters]. A
    // bounded preview (for example 8192 code units) is not a delivered body and
    // never counts as full, and the journey never taps a load-full/paging
    // affordance: the full text must be present without one. The large scenario
    // additionally requires the body to exceed the native timeline body window.
    final longBody = _longBodyRow(settledSnapshot);
    final bodyText = longBody?['text'] as String?;
    final bodyCharacters = bodyText?.length;
    final bodyBytes = bodyText == null ? null : utf8.encode(bodyText).length;
    final prefixOk = bodyText != null && bodyText.startsWith('body-00000 ');
    final suffixOk =
        bodyText != null && bodyText.endsWith(_expectedBodySuffix(largeBody));
    final window = settledSnapshot['timelineWindow'];
    final longBodyId = longBody?['id'];
    final previewed = _idList(window, 'previewedItemIds').contains(longBodyId);
    final loaded = _idList(window, 'loadedItemIds').contains(longBodyId);
    final bodyPending = _idList(
      window,
      'pendingItemBodyIds',
    ).contains(longBodyId);
    final fullTextPresent =
        bodyCharacters == expectedBodyCharacters && prefixOk && suffixOk;
    // Bounded boundary previews so a reviewer can re-check the content without
    // duplicating the whole reply.
    String? headPreview;
    String? tailPreview;
    if (bodyText != null) {
      headPreview = bodyText.substring(
        0,
        bodyText.length < 32 ? bodyText.length : 32,
      );
      tailPreview = bodyText.substring(
        bodyText.length < 32 ? 0 : bodyText.length - 32,
      );
    }
    report['longBody'] = <String, Object?>{
      'rowId': longBodyId,
      'expectedCharacters': expectedBodyCharacters,
      'deliveredCharacters': bodyCharacters,
      'deliveredBytes': bodyBytes,
      'largeBody': largeBody,
      'prefixOk': prefixOk,
      'suffixOk': suffixOk,
      'headPreview': headPreview,
      'tailPreview': tailPreview,
      'fullTextPresent': fullTextPresent,
      'previewedItem': previewed,
      'loadedItem': loaded,
      'pendingItemBody': bodyPending,
      'manualLoadTapped': false,
    };
    // Only the full body is a pass; a bounded preview (8192) is not. The
    // delivered text length/content is the authoritative signal, so a stale
    // paging flag can never turn a genuinely full body into a false failure.
    checks['longBodyFullText'] = fullTextPresent;
    // The large scenario must additionally deliver a body genuinely above the
    // 256KiB native timeline window; the fixed 220,000-character body cannot.
    if (largeBody) {
      checks['largeBodyAboveNativeWindow'] =
          bodyBytes != null && bodyBytes > _nativeBodyWindowBytes;
    }
    // Driver-only application-payload counters, kept apart from the provider
    // output bytes recorded in fixture-status.json.
    final delivery = _contentDelivery(settledSnapshot);
    if (delivery == null) {
      pendingEvidence.add(
        'content delivery: snapshot exposes no contentDelivery field, so the '
        'application-payload body byte count is pending an integrated Driver '
        'build',
      );
    } else {
      findings['contentDelivery'] = <String, Object?>{
        'enabled': delivery['enabled'],
        'metric': delivery['metric'],
        'bodyUtf8Bytes': delivery['bodyUtf8Bytes'],
        'contentChanges': delivery['contentChanges'],
        'resets': delivery['resets'],
        'patches': delivery['patches'],
        'maxWindowItems': delivery['maxWindowItems'],
        'note': 'application payload UTF-8 bytes of delivered body text; not FRB/wire bytes',
      };
    }

    // Cross-block selection/copy. The production timeline renders this long
    // plain reply through `_PlainBodyText`, which seals it into several bounded
    // chunks that each become a real `RenderParagraph`. This phase drives the
    // real `SelectionArea` and the product's own context-menu copy callback, then
    // compares the platform clipboard readback with the canonical fixture body.
    // It runs *before* the reading/session-switch phases, so a selection that
    // disturbed the history scroll would also break those existing assertions;
    // the phase additionally records the scroll geometry before and after.
    stage = 'selection_copy';
    await mark(stage);
    final selectionCopy = await _selectAndCopyBody(
      driver,
      output,
      expectedBodyCharacters,
      largeBody,
    );
    report['selectionCopy'] = selectionCopy;
    checks['crossBlockSelectionCopy'] = selectionCopy['pass'] == true;

    // Read into the interior of the long assistant body, then create a second
    // session and switch back to re-check the reading anchor. The session-switch
    // helpers are the ones the multi-item stress journey already uses.
    final originalId = selectedThread(settledSnapshot);
    await mark('reading');
    final reading = await _readIntoLongBody(driver, output, settledSnapshot);
    report['reading'] = reading;
    await mark('second_session');
    final secondId = await _createSecondSession(
      driver,
      originalId,
      onStage: markStage,
      output: output,
    );
    report['secondSessionId'] = secondId;
    await File('${output.path}/stress-body-second-session.png')
        .writeAsBytes(await driver.screenshot());
    await mark('revisit_original');
    await driver.scrollUntilVisible(
      find.byValueKey('sidebar-project-tree'),
      find.byValueKey('thread-row-$originalId'),
      dyScroll: -200,
      timeout: const Duration(seconds: 45),
    );
    await driver.tap(find.byValueKey('thread-row-$originalId'));
    var reopened = await _waitFor(
      driver,
      (snapshot) =>
          _selectedThreadOrNull(snapshot) == originalId &&
          snapshot['timelineWindow'] is Map &&
          settled(snapshot, originalId),
      'reopen_original',
      evidence: output,
    );
    // Switching back re-fetches the full body and then restores the reading
    // position. Wait (bounded) until the *same* body identity is fully present
    // and the restore has finished before reading the anchor. The wait never
    // compares the anchor to the original position: waiting on anchor equality
    // would let a never-restored view pass. 20s is the accepted upper bound; a
    // body that is complete but restored wrong then fails immediately below.
    final readingRowId = reading['rowId'];
    final reopenStartedAt = DateTime.now().millisecondsSinceEpoch;
    final reopenDeadline = DateTime.now().add(const Duration(seconds: 20));
    var bodyReady = false;
    Map<String, Object?>? reopenDiagnostics;
    while (true) {
      // No assistant long-body identity was established by the reading phase, so
      // there is nothing to restore; fail fast into the diagnostics below.
      if (readingRowId is! String) break;
      if (_reopenBodyReady(
        reopened,
        readingRowId,
        expectedBodyCharacters,
        largeBody,
      )) {
        bodyReady = true;
        break;
      }
      if (!DateTime.now().isBefore(reopenDeadline)) break;
      await Future<void>.delayed(const Duration(milliseconds: 100));
      reopened = await driver.readSnapshot();
    }
    final bodyWaitMillis =
        DateTime.now().millisecondsSinceEpoch - reopenStartedAt;
    if (!bodyReady) {
      reopenDiagnostics = _reopenDiagnostics(
        reopened,
        readingRowId is String ? readingRowId : '',
        expectedBodyCharacters,
        largeBody,
      );
      try {
        await File(
          '${output.path}/stress-body-reopen-diagnostics.json',
        ).writeAsString(
          '${const JsonEncoder.withIndent('  ').convert(reopenDiagnostics)}\n',
        );
      } catch (_) {
        // Evidence writes must never mask the real observation.
      }
    }
    await File('${output.path}/stress-body-reopened.png')
        .writeAsBytes(await driver.screenshot());
    final reopenedScroll = _timelineScroll(reopened);
    await File('${output.path}/stress-body-timeline-scroll.json').writeAsString(
      '${const JsonEncoder.withIndent('  ').convert(reopenedScroll)}\n',
    );
    final reopenedRows = _rowIds(reopened);
    final readingRow = reading['rowId'];
    final readingAnchorAfter = _anchorId(reopenedScroll);
    final readingOffset = reading['anchorOffset'];
    final readingOffsetAfter = _anchorOffset(reopenedScroll);
    // The reading position, the anchor and the restore are all asserted from the
    // real scroll diagnostic when the UI exposes it; without the diagnostic the
    // restoration is pending, never passed. The reading position must be inside
    // the assistant long body and away from the bottom, so a view that was yanked
    // back to the last line cannot pass. The anchor is compared only after the
    // body is complete and the restore has settled; when it is complete and still
    // wrong the run fails immediately, and the offset tolerance is never widened.
    final diagnosticPresent = reopenedScroll != null;
    final anchorRestored =
        bodyReady &&
        reading['insideLongBody'] == true &&
        reading['followingBottom'] == false &&
        reading['anchorItemId'] is String &&
        reading['anchorOffset'] is num &&
        readingAnchorAfter == reading['anchorItemId'] &&
        readingOffsetAfter != null &&
        (readingOffsetAfter - (readingOffset as num).toDouble()).abs() <= 1.0;
    report['sessionSwitch'] = <String, Object?>{
      'originalThreadId': originalId,
      'secondThreadId': secondId,
      'returnedToOriginal': _selectedThreadOrNull(reopened) == originalId,
      'bodyReadyAfterReopen': bodyReady,
      'bodyWaitMillis': bodyWaitMillis,
      'bodyWaitUpperBoundMillis': 20000,
      'restorePendingAfterReopen': reopenedScroll?['restorePending'],
      'programmaticScrollAfterReopen': reopenedScroll?['programmaticScroll'],
      'readingRowId': readingRow,
      'readingRowPresentAfterReopen':
          readingRow is String && reopenedRows.contains(readingRow),
      'readingAnchorBefore': reading['anchorItemId'],
      'readingAnchorAfter': readingAnchorAfter,
      'readingOffsetBefore': readingOffset,
      'readingOffsetAfter': readingOffsetAfter,
      'readingAnchorRestored': diagnosticPresent ? anchorRestored : null,
      'reopenDiagnostics': reopenDiagnostics,
    };
    if (diagnosticPresent) {
      checks['readingAnchorRestored'] = anchorRestored;
    } else {
      pendingEvidence.add(
        'session switch: the reopened snapshot exposes no timelineScroll '
        'diagnostic, so reading-anchor restoration is pending rather than passed',
      );
    }
    stage = 'session_switch_complete';
    await mark(stage);
  } catch (error, stackTrace) {
    failure = error;
    failureStack = stackTrace;
    // Capture the failing observation before shutdown. The original error is
    // always preserved: a driver that already exited yields no extra evidence
    // instead of a second, misleading error.
    await _captureFailure(driver, output);
  }
  try {
    frames = jsonDecode(
      await driver.requestData(
        'frame-stop',
        timeout: const Duration(seconds: 20),
      ),
    ) as Map<String, dynamic>;
    await File(
      '${output.path}/stress-body-frames.json',
    ).writeAsString('${const JsonEncoder.withIndent('  ').convert(frames)}\n');
  } catch (error) {
    shutdownError ??= 'frame recording stop failed: $error';
  }
  try {
    final shutdown = jsonDecode(
      await driver
          .requestData('shutdown', timeout: const Duration(seconds: 60))
          .timeout(const Duration(seconds: 65)),
    );
    if (shutdown is! Map || shutdown['shutdown'] != 'completed') {
      shutdownError ??= 'native GUI shutdown did not report completion';
    }
  } catch (error) {
    shutdownError ??= 'native GUI shutdown failed: $error';
  } finally {
    try {
      await driver.close().timeout(const Duration(seconds: 5));
    } catch (_) {
      // A completed shutdown must not keep the acceptance process open.
    }
  }
  final failedChecks = checks.entries
      .where((entry) => entry.value != true)
      .map((entry) => entry.key)
      .toList(growable: false);
  final status =
      failure == null && shutdownError == null && failedChecks.isEmpty
      ? 'complete'
      : 'failed';
  await _writeReport(output, <String, Object?>{
    'scenario': scenario,
    'verdict': 'pending',
    'status': status,
    'stage': stage,
    'error': failure?.toString(),
    'shutdownError': shutdownError,
    'failedChecks': failedChecks,
    'checks': checks,
    'findings': findings,
    'pendingEvidence': pendingEvidence,
    'observations': report,
    'frameSampleCount': frames?['samples'] is List
        ? (frames!['samples'] as List).length
        : null,
    'completedAt': DateTime.now().toUtc().toIso8601String(),
  });
  await mark(status == 'complete' ? 'complete' : stage);
  if (failure != null) {
    Error.throwWithStackTrace(failure, failureStack!);
  }
  if (shutdownError != null) {
    throw StateError('native GUI shutdown failed: $shutdownError');
  }
  if (failedChecks.isNotEmpty) {
    throw StateError(
      'stress-body checks failed: ${failedChecks.join(', ')}; '
      'longBody=${report['longBody']}; sessionSwitch=${report['sessionSwitch']}',
    );
  }
}

Future<void> _writeReport(Directory output, Map<String, Object?> report) async {
  try {
    await File(
      '${output.path}/stress-body-report.json',
    ).writeAsString('${const JsonEncoder.withIndent('  ').convert(report)}\n');
  } catch (_) {
    // Evidence directories are owned by the coordinator; writing there must not
    // mask the real failure.
  }
}

Future<void> _openProject(FlutterDriverSession driver, String project) async {
  await driver.waitFor(
    find.byValueKey('sidebar-open-project'),
    timeout: const Duration(seconds: 60),
  );
  await _tap(driver, 'sidebar-open-project');
  await _tap(driver, 'add-project-local');
  await driver.waitFor(
    find.byValueKey('add-project-continue-ready'),
    timeout: const Duration(seconds: 30),
  );
  await _tap(driver, 'add-project-continue-ready');
  await driver.waitFor(
    find.byValueKey('project-path-input'),
    timeout: const Duration(seconds: 30),
  );
  await _tap(driver, 'project-path-input');
  await driver.enterText(project);
  await driver.waitFor(
    find.byValueKey('project-path-submit'),
    timeout: const Duration(seconds: 30),
  );
  await _tap(driver, 'project-path-submit');
  await driver.waitFor(
    find.byValueKey('composer-input'),
    timeout: const Duration(seconds: 60),
  );
}

Future<void> _submit(FlutterDriverSession driver, String prompt) async {
  await driver.waitFor(
    find.byValueKey('composer-input'),
    timeout: const Duration(seconds: 30),
  );
  await _tap(driver, 'composer-input');
  await driver.enterText(prompt);
  await driver.waitFor(
    find.byValueKey('composer-submit'),
    timeout: const Duration(seconds: 30),
  );
  await _tap(driver, 'composer-submit');
}

Future<void> _tap(FlutterDriverSession driver, String key) =>
    driver.rawTap(find.byValueKey(key), timeout: const Duration(seconds: 30));

Future<Map<String, dynamic>> _waitFor(
  FlutterDriverSession driver,
  bool Function(Map<String, dynamic>) predicate,
  String label, {
  Duration timeout = const Duration(seconds: 120),
  Directory? evidence,
}) async {
  final deadline = DateTime.now().add(timeout);
  Map<String, dynamic>? last;
  while (DateTime.now().isBefore(deadline)) {
    last = await driver.readSnapshot();
    if (predicate(last)) return last;
    await Future<void>.delayed(const Duration(milliseconds: 100));
  }
  // The timeout must carry the real observed state, and the snapshot must survive
  // after the coordinator reclaims its tempdir.
  if (evidence != null && last != null) {
    try {
      await File(
        '${evidence.path}/stress-body-wait-timeout-$label.json',
      ).writeAsString('${const JsonEncoder.withIndent('  ').convert(last)}\n');
    } catch (_) {
      // Capturing evidence must never replace the timeout it documents.
    }
  }
  throw StateError(
    'timed out waiting for stress-body $label after ${timeout.inSeconds}s: '
    '${_summarize(last)}',
  );
}

/// Saves the last snapshot and a screenshot for a failed journey.
///
/// The caller keeps the original failure: a driver that already exited simply
/// yields no extra evidence instead of a second, misleading error.
Future<void> _captureFailure(
  FlutterDriverSession driver,
  Directory output,
) async {
  try {
    final snapshot = await driver.readSnapshot().timeout(
      const Duration(seconds: 10),
    );
    await File('${output.path}/stress-body-failure-snapshot.json')
        .writeAsString(
          '${const JsonEncoder.withIndent('  ').convert(snapshot)}\n',
        );
  } catch (_) {
    // Ignored: the report still carries the original error and stage.
  }
  try {
    final png = await driver.screenshot().timeout(const Duration(seconds: 20));
    await File('${output.path}/stress-body-failure.png').writeAsBytes(png);
  } catch (_) {
    // Ignored: see above.
  }
}

/// A compact description of the observed state for timeout diagnostics.
String _summarize(Map<String, dynamic>? snapshot) {
  if (snapshot == null) return 'no snapshot was observed';
  final workspace = _workspace(snapshot);
  final persistence = snapshot['persistence'];
  final directory = snapshot['sidebarDirectory'];
  return 'threadStatus=${workspace?['threadStatus']}, '
      'busy=${workspace?['isBusy']}, '
      'sync=${workspace?['syncState']}, '
      'turn=${_turnIdOrNull(snapshot)}, '
      'selected=${_selectedThreadOrNull(snapshot)}, '
      'saving=${persistence is Map ? persistence['kind'] : null}, '
      'sessions=${directory is Map ? directory['count'] : null}';
}

String? _turnIdOrNull(Map<String, dynamic> snapshot) {
  final workspace = _workspace(snapshot);
  final turn = workspace?['turn'];
  return turn is Map && turn['id'] is String ? turn['id'] as String : null;
}

Map<String, dynamic>? _workspace(Map<String, dynamic> snapshot) {
  final workspace = snapshot['workspace'];
  return workspace is Map<String, dynamic> ? workspace : null;
}

bool _isBusy(Map<String, dynamic> snapshot) =>
    _workspace(snapshot)?['isBusy'] == true;

int _outputTokens(Map<String, dynamic> snapshot) {
  final usage = _workspace(snapshot)?['usage'];
  return usage is Map ? (usage['outputTokens'] as num? ?? 0).toInt() : 0;
}

bool _settled(Map<String, dynamic> snapshot) {
  final workspace = _workspace(snapshot);
  final persistence = snapshot['persistence'];
  return workspace != null &&
      workspace['isBusy'] == false &&
      workspace['threadStatus'] == 'idle' &&
      workspace['syncState'] == 'ready' &&
      persistence is Map &&
      persistence['kind'] == 'ready' &&
      persistence['pendingCommits'] == 0;
}

List<int> _answerLengths(Map<String, dynamic> snapshot) {
  final timeline = _workspace(snapshot)?['timeline'];
  if (timeline is! List) return const [];
  return [
    for (final row in timeline)
      if (row is Map && row['type'] == 'finalAnswer')
        ((row['text'] as String?) ?? '').length,
  ];
}

int _answerTextLength(Map<String, dynamic> snapshot) =>
    _answerLengths(snapshot).fold(0, (total, length) => total + length);

/// Samples the in-flight large body while it is still generating.
///
/// Waits until the streamed body has passed the 256KiB native timeline window
/// *while the Turn is still not terminal*, then collects at least two consecutive
/// frames where the same assistant row grows and keeps the exact fixture prefix.
/// The evidence is the real body text length and prefix, never an item revision,
/// and the journey never taps a load-full/paging affordance.
Future<Map<String, Object?>> _sampleStreamingLargeBody(
  FlutterDriverSession driver,
  Directory output,
) async {
  final frames = <Map<String, Object?>>[];
  var windowPassed = false;
  var identityStable = true;
  var growthFrames = 0;
  String? previousId;
  int? previousBytes;
  final deadline = DateTime.now().add(const Duration(seconds: 60));
  while (DateTime.now().isBefore(deadline) && growthFrames < 2) {
    final snapshot = await driver.readSnapshot();
    final terminal =
        _outputTokens(snapshot) >= _expectedTokens && !_isBusy(snapshot);
    if (terminal) break;
    final row = _longBodyRow(snapshot);
    final id = row?['id'];
    final text = row?['text'] as String?;
    if (id is! String || text == null) {
      await Future<void>.delayed(const Duration(milliseconds: 100));
      continue;
    }
    final bytes = utf8.encode(text).length;
    if (bytes > _nativeBodyWindowBytes) {
      windowPassed = true;
      final prefixOk = text.startsWith('body-00000 ');
      final sameIdentity = previousId == id;
      final grew =
          sameIdentity && previousBytes != null && bytes > previousBytes;
      if (previousId != null && !sameIdentity) identityStable = false;
      if (grew && prefixOk) growthFrames += 1;
      frames.add(<String, Object?>{
        'at': DateTime.now().millisecondsSinceEpoch,
        'rowId': id,
        'characters': text.length,
        'bytes': bytes,
        'prefixOk': prefixOk,
        'grewOverPrevious': grew,
        'terminal': terminal,
      });
      previousId = id;
      previousBytes = bytes;
    }
    await Future<void>.delayed(const Duration(milliseconds: 100));
  }
  final evidence = <String, Object?>{
    'windowPassed': windowPassed,
    'identityStable': identityStable,
    'growthFramesAboveWindow': growthFrames,
    'framesSampled': frames.length,
    'thresholdBytes': _nativeBodyWindowBytes,
    'manualLoadTapped': false,
    // Bounded tail so a reviewer can re-check the growth without the whole list.
    'frames': frames.length > 8 ? frames.sublist(frames.length - 8) : frames,
  };
  try {
    await File('${output.path}/stress-body-large-growth.json').writeAsString(
      '${const JsonEncoder.withIndent('  ').convert(evidence)}\n',
    );
  } catch (_) {
    // Evidence writes must never mask the observation they document.
  }
  return evidence;
}

/// Scrolls into the *interior* of the assistant long body — the actual reply row,
/// never the user input row — until the viewport anchor sits inside that row and
/// the viewport has really left the pinned last line, then records the row
/// identity and the per-step geometry that proves it.
///
/// The scroll step is **positive** (see [_readScrollStep]); a negative step keeps
/// pulling content from below, so the anchor never leaves the bottom.
Future<Map<String, Object?>> _readIntoLongBody(
  FlutterDriverSession driver,
  Directory output,
  Map<String, dynamic> settledSnapshot,
) async {
  final row = _longBodyRow(settledSnapshot);
  final target = row?['id'];
  final followingBottomBefore = _timelineScroll(
    settledSnapshot,
  )?['followingBottom'];
  if (target is! String) {
    return <String, Object?>{
      'rowId': null,
      'scrolled': false,
      'anchorItemId': null,
      'insideLongBody': false,
      'reason':
          'no assistant final-answer row identity was available to read into',
    };
  }
  var scrolled = false;
  var scroll = _timelineScroll(await driver.readSnapshot());
  var reading = _readingGeometry(scroll, target);
  // Every step records the real geometry before/after the scroll, so the reading
  // position can be re-checked instead of trusted from a single frame.
  final geometry = <Map<String, Object?>>[
    <String, Object?>{'phase': 'start', ...reading},
  ];
  try {
    for (
      var step = 0;
      step < _maxReadSteps && reading['insideLongBody'] != true;
      step++
    ) {
      await driver.scrollBy(
        find.byValueKey('timeline-scrollable'),
        _readScrollStep,
        timeout: const Duration(seconds: 15),
      );
      scrolled = true;
      scroll = _timelineScroll(await driver.readSnapshot());
      reading = _readingGeometry(scroll, target);
      geometry.add(<String, Object?>{
        'phase': 'after_scroll_${step + 1}',
        ...reading,
      });
    }
  } on Object catch (error) {
    // The geometry below is still the real observation, so record the failure
    // instead of guessing the reading position.
    await File('${output.path}/stress-body-reading-scroll-error.txt')
        .writeAsString('$error\n');
  }
  await File('${output.path}/stress-body-reading.png')
      .writeAsBytes(await driver.screenshot());
  await File('${output.path}/stress-body-reading-scroll.json').writeAsString(
    '${const JsonEncoder.withIndent('  ').convert(<String, Object?>{'targetRowId': target, 'geometry': geometry, 'final': scroll})}\n',
  );
  final pixelsBefore = geometry.first['pixels'];
  final pixelsAfter = reading['pixels'];
  final maxScrollExtent = reading['maxScrollExtent'];
  final extentAfter = reading['extentAfter'];
  // A zoomed-out view of the same numbers, so a reviewer can see the viewport
  // really moved up (pixels decreased from the bottom).
  final double? pixelsDelta = pixelsBefore is num && pixelsAfter is num
      ? pixelsBefore.toDouble() - pixelsAfter.toDouble()
      : null;
  final pixelsMoved = pixelsDelta != null && pixelsDelta > 1.0;
  final detachedThreshold = _detachedExtentThreshold(
    reading['viewportDimension'],
  );
  // Leaving the bottom is proven from live geometry, never from the scroll
  // request alone: the viewport moved up, the view is no longer following the
  // bottom, and a viewport's worth of content still remains below it.
  final leftBottom =
      reading['followingBottom'] == false &&
      pixelsMoved &&
      extentAfter is num &&
      extentAfter > detachedThreshold &&
      pixelsAfter is num &&
      maxScrollExtent is num &&
      pixelsAfter < maxScrollExtent - 1.0;
  return <String, Object?>{
    // The stable row identity is the assistant long-body row id, so it can be
    // compared against the reopened window; the anchor item and its offset are
    // the reading position inside that row.
    'rowId': target,
    'rowType': row?['type'],
    'scrolled': scrolled,
    'steps': geometry.length - 1,
    'anchorItemId': reading['anchorItemId'],
    'anchorOffset': reading['anchorOffset'],
    'pixelsBefore': pixelsBefore,
    'pixelsAfter': pixelsAfter,
    'pixelsDelta': pixelsDelta,
    'maxScrollExtent': maxScrollExtent,
    'extentAfter': extentAfter,
    'detachedThreshold': detachedThreshold,
    'pixelsMoved': pixelsMoved,
    'leftBottom': leftBottom,
    'insideLongBody':
        reading['insideLongBody'] == true && pixelsMoved && leftBottom,
    'followingBottom': reading['followingBottom'],
    'detachedByUser': reading['detachedByUser'],
    'followingBottomBeforeReading': followingBottomBefore,
    'geometry': geometry,
  };
}

/// One reading-position observation: the anchor identity/offset plus the live
/// scroll geometry that proves the viewport left the bottom.
Map<String, Object?> _readingGeometry(
  Map<String, dynamic>? scroll,
  String target,
) {
  final anchorId = _anchorId(scroll);
  final offset = _anchorOffset(scroll);
  final pixels = _doubleOrNull(scroll?['pixels']);
  final maxScrollExtent = _doubleOrNull(scroll?['maxScrollExtent']);
  final extentAfter = _doubleOrNull(scroll?['extentAfter']);
  final viewport = _doubleOrNull(scroll?['viewportDimension']);
  final followingBottom = scroll?['followingBottom'];
  // The anchor offset is the topmost visible row's *content* offset (the UI
  // subtracts the bottom-alignment slack, see timeline_paging.dart
  // `_captureAnchor`). Inside the oversized reply that row's top sits above the
  // viewport, so the offset is negative; a positive value would mean the anchor
  // is below the viewport, which cannot happen for the topmost visible row.
  final inside =
      anchorId == target &&
      offset != null &&
      offset < 0 &&
      followingBottom == false &&
      extentAfter != null &&
      extentAfter > _detachedExtentThreshold(viewport) &&
      pixels != null &&
      maxScrollExtent != null &&
      pixels < maxScrollExtent - 1.0;
  return <String, Object?>{
    'anchorItemId': anchorId,
    'anchorOffset': offset,
    'pixels': pixels,
    'maxScrollExtent': maxScrollExtent,
    'extentAfter': extentAfter,
    'viewportDimension': viewport,
    'followingBottom': followingBottom,
    'detachedByUser': scroll?['detachedByUser'],
    'insideLongBody': inside,
  };
}

/// A full viewport of content (never below [_minDetachedExtentAfter] px) must
/// remain below the viewport for the reading position to count as detached.
double _detachedExtentThreshold(Object? viewportDimension) {
  final viewport = _doubleOrNull(viewportDimension) ?? 0;
  return viewport > _minDetachedExtentAfter
      ? viewport
      : _minDetachedExtentAfter;
}

double? _doubleOrNull(Object? value) => value is num ? value.toDouble() : null;

/// Creates one strict follow-up session and waits for its title to settle.
///
/// Every wait is bounded and every step is staged: a stuck composer fails into
/// the report with the observed state instead of hanging the host until its own
/// timeout. `FlutterDriver.waitFor`/`waitForAbsent` default to *no* timeout, so
/// the finder waits below always pass an explicit deadline.
Future<String> _createSecondSession(
  FlutterDriverSession driver,
  String originalId, {
  required Future<void> Function(String stage) onStage,
  required Directory output,
}) async {
  await onStage('second_session_new_tapped');
  await driver.tap(find.byValueKey('sidebar-new-session'));
  await waitForSnapshot(
    driver,
    (snapshot) => _selectedThreadOrNull(snapshot) == null,
    'new session',
  );
  await onStage('second_session_composer');
  await driver.waitFor(
    find.byValueKey('start-page-selectors'),
    timeout: const Duration(seconds: 30),
  );
  await driver.waitFor(
    find.byValueKey('composer-input'),
    timeout: const Duration(seconds: 30),
  );
  await driver.tap(find.byValueKey('composer-input'));
  await driver.enterText(_secondSessionPrompt);
  await onStage('second_session_typed');
  // Keep the composer state the draft wait is about, so a stuck draft can be
  // re-checked after the coordinator reclaims its tempdir.
  await File(
    '${output.path}/stress-body-second-session-composer.json',
  ).writeAsString(
    '${const JsonEncoder.withIndent('  ').convert(await driver.readSnapshot())}\n',
  );
  // The submit button only becomes usable once the draft is canonical in the
  // bridge state, so wait for the draft itself (the multi-item stress journey
  // does the same) instead of tapping an indeterminate button.
  await waitForSnapshot(
    driver,
    (snapshot) => _newThreadDraft(snapshot) == _secondSessionPrompt,
    'second session draft',
  );
  await driver.waitFor(
    find.byValueKey('composer-submit'),
    timeout: const Duration(seconds: 15),
  );
  await onStage('second_session_submitted');
  await driver.tap(find.byValueKey('composer-submit'));
  final opened = await waitForSnapshot(driver, (snapshot) {
    final selected = _selectedThreadOrNull(snapshot);
    return selected != null && selected != originalId;
  }, 'second session open');
  final id = selectedThread(opened);
  await onStage('second_session_opened');
  await waitForSnapshot(driver, (snapshot) {
    final directory = snapshot['sidebarDirectory'];
    if (directory is! Map) return false;
    final titles = directory['titles'];
    return snapshot['timelineWindow'] is Map &&
        settled(snapshot, id) &&
        titles is Map &&
        titles[id] == 'Fixture Session 1';
  }, 'second session complete');
  await onStage('second_session_complete');
  return id;
}

/// The canonical new-thread composer draft, when the snapshot exposes it.
String? _newThreadDraft(Map<String, dynamic> snapshot) {
  final navigation = snapshot['navigation'];
  if (navigation is! Map) return null;
  final composer = navigation['newThreadComposer'];
  if (composer is! Map) return null;
  final draft = composer['draft'];
  return draft is String ? draft : null;
}

String? _selectedThreadOrNull(Map<String, dynamic> snapshot) {
  final navigation = snapshot['navigation'];
  if (navigation is! Map) return null;
  final selected = navigation['selectedThreadId'];
  return selected is String ? selected : null;
}

Map<String, dynamic>? _timelineScroll(Map<String, dynamic> snapshot) {
  final scroll = snapshot['timelineScroll'];
  return scroll is Map<String, dynamic> ? scroll : null;
}

List<String> _rowIds(Map<String, dynamic> snapshot) {
  final timeline = _workspace(snapshot)?['timeline'];
  if (timeline is! List) return const [];
  return [
    for (final row in timeline)
      if (row is Map && row['id'] is String) row['id'] as String,
  ];
}

List<Map<String, dynamic>> _timelineRows(Map<String, dynamic> snapshot) {
  final timeline = _workspace(snapshot)?['timeline'];
  if (timeline is! List) return const [];
  return timeline.whereType<Map<String, dynamic>>().toList(growable: false);
}

/// The assistant long-body row: the final answer carrying the most text.
Map<String, dynamic>? _longBodyRow(Map<String, dynamic> snapshot) {
  Map<String, dynamic>? best;
  var bestLength = -1;
  for (final row in _timelineRows(snapshot)) {
    if (row['type'] != 'finalAnswer' || row['id'] is! String) continue;
    final length = (row['text'] as String?)?.length ?? 0;
    if (length > bestLength) {
      bestLength = length;
      best = row;
    }
  }
  return best;
}

/// The Driver-only application-payload counters, when the GUI exposes them.
Map<String, dynamic>? _contentDelivery(Map<String, dynamic> snapshot) {
  final delivery = snapshot['contentDelivery'];
  return delivery is Map<String, dynamic> ? delivery : null;
}

List<String> _idList(Object? window, String key) {
  if (window is! Map) return const [];
  final value = window[key];
  if (value is! List) return const [];
  return value.whereType<String>().toList(growable: false);
}

String? _anchorId(Map<String, dynamic>? scroll) {
  final anchor = scroll?['anchor'];
  if (anchor is Map && anchor['itemId'] is String) {
    return anchor['itemId'] as String;
  }
  return null;
}

double? _anchorOffset(Map<String, dynamic>? scroll) {
  final anchor = scroll?['anchor'];
  final offset = anchor is Map ? anchor['offset'] : null;
  return offset is num ? offset.toDouble() : null;
}

/// The head marker every fixture body starts with.
const _bodyHeadMarker = 'body-00000 ';

/// The tail marker the strict fixture body ends with, per scenario.
String _expectedBodySuffix(bool largeBody) =>
    largeBody ? 'body-19999 ${'W' * _largeBodyPad} ' : ' body-19999 ';

/// Drives a real `SelectionArea` select/copy of the whole long-body region and
/// verifies the clipboard readback against the canonical fixture body.
///
/// The production timeline renders the long reply through `_PlainBodyText`,
/// which seals it into several bounded chunks that each become a real
/// `RenderParagraph`. This phase proves a real selection/copy across those
/// chunks keeps every character and inserts no newline:
///
/// * the rendered chunks must concatenate to the exact canonical body, carried
///   by at least two distinct `RenderParagraph`s (never the domain text), and
/// * the clipboard, read back from the platform after the product's own
///   context-menu copy callback, must contain that exact body as one contiguous
///   run.
///
/// The domain text is never written to the clipboard directly: the copy is the
/// product callback and the readback comes from `Clipboard.getData`. The
/// selection is cancelled with a real click and the region must then report no
/// copyable selection, so the later reading/session-switch phases start clean.
/// The Driver bridge reports a gap instead of passing when the production chunk
/// paragraphs or the copyable selection are unavailable.
Future<Map<String, Object?>> _selectAndCopyBody(
  FlutterDriverSession driver,
  Directory output,
  int expectedBodyCharacters,
  bool largeBody,
) async {
  final expectedBody = _canonicalBody(largeBody);
  final expectedHash = _fnv1a64(expectedBody).toRadixString(16);
  final scrollBefore = _timelineScroll(await driver.readSnapshot());
  final pixelsBefore = _doubleOrNull(scrollBefore?['pixels']);
  // Bounded wait for the production chunk split to finish and stabilise, so the
  // selection never races a frame that seals another chunk (a fresh chunk's
  // paragraph starts unselected). Stability means the same paragraph count over
  // consecutive polls with the full body already rendered.
  Map<String, dynamic>? rendered;
  int? stableParagraphCount;
  var stablePolls = 0;
  final deadline = DateTime.now().add(const Duration(seconds: 20));
  while (true) {
    rendered = _decodeDriverMap(
      await driver.requestData(
        'selection-body',
        timeout: const Duration(seconds: 20),
      ),
    );
    // A Driver that does not implement the request answers with an error; fail
    // immediately with that evidence instead of waiting for the deadline.
    if (rendered?['ok'] != true) break;
    final paragraphs = (rendered?['paragraphCount'] as num? ?? 0).toInt();
    if (rendered?['characters'] == expectedBodyCharacters && paragraphs >= 2) {
      if (paragraphs == stableParagraphCount) {
        stablePolls += 1;
      } else {
        stablePolls = 0;
        stableParagraphCount = paragraphs;
      }
      if (stablePolls >= 2) break;
    } else {
      stablePolls = 0;
      stableParagraphCount = null;
    }
    if (!DateTime.now().isBefore(deadline)) break;
    await Future<void>.delayed(const Duration(milliseconds: 100));
  }
  final renderedReady =
      rendered != null &&
      rendered['characters'] == expectedBodyCharacters &&
      (rendered['paragraphCount'] as num? ?? 0) >= 2 &&
      rendered['hash'] == expectedHash;
  // Real selection of the whole timeline region.
  final selected = _decodeDriverMap(
    await driver.requestData(
      'selection-select-all',
      timeout: const Duration(seconds: 30),
    ),
  );
  try {
    await File('${output.path}/stress-body-selection-highlight.png')
        .writeAsBytes(await driver.screenshot());
  } catch (_) {
    // Evidence capture must never replace the observation it documents.
  }
  // Real copy through the product context-menu callback, then the readback.
  final copied = _decodeDriverMap(
    await driver.requestData(
      'selection-copy',
      timeout: const Duration(seconds: 60),
    ),
  );
  final copiedText = copied?['copied'] as String?;
  final bodyContiguous =
      copiedText != null && copiedText.contains(expectedBody);
  // The canonical body extracted from the readback, so the summary can carry a
  // full hash plus head/tail of exactly the body slice, not just a boolean.
  final bodyOffset = copiedText?.indexOf(_bodyHeadMarker);
  final copiedBody =
      copiedText != null &&
          bodyOffset != null &&
          bodyOffset >= 0 &&
          bodyOffset + expectedBodyCharacters <= copiedText.length
      ? copiedText.substring(bodyOffset, bodyOffset + expectedBodyCharacters)
      : null;
  // The multi-paragraph proof must come from the copy's own instant, not from an
  // earlier poll, and the chunks it copied must concatenate to the canonical
  // body.
  final copiedParagraphCount = (copied?['paragraphCount'] as num? ?? 0).toInt();
  final copiedChunksMatchCanonical =
      copiedParagraphCount >= 2 &&
      copied?['characters'] == expectedBodyCharacters &&
      copied?['hash'] == expectedHash;
  // Cancel the selection with a real click, then re-read the region state: the
  // click must leave a collapsed (non-copyable) selection so the later history
  // and session-switch assertions run from a clean timeline.
  String? cancelClickError;
  var cancelClicked = false;
  try {
    await driver.rawTap(
      find.byValueKey('timeline-scrollable'),
      timeout: const Duration(seconds: 30),
    );
    cancelClicked = true;
  } catch (error) {
    cancelClickError = error.toString();
  }
  final selectionState = _decodeDriverMap(
    await driver.requestData(
      'selection-state',
      timeout: const Duration(seconds: 15),
    ),
  );
  final selectionCleared = selectionState?['copyable'] == false;
  final scrollAfter = _timelineScroll(await driver.readSnapshot());
  final pixelsAfter = _doubleOrNull(scrollAfter?['pixels']);
  // Cancelling the selection must not move the reader: the viewport pixels and
  // the follow-bottom state are compared before and after the whole phase.
  final scrollStable =
      pixelsBefore != null &&
      pixelsAfter != null &&
      (pixelsBefore - pixelsAfter).abs() <= 1.0 &&
      scrollBefore?['followingBottom'] == scrollAfter?['followingBottom'];
  final pass =
      copied?['ok'] == true &&
      copiedChunksMatchCanonical &&
      bodyContiguous &&
      selected?['copyable'] == true &&
      cancelClicked &&
      selectionCleared &&
      scrollStable;
  final evidence = <String, Object?>{
    'method':
        'SelectionArea selectAll + product context-menu copy callback + '
        'Clipboard.getData readback + real click cancel',
    'expectedCharacters': expectedBodyCharacters,
    'expectedHash': expectedHash,
    'expectedHead': _headOf(expectedBody),
    'expectedTail': _tailOf(expectedBody),
    'renderedParagraphCount': rendered?['paragraphCount'],
    'renderedChunkCount': rendered?['chunkCount'],
    'renderedCharacters': rendered?['characters'],
    'renderedHash': rendered?['hash'],
    'renderedHead': rendered?['head'],
    'renderedTail': rendered?['tail'],
    'bodyReadOk': rendered?['ok'],
    'renderedMatchesCanonicalBody': renderedReady,
    'selectOk': selected?['ok'],
    'copyButtonPresent': selected?['copyable'],
    'menuTypes': selected?['menuTypes'],
    'copyOk': copied?['ok'],
    'copiedParagraphCount': copiedParagraphCount,
    'copiedRenderedCharacters': copied?['characters'],
    'copiedRenderedHash': copied?['hash'],
    'copiedChunksMatchCanonical': copiedChunksMatchCanonical,
    'copiedLength': copiedText?.length,
    'copiedHead': copied?['copiedHead'],
    'copiedTail': copied?['copiedTail'],
    'copiedHash': copied?['copiedHash'],
    'bodyOffsetInClipboard': bodyOffset,
    'copiedBodyLength': copiedBody?.length,
    'copiedBodyHash': copiedBody == null
        ? null
        : _fnv1a64(copiedBody).toRadixString(16),
    'copiedBodyHead': copiedBody == null ? null : _headOf(copiedBody),
    'copiedBodyTail': copiedBody == null ? null : _tailOf(copiedBody),
    'copiedBodyMatchesCanonical': copiedBody == expectedBody,
    'copiedContainsCanonicalBody': bodyContiguous,
    'copyableAfterCopy': copied?['copyableAfterCopy'],
    'cancelClick': cancelClicked,
    'cancelClickError': cancelClickError,
    'copyableAfterCancelClick': selectionState?['copyable'],
    'menuTypesAfterCancelClick': selectionState?['menuTypes'],
    'selectionCleared': selectionCleared,
    'copyMillis': copied?['copyMillis'],
    'scrollPixelsBefore': pixelsBefore,
    'scrollPixelsAfter': pixelsAfter,
    'scrollStable': scrollStable,
    'pass': pass,
  };
  await File(
    '${output.path}/stress-body-selection-copy.json',
  ).writeAsString('${const JsonEncoder.withIndent('  ').convert(evidence)}\n');
  if (copiedText != null) {
    // The full readback stays as a plain file so a reviewer can diff it instead
    // of trusting the summary flags.
    await File('${output.path}/stress-body-selection-copied.txt')
        .writeAsString(copiedText);
  }
  return evidence;
}

/// The exact body the scripted provider streams: every increment is
/// `body-%05d ` (plus the large-scenario `W` pad), reconstructed here so the
/// clipboard comparison never trusts the delivered domain text.
String _canonicalBody(bool largeBody) {
  final buffer = StringBuffer();
  for (var ordinal = 0; ordinal < _expectedTokens; ordinal += 1) {
    buffer.write('body-${ordinal.toString().padLeft(5, '0')} ');
    if (largeBody) {
      buffer.write('W' * _largeBodyPad);
      buffer.write(' ');
    }
  }
  return buffer.toString();
}

Map<String, dynamic>? _decodeDriverMap(String raw) {
  final decoded = jsonDecode(raw);
  return decoded is Map<String, dynamic> ? decoded : null;
}

String _headOf(String text) =>
    text.substring(0, text.length < 64 ? text.length : 64);

String _tailOf(String text) =>
    text.substring(text.length < 64 ? 0 : text.length - 64);

/// FNV-1a over the code units. Must stay identical to the Driver-side helper.
int _fnv1a64(String text) {
  var hash = 0xcbf29ce484222325;
  for (var index = 0; index < text.length; index += 1) {
    hash ^= text.codeUnitAt(index);
    hash = hash * 0x100000001b3;
  }
  return hash;
}

/// Whether the reopened reading session has finished showing the full body and
/// finished restoring the reading anchor.
///
/// Deliberately does **not** compare the anchor to the original position: the
/// wait must observe the *same identity* body completing (exact length + head and
/// tail markers), no pending re-fetch for that identity, and the restore settling
/// (`restorePending`/`programmaticScroll` false whenever the UI exposes the
/// geometry). Waiting on anchor equality would let a position that never restores
/// pass by masking. A baseline without the `timelineScroll` diagnostic waits only
/// on the body and stays pending rather than passing.
bool _reopenBodyReady(
  Map<String, dynamic> snapshot,
  String expectedRowId,
  int expectedBodyCharacters,
  bool largeBody,
) {
  final window = snapshot['timelineWindow'];
  if (window is! Map) return false;
  if (_idList(window, 'pendingItemBodyIds').contains(expectedRowId)) {
    return false;
  }
  final row = _longBodyRow(snapshot);
  if (row == null || row['id'] != expectedRowId) return false;
  final text = row['text'] as String?;
  if (text == null || text.length != expectedBodyCharacters) return false;
  if (!text.startsWith(_bodyHeadMarker)) return false;
  if (!text.endsWith(_expectedBodySuffix(largeBody))) return false;
  final scroll = _timelineScroll(snapshot);
  if (scroll != null) {
    if (scroll['restorePending'] != false) return false;
    if (scroll['programmaticScroll'] != false) return false;
  }
  return true;
}

/// A bounded diagnostic of the reopened reading session for a wait timeout.
///
/// Records the same-identity body completeness (text length + head/tail), the
/// window's omission flags for that identity, and the restore/anchor geometry, so
/// a timeout states whether the body never completed or the restore never
/// finished instead of guessing.
Map<String, Object?> _reopenDiagnostics(
  Map<String, dynamic> snapshot,
  String expectedRowId,
  int expectedBodyCharacters,
  bool largeBody,
) {
  final window = snapshot['timelineWindow'];
  final scroll = _timelineScroll(snapshot);
  final row = _longBodyRow(snapshot);
  final text = row?['text'] as String?;
  return <String, Object?>{
    'expectedRowId': expectedRowId,
    'expectedCharacters': expectedBodyCharacters,
    'longBodyRowId': row?['id'],
    'longBodyCharacters': text?.length,
    'sameIdentity': row?['id'] == expectedRowId,
    'prefixOk': text != null && text.startsWith(_bodyHeadMarker),
    'suffixOk': text != null && text.endsWith(_expectedBodySuffix(largeBody)),
    'previewedItemId': _idList(
      window,
      'previewedItemIds',
    ).contains(expectedRowId),
    'loadedItemId': _idList(window, 'loadedItemIds').contains(expectedRowId),
    'pendingItemBodyId': _idList(
      window,
      'pendingItemBodyIds',
    ).contains(expectedRowId),
    'previewedItemIds': _idList(window, 'previewedItemIds'),
    'loadedItemIds': _idList(window, 'loadedItemIds'),
    'pendingItemBodyIds': _idList(window, 'pendingItemBodyIds'),
    'restorePending': scroll?['restorePending'],
    'programmaticScroll': scroll?['programmaticScroll'],
    'restoreAnchor': scroll?['restoreAnchor'],
    'anchor': scroll?['anchor'],
    'geometry': scroll == null
        ? null
        : <String, Object?>{
            'pixels': scroll['pixels'],
            'maxScrollExtent': scroll['maxScrollExtent'],
            'viewportDimension': scroll['viewportDimension'],
            'extentAfter': scroll['extentAfter'],
            'followingBottom': scroll['followingBottom'],
            'bottomSlack': scroll['bottomSlack'],
          },
    'rowIds': _rowIds(snapshot),
  };
}
