import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';
import 'window_control.dart';

/// Fully condition-driven realtime acceptance journey.
///
/// Every wait is a real observation of the GUI's Driver state: a new turn
/// identity, a new assistant row, a running tool, an approval prompt, a
/// cancelled or failed terminal, and the durable settled projection. Timing is
/// captured from wall-clock stamps taken at those observations, never from a
/// fixed sleep. The fixture only issues tool calls; the tools really run.
const _answers = <String, String>{
  'delayed': 'realtime delayed first token',
  'reasoning': 'realtime reasoning answer',
  'longCommand': 'realtime long command complete',
  'parallel': 'realtime parallel tools complete',
  'approval': 'realtime approval complete',
  'resume': 'realtime continued after cancel',
};

const _activeToolStatuses = <String>{
  'queued',
  'started',
  'streaming',
  'running',
  'approved',
  'cancelling',
};

/// Statuses that prove a tool is genuinely executing, not merely scheduled.
///
/// `queued` is deliberately excluded: two tools sharing a tool group can be
/// queued together without ever running at the same time, so concurrency is
/// only accepted from live statuses.
const _liveToolStatuses = <String>{'started', 'streaming', 'running'};

/// Requested narrow size for the wide/narrow layout observation. The journey
/// reads the window's real geometry first and restores exactly what it found;
/// it never assumes a default size.
const _narrowWindowWidth = 640;
const _narrowWindowHeight = 600;

/// At least this many pixels of content must remain below the viewport after
/// scrolling up, for the history reading position to count as detached from the
/// bottom rather than pinned to the last line.
const _minDetachedExtentAfter = 200.0;

/// Fixed pointer-drag step used to read up into history.
///
/// `FlutterDriver.scrollBy` drags the pointer by `dy`, so a positive step reveals
/// earlier content (a negative step keeps feeding content from below). The
/// timeline is a virtual list, so the oldest row may not be laid out at all and
/// must never be searched for by the driver. The step is deliberately finer than
/// the long-body stress step so a short assistant row cannot be stepped over.
const _historyScrollStep = 160.0;

/// Bounded number of history scroll steps; a view that never reaches a live
/// assistant row is reported from its real geometry instead of hanging.
const _maxHistoryScrollSteps = 60;

/// The activity details panel's documented height ceiling (`conversation_activity_bar.dart`).
const _maxActivityDetailsHeight = 320.0;

/// The minimum usable height of the expanded activity details panel.
///
/// The panel must show real, readable detail rather than a few clipped pixels:
/// roughly three text lines at the default body font (about 16 logical pixels per
/// line) is the smallest a reader can actually use. An expanded panel shorter than
/// this is a real layout failure, never evidence of a well-bounded panel.
const _minActivityDetailsHeight = 48.0;

/// Bounded deadline for observing that a window resize really reached Flutter.
///
/// Deliberately short: the applied size has to be read from real layout geometry,
/// never waited for by blocking on the app becoming idle. The live activity
/// spinner and the streamed text animate continuously, so an idle wait would burn
/// its whole deadline and never settle.
const _resizeSettleDeadline = Duration(seconds: 3);

/// Half a logical pixel of tolerance for rect comparisons; the Driver reports
/// logical pixels, so exact equality is not meaningful.
const _rectEpsilon = 0.5;

/// A widget rectangle read from the Driver geometry commands, in global (window)
/// logical pixels.
class DriverRect {
  const DriverRect({
    required this.key,
    required this.left,
    required this.top,
    required this.right,
    required this.bottom,
  });

  final String key;
  final double left;
  final double top;
  final double right;
  final double bottom;

  double get width => right - left;
  double get height => bottom - top;

  /// Whether [other] lies completely inside this rectangle.
  bool contains(DriverRect other) =>
      other.left >= left - _rectEpsilon &&
      other.top >= top - _rectEpsilon &&
      other.right <= right + _rectEpsilon &&
      other.bottom <= bottom + _rectEpsilon;

  Map<String, Object?> toJson() => <String, Object?>{
    'key': key,
    'left': left,
    'top': top,
    'right': right,
    'bottom': bottom,
    'width': width,
    'height': height,
  };
}

/// Whether two rectangles have the same size to within [_rectEpsilon]. Used to
/// confirm a resize really settled in the layout, never to compare positions.
bool _sameRectSize(DriverRect a, DriverRect b) =>
    (a.width - b.width).abs() <= _rectEpsilon &&
    (a.height - b.height).abs() <= _rectEpsilon;

const _terminalTurnStatuses = <String>{
  'completed',
  'failed',
  'cancelled',
  'budgetLimited',
};

Future<void> main(List<String> args) async {
  if (args.length != 4) {
    stderr.writeln(
      'usage: realtime_journey.dart VM_URL PROJECT_DIR OUTPUT_DIR COORD_DIR',
    );
    exitCode = 64;
    return;
  }
  // Connecting can fail before any journey state exists (bad VM URL, a GUI that
  // already exited, or a script error). Record a stage and a summary first so a
  // failure never leaves the coordinator with empty evidence.
  final output = Directory(args[2]);
  final coord = Directory(args[3]);
  final FlutterDriverSession driver;
  try {
    driver = await FlutterDriverSession.connect(vmServiceUrl: args[0]);
  } catch (error, stackTrace) {
    await _recordConnectFailure(output, coord, error);
    // `Error.throwWithStackTrace` never returns, so `driver` is definitely
    // assigned on every path that reaches the journey below.
    Error.throwWithStackTrace(error, stackTrace);
  }
  final journey = RealtimeJourney(
    driver: driver,
    project: args[1],
    output: output,
    coord: coord,
  );
  Object? failure;
  StackTrace? failureStack;
  Map<String, dynamic>? frames;
  try {
    await journey.run();
  } catch (error, stackTrace) {
    failure = error;
    failureStack = stackTrace;
    // Capture the failing observation before shutdown. The original error is
    // always preserved: a driver that already exited yields no extra evidence
    // instead of a second, misleading error.
    await _captureFailure(driver, output);
  }
  try {
    frames = await journey.stopFrameRecording();
  } catch (error) {
    journey.shutdownError ??= 'frame recording stop failed: $error';
  }
  Object? shutdownFailure;
  try {
    await _shutdown(driver);
  } catch (error) {
    shutdownFailure = error;
  }
  // Keep the stage marker consistent with the recorded failure: a shutdown
  // error must not leave the run looking complete.
  if (shutdownFailure != null) {
    journey.shutdownError ??= shutdownFailure.toString();
  }
  // Resolve the terminal stage before writing either artifact so the summary and
  // the stage file can never disagree: a clean run reports `complete`, a journey
  // exception keeps the exact stage it stopped at, and a shutdown-only failure
  // or a failed check reports a stage that names the failure.
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
    frames: frames,
  );
  await journey.mark(journey.stage);
  if (failure != null) {
    Error.throwWithStackTrace(failure, failureStack!);
  }
  await journey.raiseIfFailed(shutdownFailure);
}

Future<void> _shutdown(FlutterDriverSession driver) async {
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

/// Saves the last Driver snapshot and a screenshot for a failed journey.
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
    await File('${output.path}/realtime-failure-snapshot.json').writeAsString(
      '${const JsonEncoder.withIndent('  ').convert(snapshot)}\n',
    );
  } catch (_) {
    // Ignored: the summary still carries the original error and stage.
  }
  try {
    final png = await driver.screenshot().timeout(const Duration(seconds: 20));
    await File('${output.path}/realtime-failure.png').writeAsBytes(png);
  } catch (_) {
    // Ignored: see above.
  }
}

/// Records a stage marker and a minimal summary when the Driver cannot connect.
///
/// The coordinator reads both to report a specific stage and reason even though
/// no journey state was ever reached; the exit status and log come from the
/// parent process.
Future<void> _recordConnectFailure(
  Directory output,
  Directory coord,
  Object error,
) async {
  try {
    await File('${coord.path}/realtime-stage').writeAsString('connect_failed');
    final summary = <String, Object?>{
      'scenario': 'realtime',
      'verdict': 'pending',
      'status': 'failed',
      'stage': 'connect_failed',
      'error': error.toString(),
      'shutdownError': null,
      'failedChecks': const <String>[],
      'checks': const <String, Object?>{},
      'timings': const <String, Object?>{},
      'counts': const <String, Object?>{},
      'observations': const <String, Object?>{},
      'transitions': 0,
      'frameSampleCount': null,
      'completedAt': DateTime.now().toUtc().toIso8601String(),
      'pendingEvidence': const <String>[],
    };
    await File(
      '${output.path}/realtime-summary.json',
    ).writeAsString('${const JsonEncoder.withIndent('  ').convert(summary)}\n');
  } catch (_) {
    // Evidence directories are owned by the coordinator; if writing fails the
    // parent still captures the exit status and the sanitized Driver log.
  }
}

class SubmitContext {
  const SubmitContext({
    required this.priorTurnId,
    required this.priorContent,
    required this.priorReasoning,
    required this.priorAnswers,
    required this.submittedAt,
  });

  final String? priorTurnId;
  final int priorContent;
  final int priorReasoning;
  final int priorAnswers;
  final int submittedAt;
}

class RealtimeJourney {
  RealtimeJourney({
    required this.driver,
    required this.project,
    required this.output,
    required this.coord,
  });

  final FlutterDriverSession driver;
  final String project;
  final Directory output;
  final Directory coord;
  final List<Map<String, Object?>> transitions = <Map<String, Object?>>[];
  final Map<String, Object?> checks = <String, Object?>{};
  final Map<String, Object?> timings = <String, Object?>{};
  final Map<String, Object?> counts = <String, Object?>{};

  /// Structured evidence that is not a single per-transition snapshot (for
  /// example the long-command in-flight progress samples). It is written into
  /// the summary so the evidence survives instead of being dropped.
  final Map<String, Object?> observations = <String, Object?>{};
  String stage = 'created';
  String? shutdownError;

  /// The activity identity that was expanded during the reasoning stream, kept so
  /// the next live activity can prove a different identity starts collapsed.
  String? expandedActivityIdentity;

  /// Whether the previous live activity was left actually expanded, so the
  /// cross-identity reset has a real expanded reference to reset.
  bool activityLeftExpanded = false;

  /// Evidence that could not be asserted yet, kept explicit so a partial
  /// observation is never reported as a pass. The integrated UI contract adds
  /// `timelineScroll`/`conversationActivity`; when they are absent every related
  /// sub-check is recorded here instead of being asserted.
  final List<String> pendingEvidence = <String>[
    'parallel concurrency is observed from two live exec tools in one poll; a backend overlap counter is still outstanding',
    'in-flight tool stdout is asserted from the typed timeline output/progressBytes/itemRevision fields when the integrated Driver build projects them; a running tool is otherwise evidenced by its live status and screenshot, with its output verified only after delivery',
    'exec is a background tool: the scripted model only reports completion after it observed the task with the supported wait tool, so the model Turn terminal is recorded separately from the delivered command result and from the durable settled projection',
    'the parallel scenario waits on both command receipts; a single settled model step may consume both delivered results, so one answer is accepted for the batch',
    'first-content latency is measured from the Driver wall clock, not a provider TTFT field',
    'the provider fixture reports its own provider output bytes; those bytes are never an FRB transfer measurement',
  ];

  File get stageFile => File('${coord.path}/realtime-stage');

  bool get failed =>
      checks.values.any((value) => value != true) || shutdownError != null;

  Future<void> run() async {
    await mark('connected');
    await _openProject();
    await observe('project_opened');
    await driver.requestData(
      'frame-start',
      timeout: const Duration(seconds: 20),
    );
    counts['frameRecordingStartedAt'] = DateTime.now().millisecondsSinceEpoch;

    await _delayedFirstToken();
    await _reasoningThenAnswer();
    await _longCommand();
    await _parallelTools();
    await _approval();
    await _providerError();
    await _cancel();
    await _resume();
    await _uiContracts();
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
      // A terminal failure makes the awaited state unreachable; report the full
      // observation and the turn identity immediately instead of burning the
      // whole timeout and only then describing a stale snapshot.
      if (abort != null && abort(current)) {
        throw StateError('$label aborted: ${summarize(current)}');
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    final diagnostic = last == null ? const {} : summarize(last);
    throw StateError('timed out waiting for $label: $diagnostic');
  }

  Future<void> observe(String name, [Map<String, dynamic>? current]) async {
    final snapshot = current ?? await this.snapshot();
    final entry = <String, Object?>{
      'stage': name,
      'atUnixMillis': DateTime.now().millisecondsSinceEpoch,
      ...summarize(snapshot),
    };
    transitions.add(entry);
    await File(
      '${output.path}/realtime-$name.json',
    ).writeAsString('${const JsonEncoder.withIndent('  ').convert(entry)}\n');
    await File('${output.path}/realtime-transitions.json').writeAsString(
      '${const JsonEncoder.withIndent('  ').convert(transitions)}\n',
    );
  }

  Future<void> shot(String name) async {
    await File('${output.path}/realtime-$name.png')
        .writeAsBytes(await driver.screenshot());
  }

  Future<SubmitContext> submitPrompt(String prompt, String label) async {
    final before = await snapshot();
    final context = SubmitContext(
      priorTurnId: turnId(before),
      priorContent: assistantContentCount(before),
      priorReasoning: reasoningCount(before),
      priorAnswers: answerCount(before),
      submittedAt: 0,
    );
    await mark('${label}_submit');
    await _submit(prompt);
    return SubmitContext(
      priorTurnId: context.priorTurnId,
      priorContent: context.priorContent,
      priorReasoning: context.priorReasoning,
      priorAnswers: context.priorAnswers,
      submittedAt: DateTime.now().millisecondsSinceEpoch,
    );
  }

  Future<Map<String, dynamic>> awaitNewTurn(
    SubmitContext context,
    String label,
  ) {
    // A new turn identity is the only reliable signal that this submission was
    // accepted: a provider error or a cancelled stream can leave the busy flag
    // false before the next poll, so requiring `isBusy` here would miss the
    // turn entirely. Completion is handled separately by `awaitTerminal`.
    return waitFor(
      (snapshot) => turnId(snapshot) != context.priorTurnId,
      '${label}_running',
      timeout: const Duration(seconds: 30),
    );
  }

  Future<Map<String, dynamic>> awaitTerminal(String turn, String label) {
    return waitFor(
      (snapshot) =>
          turnId(snapshot) == turn &&
          _terminalTurnStatuses.contains(turnStatus(snapshot)),
      '${label}_terminal',
      timeout: const Duration(seconds: 120),
      abort: (snapshot) =>
          threadFaulted(snapshot) ||
          (turnId(snapshot) != turn && isDurableSettled(snapshot)),
    );
  }

  Future<Map<String, dynamic>> awaitSettled(String turn, String label) {
    return waitFor(
      (snapshot) => turnId(snapshot) == turn && isDurableSettled(snapshot),
      '${label}_settled',
      timeout: const Duration(seconds: 120),
      abort: (snapshot) =>
          threadFaulted(snapshot) ||
          turnStatus(snapshot) == 'failed' ||
          (turnId(snapshot) != turn && isDurableSettled(snapshot)),
    );
  }

  /// Waits for one model Turn to reach the `completed` terminal state.
  ///
  /// A failed, cancelled or superseded terminal makes that state unreachable, so
  /// the real observation is reported immediately instead of after the timeout.
  Future<Map<String, dynamic>> awaitCompleted(String turn, String label) {
    return waitFor(
      (snapshot) =>
          turnId(snapshot) == turn && turnStatus(snapshot) == 'completed',
      '${label}_terminal',
      timeout: const Duration(seconds: 120),
      abort: (snapshot) =>
          turnFailed(snapshot) ||
          (turnId(snapshot) != turn && isDurableSettled(snapshot)),
    );
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

  Future<void> _tap(String key) async {
    await driver.rawTap(
      find.byValueKey(key),
      timeout: const Duration(seconds: 30),
    );
  }

  Future<void> _delayedFirstToken() async {
    final context = await submitPrompt(
      'Realtime delayed first token',
      'delayed',
    );
    final waiting = await waitFor(
      (snapshot) =>
          turnId(snapshot) != context.priorTurnId &&
          assistantContentCount(snapshot) == context.priorContent,
      'delayed_waiting',
      timeout: const Duration(seconds: 20),
      abort: threadFaulted,
    );
    await observe('delayed_waiting', waiting);
    await shot('delayed-waiting');
    final turn = turnId(waiting)!;
    // First content is a new assistant row appearing while the Turn is live; a
    // terminal (not-busy) snapshot must never be read as the first token.
    final firstContent = await waitFor(
      (snapshot) =>
          turnId(snapshot) == turn &&
          assistantContentCount(snapshot) > context.priorContent,
      'delayed_first_content',
      timeout: const Duration(seconds: 30),
      abort: threadFaulted,
    );
    final firstContentAt = DateTime.now().millisecondsSinceEpoch;
    await observe('delayed_first_content', firstContent);
    await shot('delayed-streaming');
    final terminal = await awaitTerminal(turn, 'delayed');
    final terminalAt = DateTime.now().millisecondsSinceEpoch;
    await observe('delayed_terminal', terminal);
    final settled = await awaitSettled(turn, 'delayed');
    await observe('delayed_settled', settled);
    timings['delayed'] = {
      'submittedAt': context.submittedAt,
      'firstContentAt': firstContentAt,
      'terminalAt': terminalAt,
      'durableSettledAt': DateTime.now().millisecondsSinceEpoch,
      'timeToFirstContentMillis': firstContentAt - context.submittedAt,
      'terminalStatus': turnStatus(terminal),
      'outputTokens': outputTokens(settled),
    };
    checks['delayedFirstToken'] = (firstContentAt - context.submittedAt) >= 600;
    checks['delayedCompleted'] = answerContains(settled, _answers['delayed']!);
  }

  Future<void> _reasoningThenAnswer() async {
    final context = await submitPrompt(
      'Realtime reasoning then answer',
      'reasoning',
    );
    final running = await awaitNewTurn(context, 'reasoning');
    final turn = turnId(running)!;
    final reasoning = await waitFor(
      (snapshot) =>
          turnId(snapshot) == turn &&
          isBusy(snapshot) &&
          reasoningCount(snapshot) > context.priorReasoning &&
          answerCount(snapshot) == context.priorAnswers,
      'reasoning_in_progress',
      timeout: const Duration(seconds: 30),
      abort: threadFaulted,
    );
    await observe('reasoning_in_progress', reasoning);
    await shot('reasoning-in-progress');
    // The reasoning row is this turn's first content, so the time-to-first-content
    // stamp is taken here, before the activity-bar contract spends time observing.
    final firstContentAt = DateTime.now().millisecondsSinceEpoch;
    // The activity bar is exercised while reasoning is really running: only a
    // live activity exposes expandable details, and this is the long "Thinking"
    // window where the bar's latest summary line is on screen.
    await _activityContractDuringBusy('reasoning', narrowLayout: true);
    // The activity contract can consume most of the pacing window, so this
    // observation accepts either the answer row appearing or this turn reaching a
    // terminal — never an unbounded `isBusy` window that could already be gone.
    final streaming = await waitFor(
      (snapshot) =>
          turnId(snapshot) == turn &&
          (answerCount(snapshot) > context.priorAnswers ||
              _terminalTurnStatuses.contains(turnStatus(snapshot))),
      'reasoning_answer_streaming',
      timeout: const Duration(seconds: 30),
      abort: threadFaulted,
    );
    await observe('reasoning_answer_streaming', streaming);
    await shot('reasoning-answer-streaming');
    final terminal = await awaitTerminal(turn, 'reasoning');
    final terminalAt = DateTime.now().millisecondsSinceEpoch;
    final settled = await awaitSettled(turn, 'reasoning');
    await observe('reasoning_complete', settled);
    final rows = timelineRows(settled);
    final reasoningIndex = rows.indexWhere(
      (row) => row['type'] == 'reasoningSummary',
    );
    final answerIndex = rows.indexWhere(
      (row) =>
          row['type'] == 'finalAnswer' &&
          (row['text'] as String? ?? '').contains(_answers['reasoning']!),
    );
    timings['reasoning'] = {
      'submittedAt': context.submittedAt,
      'firstContentAt': firstContentAt,
      'terminalAt': terminalAt,
      'durableSettledAt': DateTime.now().millisecondsSinceEpoch,
      'timeToFirstContentMillis': firstContentAt - context.submittedAt,
      'terminalStatus': turnStatus(terminal),
      'reasoningRows': rows
          .where((row) => row['type'] == 'reasoningSummary')
          .length,
    };
    checks['reasoningThenAnswer'] =
        reasoningIndex >= 0 && answerIndex > reasoningIndex;
  }

  Future<void> _longCommand() async {
    final context = await submitPrompt('Realtime long command', 'long_command');
    final running = await awaitNewTurn(context, 'long_command');
    final turn = turnId(running)!;
    // `exec` is a background tool, so the real command is still running while
    // the model Turn stays live on the supported `wait` receipt. The command is
    // observed from its own live status, never inferred from the group.
    final toolRunning = await waitFor(
      (snapshot) => turnId(snapshot) == turn && activeExecCount(snapshot) >= 1,
      'long_command_tool_running',
      timeout: const Duration(seconds: 30),
      abort: turnFailed,
    );
    final toolRunningAt = DateTime.now().millisecondsSinceEpoch;
    await observe('long_command_tool_running', toolRunning);
    // One immediate loop, starting from the first running snapshot: it samples
    // the running call's typed stdout *and* the cross-identity activity reset in
    // the same poll, so neither is lost to the other's cost and no screenshot is
    // taken before the first live sample. The previous activity was left expanded
    // during reasoning, so this turn's tool activity must publish a different
    // identity collapsed. A version jump or a `running` status alone is never
    // accepted; the final line result is still verified below.
    await _longCommandLiveContract(toolRunning);

    // Delivery: the real command exited and its committed output is visible.
    // Only a delivered result may satisfy the output check; the model Turn
    // terminal is a separate fact and never stands in for the delivered command.
    final delivered = await waitFor(
      (snapshot) => execDelivered(
        snapshot,
        'realtime-long-call',
        'realtime-long-output-line',
      ),
      'long_command_tool_delivered',
      timeout: const Duration(seconds: 120),
      abort: turnFailed,
    );
    final deliveredAt = DateTime.now().millisecondsSinceEpoch;
    await observe('long_command_tool_delivered', delivered);
    await shot('long-command-tool-delivered');

    // The model Turn terminal and the durable settled projection are two
    // different facts and are recorded separately, never conflated.
    final terminal = await awaitCompleted(turn, 'long_command');
    final terminalAt = DateTime.now().millisecondsSinceEpoch;
    await observe('long_command_execution_terminal', terminal);
    final settled = await awaitSettled(turn, 'long_command');
    final settledAt = DateTime.now().millisecondsSinceEpoch;
    await observe('long_command_continuation', settled);
    await shot('long-command-continuation');

    final result = execTools(settled)
        .map((tool) => tool['result'] as String? ?? '')
        .join('\n');
    timings['longCommand'] = {
      'submittedAt': context.submittedAt,
      'toolRunningAt': toolRunningAt,
      'toolDeliveredAt': deliveredAt,
      'executionTerminalAt': terminalAt,
      'executionTerminalStatus': turnStatus(terminal),
      'durableSettledAt': settledAt,
      'commandDeliveredMillis': deliveredAt - context.submittedAt,
      'executionTerminalMillis': terminalAt - context.submittedAt,
    };
    counts['longCommandOutputLines'] = 'realtime-long-output-line'
        .allMatches(result)
        .length;
    checks['longCommandRunning'] = true;
    // The completed wording must not exist before the command delivered.
    checks['longCommandNoEarlyCompletion'] = !answerContains(
      toolRunning,
      _answers['longCommand']!,
    );
    checks['longCommandOutput'] =
        execDelivered(
          settled,
          'realtime-long-call',
          'realtime-long-output-line',
        ) &&
        (counts['longCommandOutputLines'] as int) > 1 &&
        answerContains(settled, _answers['longCommand']!);
  }

  /// One immediate, unified live-window loop for the long command.
  ///
  /// It starts from the first running snapshot — never after a screenshot — and
  /// samples two independent live facts in the *same* poll so neither can be lost
  /// to the other's cost:
  ///
  /// * the running `exec` call's typed in-flight stdout (`output`/
  ///   `progressBytes`/`itemRevision`) must grow across at least two samples of
  ///   the same call, which a revision jump alone, a bare `running` status, or
  ///   the final result read early can never satisfy; and
  /// * a *different* live activity identity must be published collapsed, i.e. the
  ///   expand state must reset when the activity identity switches.
  ///
  /// Both are recorded the moment they are observed. A build without the typed
  /// in-flight fields, or a run that never exposed the activity field, records an
  /// explicit pending gap instead of a pass.
  Future<void> _longCommandLiveContract(Map<String, dynamic> first) async {
    const callId = 'realtime-long-call';
    final previousIdentity = expandedActivityIdentity;
    final wantIdentityReset = previousIdentity != null && activityLeftExpanded;
    final samples = <Map<String, Object?>>[];
    var projectionHasFields = false;
    var activityFieldSeen = false;
    var observedGrowth = false;
    var identityResetObserved = false;
    Map<String, Object?>? previous;
    var latest = first;
    final deadline = DateTime.now().add(const Duration(seconds: 40));
    while (DateTime.now().isBefore(deadline)) {
      final snapshot = latest;
      // (a) Cross-identity reset: recorded the moment a different live identity
      // is published collapsed, never awaited after the window has closed.
      final activity = conversationActivity(snapshot);
      if (activity != null) activityFieldSeen = true;
      if (wantIdentityReset &&
          !identityResetObserved &&
          activity != null &&
          activity['identity'] is String &&
          activity['identity'] != previousIdentity &&
          activity['expanded'] == false) {
        identityResetObserved = true;
        // Only the lightweight observation file is written here: the loop must
        // not block on a screen capture (which costs seconds) before it samples
        // the running call's stdout. Every screenshot is taken after the loop.
        await observe('long_command_activity_identity_switch', snapshot);
        checks['activityIdentityResetsCollapse'] = true;
      }
      // (b) In-flight typed stdout growth of the same call identity.
      if (_timelineExposesInFlightToolFields(snapshot)) {
        projectionHasFields = true;
      }
      final sample = _liveToolProgressSample(snapshot, callId);
      if (sample != null) {
        samples.add(<String, Object?>{
          'at': DateTime.now().millisecondsSinceEpoch,
          ...sample,
        });
        if (previous != null &&
            (sample['progressBytes']! as int) >
                (previous['progressBytes']! as int) &&
            (sample['outputChars']! as int) >
                (previous['outputChars']! as int) &&
            (sample['itemRevision']! as int) >
                (previous['itemRevision']! as int)) {
          observedGrowth = true;
        }
        previous = sample;
      }
      final tool = _toolByCallId(snapshot, callId);
      // Once the call is terminal no further live sample or activity can arrive.
      final toolTerminal =
          tool != null && _terminalToolStatuses.contains(tool['status']);
      final growthSettled = observedGrowth || toolTerminal;
      final identitySettled =
          !wantIdentityReset || identityResetObserved || toolTerminal;
      if (growthSettled && identitySettled) break;
      await Future<void>.delayed(const Duration(milliseconds: 100));
      latest = await this.snapshot();
    }
    // Every screenshot is taken *after* the sampling loop, so its cost never
    // delays the first live observations. The activity frames show the state
    // current at capture time, never a claim to be the switch-time frame.
    if (samples.isNotEmpty) {
      await shot('long-command-tool-progress');
    }
    if (observedGrowth) {
      await shot('long-command-tool-progress-grown');
    }
    if (identityResetObserved) {
      observations['longCommandActivityIdentityFrame'] =
          'screenshot long-command-activity-identity-current.png is the state '
          'current after the live loop, not the switch-time frame';
      await shot('long-command-activity-identity-current');
    }
    counts['longCommandProgressSamples'] = samples.length;
    observations['longCommandProgress'] = samples;
    if (!projectionHasFields) {
      pendingEvidence.add(
        'long-command in-flight output: the timeline tool projection exposes no '
        'typed output/progressBytes/itemRevision fields, so the mid-flight stdout '
        'growth of $callId is pending an integrated Driver build',
      );
    } else {
      checks['longCommandInFlightOutputGrows'] = observedGrowth;
    }
    if (!wantIdentityReset) {
      if (!activityFieldSeen) {
        pendingEvidence.add(
          'activity bar: the snapshot exposes no conversationActivity field, so '
          'the cross-identity reset is pending an integrated Driver build',
        );
      } else {
        pendingEvidence.add(
          'activity bar: the live details were not left expanded by the previous '
          'activity, so the cross-identity reset has no expanded reference',
        );
      }
    } else if (!identityResetObserved) {
      // The window closed before a different identity was published collapsed.
      // This is a real failure, recorded as a failing check and the last real
      // observation instead of an unbounded wait that only times out.
      checks['activityIdentityResetsCollapse'] = false;
      observations['longCommandActivityIdentity'] = summarize(latest);
    }
  }

  Future<void> _parallelTools() async {
    final context = await submitPrompt('Realtime parallel tools', 'parallel');
    final running = await awaitNewTurn(context, 'parallel');
    final turn = turnId(running)!;
    // Two live tools must be observed together: a shared tool group alone does
    // not prove the commands overlapped.
    final concurrent = await waitFor(
      (snapshot) =>
          turnId(snapshot) == turn && liveExecStatuses(snapshot).length >= 2,
      'parallel_running',
      timeout: const Duration(seconds: 30),
      abort: turnFailed,
    );
    await observe('parallel_running', concurrent);
    await shot('parallel-tools-running');
    final liveAtObserve = liveExecStatuses(concurrent);

    // Wait for both real commands to exit before treating the work as done.
    bool bothDelivered(Map<String, dynamic> snapshot) =>
        execDelivered(
          snapshot,
          'realtime-parallel-call-a',
          'realtime-parallel-a-line',
        ) &&
        execDelivered(
          snapshot,
          'realtime-parallel-call-b',
          'realtime-parallel-b-line',
        );
    final delivered = await waitFor(
      bothDelivered,
      'parallel_tools_delivered',
      timeout: const Duration(seconds: 120),
      abort: turnFailed,
    );
    final deliveredAt = DateTime.now().millisecondsSinceEpoch;
    await observe('parallel_tools_delivered', delivered);
    await shot('parallel-tools-delivered');

    // The model Turn terminal is observed on its own, separately from the
    // durable settled projection below.
    final terminal = await awaitCompleted(turn, 'parallel');
    final terminalAt = DateTime.now().millisecondsSinceEpoch;
    await observe('parallel_execution_terminal', terminal);

    // The model waits on both background receipts, so the work is only complete
    // once the conversation settled with the delivered output of both commands.
    // A single settled answer may consume both delivered results: the runtime
    // can batch delivered results into one model step, so the check does not
    // require one provider request per command.
    final settled = await waitFor(
      (snapshot) =>
          bothDelivered(snapshot) &&
          isDurableSettled(snapshot) &&
          answerMatchCount(snapshot, _answers['parallel']!) >= 1,
      'parallel_continuation',
      timeout: const Duration(seconds: 120),
      abort: turnFailed,
    );
    final settledAt = DateTime.now().millisecondsSinceEpoch;
    await observe('parallel_complete', settled);
    await shot('parallel-complete');

    final result = execTools(settled)
        .map((tool) => tool['result'] as String? ?? '')
        .join('\n');
    timings['parallel'] = {
      'submittedAt': context.submittedAt,
      'toolDeliveredAt': deliveredAt,
      'executionTerminalAt': terminalAt,
      'executionTerminalStatus': turnStatus(terminal),
      'continuationSettledAt': settledAt,
      'liveStatusesAtObservation': liveAtObserve,
    };
    checks['parallelConcurrentRunning'] = liveAtObserve.length >= 2;
    checks['parallelNoEarlyCompletion'] = !answerContains(
      concurrent,
      _answers['parallel']!,
    );
    checks['parallelTools'] =
        bothDelivered(settled) &&
        result.contains('realtime-parallel-a') &&
        result.contains('realtime-parallel-b') &&
        answerMatchCount(settled, _answers['parallel']!) >= 1;
  }

  Future<void> _approval() async {
    final context = await submitPrompt('Realtime approval command', 'approval');
    final running = await awaitNewTurn(context, 'approval');
    final turn = turnId(running)!;
    final pending = await waitFor(
      (snapshot) =>
          turnId(snapshot) == turn &&
          interactionKind(snapshot) == 'toolApproval',
      'approval_pending',
      timeout: const Duration(seconds: 60),
      abort: turnFailed,
    );
    await observe('approval_pending', pending);
    await shot('approval-pending');
    // The gated command is a real background task, so the model Turn stays live
    // on the supported `wait` receipt while consent is outstanding: the
    // approval has to be granted before the Turn can finish, and the delivered
    // command output is the only thing that may satisfy the completion check.
    bool approved(Map<String, dynamic> snapshot) => execTools(snapshot).any(
      (tool) =>
          tool['callId'] == 'realtime-approval-call' &&
          tool['status'] == 'succeeded' &&
          (tool['result'] as String? ?? '').contains('realtime-approval-line'),
    );
    // Consent is requested before the model has claimed completion, and the
    // Turn has not failed while it waits on the gated task.
    checks['approvalHeldTurnOpen'] =
        !turnFailed(pending) && !answerContains(pending, _answers['approval']!);
    await driver.waitFor(
      find.byValueKey('tool-approve'),
      timeout: const Duration(seconds: 30),
    );
    await _tap('tool-approve');
    final approvedAt = DateTime.now().millisecondsSinceEpoch;
    final delivered = await waitFor(
      approved,
      'approval_tool_delivered',
      timeout: const Duration(seconds: 120),
      abort: turnFailed,
    );
    await observe('approval_tool_delivered', delivered);
    await shot('approval-tool-delivered');
    final terminal = await awaitCompleted(turn, 'approval');
    final settled = await waitFor(
      (snapshot) =>
          approved(snapshot) &&
          isDurableSettled(snapshot) &&
          answerContains(snapshot, _answers['approval']!),
      'approval_complete',
      timeout: const Duration(seconds: 120),
      abort: turnFailed,
    );
    await observe('approval_complete', settled);
    timings['approval'] = {
      'submittedAt': context.submittedAt,
      'approvedAt': approvedAt,
      'executionTerminalAt': DateTime.now().millisecondsSinceEpoch,
      'executionTerminalStatus': turnStatus(terminal),
      'terminalStatus': turnStatus(settled),
    };
    checks['approvalResolved'] =
        approved(settled) && answerContains(settled, _answers['approval']!);
  }

  Future<void> _providerError() async {
    final context = await submitPrompt(
      'Realtime provider error',
      'provider_error',
    );
    final running = await awaitNewTurn(context, 'provider_error');
    final turn = turnId(running)!;
    final failed = await waitFor(
      (snapshot) =>
          turnId(snapshot) == turn && turnStatus(snapshot) == 'failed',
      'provider_error',
      timeout: const Duration(seconds: 60),
    );
    final terminalAt = DateTime.now().millisecondsSinceEpoch;
    await observe('provider_error', failed);
    await shot('provider-error');
    timings['providerError'] = {
      'submittedAt': context.submittedAt,
      'terminalAt': terminalAt,
      'terminalStatus': 'failed',
      'durableSettledAt': null,
    };
    checks['providerErrorSurfaced'] = true;
  }

  Future<void> _cancel() async {
    final context = await submitPrompt('Realtime cancellable stream', 'cancel');
    final running = await awaitNewTurn(context, 'cancel');
    final turn = turnId(running)!;
    final streaming = await waitFor(
      (snapshot) => turnId(snapshot) == turn && isBusy(snapshot),
      'cancel_streaming',
      timeout: const Duration(seconds: 30),
    );
    await observe('cancel_streaming', streaming);
    await shot('cancel-streaming');
    await driver.waitFor(
      find.byValueKey('composer-stop'),
      timeout: const Duration(seconds: 30),
    );
    await _tap('composer-stop');
    final cancelled = await waitFor(
      (snapshot) =>
          turnId(snapshot) == turn && turnStatus(snapshot) == 'cancelled',
      'cancel_cancelled',
      timeout: const Duration(seconds: 30),
    );
    final cancelledAt = DateTime.now().millisecondsSinceEpoch;
    await observe('cancel_cancelled', cancelled);
    await shot('cancelled');
    timings['cancel'] = {
      'submittedAt': context.submittedAt,
      'terminalAt': cancelledAt,
      'terminalStatus': 'cancelled',
      'durableSettledAt': null,
    };
    checks['cancelSurfaced'] = true;
  }

  Future<void> _resume() async {
    final context = await submitPrompt(
      'Realtime continue after cancel',
      'resume',
    );
    final running = await awaitNewTurn(context, 'resume');
    final turn = turnId(running)!;
    // The resume reply is a single immediate response, so its live streaming
    // window can close between Driver polls. The observation is correlated to
    // *this* turn instead: either the scenario-unique answer text appears, or the
    // same turn reaches its terminal `completed` state. The answer text is an
    // identity that no earlier turn ever produced, and the terminal is read for
    // the correlated turn id only, so a previous turn's answer can never satisfy
    // this wait. Capturing the transient streaming state is not required here.
    final firstSeen = await waitFor(
      (snapshot) =>
          turnId(snapshot) == turn &&
          (answerContains(snapshot, _answers['resume']!) ||
              turnStatus(snapshot) == 'completed'),
      'resume_visible',
      timeout: const Duration(seconds: 30),
      abort: threadFaulted,
    );
    final firstContentAt = DateTime.now().millisecondsSinceEpoch;
    await observe('resume_visible', firstSeen);
    await shot('resume-visible');
    final observedStreaming = isBusy(firstSeen);
    final terminal = await awaitTerminal(turn, 'resume');
    final terminalAt = DateTime.now().millisecondsSinceEpoch;
    await observe('resume_terminal', terminal);
    final settled = await awaitSettled(turn, 'resume');
    await observe('resume_complete', settled);
    timings['resume'] = {
      'submittedAt': context.submittedAt,
      'firstContentAt': firstContentAt,
      'terminalAt': terminalAt,
      'durableSettledAt': DateTime.now().millisecondsSinceEpoch,
      'timeToFirstContentMillis': firstContentAt - context.submittedAt,
      'observedStreaming': observedStreaming,
      'terminalStatus': turnStatus(terminal),
    };
    checks['cancelThenContinue'] = answerContains(settled, _answers['resume']!);
  }

  /// Records the integrated activity-bar / timeline-scroll contracts.
  ///
  /// The fixture baseline predates the new `conversationActivity` and
  /// `timelineScroll` snapshot fields, so every observation is conditional: an
  /// absent field records an explicit pending note, and a present field is
  /// asserted for real. Nothing here fabricates a pass from an unknown field.
  Future<void> _uiContracts() async {
    await mark('ui_contracts');
    final settled = await snapshot();
    await shot('user-assistant-bubbles');

    // The activity bar is projected only while a turn is active, so the settled
    // projection carries no activity by design. The live contract is exercised
    // during the reasoning stream (expand / same-identity persistence / collapse)
    // and the cross-identity reset during the running tool; this terminal snapshot
    // must not be read as "the activity field is missing".
    pendingEvidence.add(
      'activity bar: live expand/persistence/collapse ran during the reasoning '
      'stream and the cross-identity reset during the running tool; the settled '
      'projection intentionally carries no activity',
    );
    pendingEvidence.add(
      'session switch: this scenario keeps one thread by design; multi-session '
      'switching is exercised by the stress-body journey',
    );

    await _contentDeliveryContract();
    await _windowResizeContract();

    final scroll = timelineScroll(settled);
    if (scroll == null) {
      pendingEvidence.add(
        'timeline scroll: snapshot exposes no timelineScroll field, so '
        'sticky-bottom, short-message and history-anchor checks are pending',
      );
      return;
    }
    final geometry = _scrollGeometry(scroll);
    if (geometry == null) {
      pendingEvidence.add(
        'timeline scroll: diagnostic present but not laid out '
        '(pixels/maxScrollExtent null), so geometry checks are pending',
      );
      return;
    }
    checks['timelineScrollGeometry'] = geometry.consistent;
    if (!geometry.consistent) {
      return;
    }
    // Short conversation: the content does not fill the viewport, so the column
    // is pinned to the bottom (zero scrollable extent, zero offset, slack below).
    if (geometry.maxScrollExtent == 0) {
      checks['shortMessageSticksBottom'] =
          geometry.pixels == 0 &&
          (geometry.bottomSlack > 0 || geometry.followingBottom);
    } else {
      // Scrollable conversation: at rest it must follow the bottom.
      // `extentAfter` is a public field, so it is read into a local before the
      // null check: field access cannot be promoted across the `||`.
      final extentAfter = geometry.extentAfter;
      checks['timelineFollowsBottom'] =
          geometry.followingBottom &&
          (extentAfter == null || extentAfter <= 1.0);
    }
    // Up-scrolling into history must keep the user's anchor instead of yanking
    // the view back to the latest row.
    if (geometry.maxScrollExtent > 0) {
      await _historyAnchorContract(settled);
    } else {
      pendingEvidence.add(
        'history anchor: conversation is shorter than the viewport, so '
        'up-scroll anchor retention is pending a longer transcript',
      );
    }
  }

  /// Narrows the GUI window through the driver's X11 window control, checks the
  /// layout is still usable and that the real window width really shrank, then
  /// restores the geometry read before the request.
  ///
  /// The window is located by the GUI process id (`_NET_WM_PID`), never by a
  /// title or class, so no unrelated window can be resized. A missing
  /// capability, an unprovable window ownership, or a width that did not shrink
  /// is recorded as pending rather than passed, together with the raw outcomes
  /// of the display queries so the reason can be read from the evidence. A real
  /// render overflow is left to the coordinator's GUI-health gate, which reads
  /// the captured GUI log.
  Future<void> _windowResizeContract() async {
    final guiPid = await _guiProcessId();
    if (guiPid == null) {
      pendingEvidence.add(
        'window resize: the driver did not expose the GUI process id, so the '
        'narrow/wide layout observation is pending',
      );
      return;
    }
    final wideShell = await _readRect('studio-shell');
    final narrow = resizeOwnedWindow(
      guiPid: guiPid,
      width: _narrowWindowWidth,
      height: _narrowWindowHeight,
    );
    if (!narrow.resized) {
      await _writeResizeDiagnostics(narrow, null);
      pendingEvidence.add(
        'window resize: driver could not narrow the GUI window '
        '(${narrow.reason}); raw outcomes: [${narrow.diagnostics.join(' | ')}]; '
        'wide/narrow layout is pending',
      );
      return;
    }
    final narrowedShell = await _settleAfterResize(
      wideShell,
      expectNarrower: true,
    );
    final narrowSnapshot = await snapshot();
    await shot('window-narrow');
    // The core layout must still be present at the narrow width.
    await driver.waitFor(
      find.byValueKey('composer-input'),
      timeout: const Duration(seconds: 10),
    );
    await driver.waitFor(
      find.byValueKey('timeline-scrollable'),
      timeout: const Duration(seconds: 10),
    );
    checks['narrowWindowUsable'] = true;
    final narrowWidth = narrow.applied!.width;
    final narrowViewport = _viewportDimension(timelineScroll(narrowSnapshot));

    final original = narrow.original!;
    final restored = resizeOwnedWindow(
      guiPid: guiPid,
      width: original.width,
      height: original.height,
    );
    final restoredShell = await _settleAfterResize(
      narrowedShell ?? wideShell,
      expectNarrower: false,
    );
    final restoredSnapshot = await snapshot();
    await shot('window-restored');
    final restoredWidth = restored.resized ? restored.applied!.width : null;
    final restoredViewport = _viewportDimension(
      timelineScroll(restoredSnapshot),
    );
    await _writeResizeDiagnostics(narrow, restored);
    if (restoredWidth == null) {
      pendingEvidence.add(
        'window resize: the original size ${original.width}x${original.height} '
        'could not be restored (${restored.reason}); raw outcomes: '
        '[${restored.diagnostics.join(' | ')}]; the window width change is pending',
      );
    } else if (narrowWidth < restoredWidth) {
      checks['narrowWindowNarrower'] = true;
    } else {
      // The X resize request was accepted but the window manager kept the width;
      // that is an environment limitation, not a UI defect, so it is recorded
      // with the measured numbers instead of failing the gate.
      pendingEvidence.add(
        'window resize: the window width did not shrink after the narrow '
        'request (narrow=$narrowWidth restored=$restoredWidth); the window '
        'manager may be ignoring the request',
      );
    }
    timings['windowResize'] = {
      'guiPid': guiPid,
      'windowId': narrow.windowId,
      'originalGeometry': original.toJson(),
      'narrowGeometry': narrow.applied?.toJson(),
      'restoredGeometry': restored.applied?.toJson(),
      'narrowWindowWidth': narrowWidth,
      'restoredWindowWidth': restoredWidth,
      'wideShellGeometry': wideShell?.toJson(),
      'narrowShellGeometry': narrowedShell?.toJson(),
      'restoredShellGeometry': restoredShell?.toJson(),
      'narrowViewportDimension': narrowViewport,
      'restoredViewportDimension': restoredViewport,
      'narrowDiagnostics': narrow.diagnostics,
      'restoredDiagnostics': restored.diagnostics,
    };
  }

  /// Records the Driver-only content-delivery counters.
  ///
  /// `contentDelivery.bodyUtf8Bytes` counts UTF-8 bytes of the body text the UI
  /// actually delivered, an *application payload* measure. It is deliberately
  /// never an FRB/wire byte count and is kept apart from the fixture's synthetic
  /// provider output bytes.
  Future<void> _contentDeliveryContract() async {
    final delivery = contentDelivery(await snapshot());
    if (delivery == null) {
      pendingEvidence.add(
        'content delivery: snapshot exposes no contentDelivery field, so the '
        'application-payload byte counters are pending an integrated Driver '
        'build; the provider output bytes are never used as a wire measure',
      );
      return;
    }
    if (delivery['enabled'] != true) {
      pendingEvidence.add(
        'content delivery: contentDelivery is present but disabled '
        '(enabled=${delivery['enabled']}), so no application-payload counters '
        'were collected',
      );
      return;
    }
    counts['contentDelivery'] = delivery;
    checks['contentDeliveryLabeled'] =
        delivery['metric'] == 'application-payload';
    pendingEvidence.add(
      'content delivery: contentDelivery.bodyUtf8Bytes is an '
      'application-payload UTF-8 measure of the delivered body text; it is not '
      'FRB/wire bytes and stays separate from the fixture provider output bytes',
    );
  }

  /// Records which display the resize used and, on refusal, the raw
  /// `xprop`/`xwininfo`/Xlib outcomes, so a pending narrow/wide observation can
  /// be diagnosed from the evidence instead of guessed.
  Future<void> _writeResizeDiagnostics(
    WindowResizeResult narrow,
    WindowResizeResult? restored,
  ) async {
    final lines = <String>[
      'narrow resized=${narrow.resized} windowId=${narrow.windowId} '
          'reason=${narrow.reason}',
      for (final line in narrow.diagnostics) 'narrow $line',
      if (restored != null)
        'restored resized=${restored.resized} windowId=${restored.windowId} '
            'reason=${restored.reason}',
      if (restored != null)
        for (final line in restored.diagnostics) 'restored $line',
    ];
    try {
      await File('${output.path}/window-resize-diagnostics.txt')
          .writeAsString('${lines.join('\n')}\n');
    } on Object {
      // Evidence writes live under the coordinator's directory and must not
      // mask the resize outcome itself.
    }
  }

  /// Reads the GUI process id from the Flutter Driver extension so the X11
  /// window can be matched by `_NET_WM_PID` instead of by title or class.
  Future<int?> _guiProcessId() async {
    try {
      final raw = await driver.requestData(
        'pid',
        timeout: const Duration(seconds: 15),
      );
      final decoded = jsonDecode(raw);
      final pid = decoded is Map ? decoded['pid'] : null;
      if (pid is int && pid > 0) return pid;
      pendingEvidence.add(
        'window resize: the driver pid response was not a process id '
        '(${raw.trim()})',
      );
    } on Object catch (error) {
      pendingEvidence.add(
        'window resize: the driver pid request failed ($error)',
      );
    }
    return null;
  }

  /// Waits, bounded by [_resizeSettleDeadline], for a resize to reach Flutter's
  /// real layout.
  ///
  /// The window's own content never idles: the live activity spinner and the
  /// streamed text produce frames continuously, so an "app is settled" wait
  /// (`waitForNoPendingFrame`) would spend its whole deadline and never return.
  /// The applied size is instead read from real `studio-shell` geometry, which
  /// changes once Flutter relayouts for the new window size; the screenshot taken
  /// right afterwards rasterizes the current frame. The poll is short and bounded,
  /// and it never treats "still animating" as "not applied".
  Future<DriverRect?> _settleAfterResize(
    DriverRect? before, {
    required bool expectNarrower,
  }) async {
    final deadline = DateTime.now().add(_resizeSettleDeadline);
    DriverRect? applied;
    while (DateTime.now().isBefore(deadline)) {
      final current = await _readRect('studio-shell');
      if (current != null && current.width > 0 && current.height > 0) {
        final changed =
            before == null ||
            (current.width - before.width).abs() > _rectEpsilon ||
            (current.height - before.height).abs() > _rectEpsilon;
        final directionOk =
            before == null ||
            (expectNarrower
                ? current.width <= before.width + _rectEpsilon
                : current.width >= before.width - _rectEpsilon);
        if (changed && directionOk) {
          if (applied != null && _sameRectSize(applied, current)) {
            return current;
          }
          applied = current;
        }
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    return applied ?? await _readRect('studio-shell');
  }

  /// Writes one evidence JSON file under the output directory.
  Future<void> _writeJson(String name, Object? data) async {
    try {
      await File(
        '${output.path}/$name',
      ).writeAsString('${const JsonEncoder.withIndent('  ').convert(data)}\n');
    } on Object {
      // Evidence writes live under the coordinator's directory and must never
      // mask the observation they document.
    }
  }

  /// Reads a widget's real rectangle through the Driver's own geometry commands.
  ///
  /// Read-only: the geometry commands never dispatch a pointer event. A widget
  /// that is not laid out reports `null` instead of holding a Driver command.
  Future<DriverRect?> _readRect(String key) async {
    final finder = find.byValueKey(key);
    try {
      final topLeft = await driver.getTopLeft(finder);
      final bottomRight = await driver.getBottomRight(finder);
      return DriverRect(
        key: key,
        left: topLeft.dx,
        top: topLeft.dy,
        right: bottomRight.dx,
        bottom: bottomRight.dy,
      );
    } on Object {
      return null;
    }
  }

  /// Records the real rectangles while the activity details are expanded and
  /// asserts the composer and its action button are still completely inside the
  /// window viewport and that the details panel stays bounded.
  ///
  /// Existing is not the same as usable: each rect is read from the live layout,
  /// and a rect that cannot be read at all fails the check with the reason.
  Future<void> _activityLayoutContract(String label) async {
    final viewport = await _readRect('studio-shell');
    final composer = await _readRect('composer-input');
    final actionKey = isBusy(await snapshot())
        ? 'composer-stop'
        : 'composer-submit';
    final action = await _readRect(actionKey);
    final details = await _readRect('conversation-activity-details');
    final evidence = <String, Object?>{
      'viewport': viewport?.toJson(),
      'composerInput': composer?.toJson(),
      'actionButtonKey': actionKey,
      'actionButton': action?.toJson(),
      'details': details?.toJson(),
      'detailsHeightCeiling': _maxActivityDetailsHeight,
      'detailsHeightFloor': _minActivityDetailsHeight,
    };
    await _writeJson('$label-activity-layout.json', evidence);
    final readable =
        viewport != null &&
        composer != null &&
        action != null &&
        details != null;
    checks['${label}_activityLayoutReadable'] = readable;
    checks['${label}_composerInsideViewport'] =
        readable && viewport.contains(composer);
    checks['${label}_actionButtonInsideViewport'] =
        readable && viewport.contains(action);
    // The details panel is bounded exactly like the UI documents: at most the
    // fixed ceiling, and never taller than the window itself.
    checks['${label}_detailsBounded'] =
        readable &&
        details.height > 0 &&
        details.height <= _maxActivityDetailsHeight + _rectEpsilon &&
        details.height <= viewport.height + _rectEpsilon &&
        viewport.contains(details);
    // Bounded is not the same as usable: the expanded panel must show real,
    // readable detail (at least ~3 text lines), so a few clipped pixels cannot
    // pass on existence plus an upper bound alone.
    checks['${label}_detailsUsable'] =
        readable && details.height >= _minActivityDetailsHeight - _rectEpsilon;
  }

  /// Narrows the real GUI window while the activity details are expanded, records
  /// the narrow layout evidence, then restores exactly the geometry read before
  /// the request. The restore runs even when the narrow observation fails.
  Future<void> _narrowDuringActivity(String label) async {
    final guiPid = await _guiProcessId();
    if (guiPid == null) {
      pendingEvidence.add(
        'activity layout: the driver did not expose the GUI process id, so the '
        'narrow-window layout evidence is pending',
      );
      return;
    }
    // Real shell geometry before the request, so the narrow layout can be proven
    // to have reached Flutter rather than waited for by blocking on idle frames.
    final beforeShell = await _readRect('studio-shell');
    final narrow = resizeOwnedWindow(
      guiPid: guiPid,
      width: _narrowWindowWidth,
      height: _narrowWindowHeight,
    );
    if (!narrow.resized) {
      await _writeResizeDiagnostics(narrow, null);
      pendingEvidence.add(
        'activity layout: driver could not narrow the GUI window '
        '(${narrow.reason}); raw outcomes: [${narrow.diagnostics.join(' | ')}]; '
        'the narrow layout evidence is pending',
      );
      return;
    }
    final original = narrow.original!;
    DriverRect? narrowedShell;
    try {
      narrowedShell = await _settleAfterResize(
        beforeShell,
        expectNarrower: true,
      );
      counts['activityNarrowShellWidth'] = narrowedShell?.width;
      await shot('$label-activity-narrow');
      await _activityLayoutContract('${label}_narrow');
    } finally {
      final restored = resizeOwnedWindow(
        guiPid: guiPid,
        width: original.width,
        height: original.height,
      );
      final restoredShell = await _settleAfterResize(
        narrowedShell ?? beforeShell,
        expectNarrower: false,
      );
      counts['activityRestoredShellWidth'] = restoredShell?.width;
      if (!restored.resized) {
        pendingEvidence.add(
          'activity layout: the original window size '
          '${original.width}x${original.height} could not be restored '
          '(${restored.reason}); raw outcomes: '
          '[${restored.diagnostics.join(' | ')}]',
        );
      }
    }
  }

  /// Expands the live activity bar, keeps it expanded across streaming updates
  /// of the same identity, collapses it, then leaves it expanded again so the
  /// cross-identity reset can be observed on the next live activity.
  ///
  /// Only a snapshot without the typed `conversationActivity` field is pending (a
  /// baseline gap). Once the field exists, every observation below is asserted
  /// for real: the activity must expose an identity, be `expandable`, load its
  /// on-demand details, keep the same identity expanded across a *new* update,
  /// collapse to `expanded=false`, and stay collapsed across a further update.
  Future<void> _activityContractDuringBusy(
    String label, {
    bool narrowLayout = false,
  }) async {
    await mark('${label}_activity_expand');
    final snapshot = await this.snapshot();
    if (!snapshot.containsKey('conversationActivity')) {
      pendingEvidence.add(
        'activity bar: the snapshot exposes no conversationActivity field during '
        '$label, so expand/collapse is pending an integrated Driver build',
      );
      return;
    }
    // The typed field exists: wait for the live, expandable activity of this
    // stream. A missing one is a real behaviour failure, not a pending gap.
    final live = await waitFor(
      (current) {
        final activity = conversationActivity(current);
        return activity != null &&
            activity['identity'] is String &&
            activity['expandable'] == true;
      },
      '${label}_activity_expandable',
      timeout: const Duration(seconds: 20),
      abort: turnFailed,
    );
    final identity = conversationActivity(live)!['identity'] as String;
    checks['activityIdentityExposed'] = true;
    checks['activityExpandableDuringActiveTurn'] = true;
    // `expandable` is the capability that makes the bar tappable; the complete
    // details are read on demand, so they are only asserted after the tap.
    await _tap('conversation-activity');
    await driver.waitFor(
      find.byValueKey('conversation-activity-details'),
      timeout: const Duration(seconds: 15),
    );
    // `expanded` is the *actual* expand state published by the activity bar;
    // `expandable` is only the capability and is never read as the state. The
    // details list is populated by the on-expand load, so a non-empty list here
    // is the real load result.
    final expanded = await waitFor(
      (current) {
        final activity = conversationActivity(current);
        final details = activity == null ? null : activity['details'];
        return activity != null &&
            activity['identity'] == identity &&
            activity['expanded'] == true &&
            details is List &&
            details.isNotEmpty;
      },
      '${label}_activity_expanded',
      timeout: const Duration(seconds: 20),
      abort: turnFailed,
    );
    await observe('${label}_activity_expanded', expanded);
    await shot('$label-activity-expanded');
    await driver.waitFor(
      find.byValueKey('conversation-activity-details'),
      timeout: const Duration(seconds: 5),
    );
    final expandedView = conversationActivity(expanded);
    // `expandable` (capability) is recorded for evidence only: the assertions use
    // the actual `expanded` state.
    counts['activityExpandableCapability'] = expandedView?['expandable'];
    // The same identity must stay expanded across a *new* update. The revision
    // token must advance, so this is never a re-read of the same snapshot. This is
    // gathered BEFORE the window resize: the resize is the slowest observation, so
    // the revision evidence is taken first and every phase stays tied to this
    // exact activity identity instead of racing the stream window.
    final revisionBefore = _streamingRevision(expanded);
    final advanced = await waitFor(
      (current) {
        final activity = conversationActivity(current);
        return activity != null &&
            activity['identity'] == identity &&
            _streamingRevision(current) > revisionBefore;
      },
      '${label}_activity_update',
      timeout: const Duration(seconds: 20),
      abort: turnFailed,
    );
    checks['activityExpandedAcrossUpdates'] =
        conversationActivity(advanced)?['expanded'] == true;
    // The activity identity the update was confirmed against is the same one left
    // expanded for the cross-identity reset.
    expandedActivityIdentity = identity;
    counts['activityRevisionBefore'] = revisionBefore;
    counts['activityRevisionAfter'] = _streamingRevision(advanced);
    // The real expanded layout is recorded while the details are open: the wide
    // window first, then the narrow window (restored immediately after). Both are
    // still on the same identity that was just confirmed expanded across updates.
    await _activityLayoutContract('${label}_wide');
    if (narrowLayout) {
      await _narrowDuringActivity(label);
    }
    // Collapse: it must publish the real `expanded=false` state, not merely hide
    // the details widget.
    await _tap('conversation-activity');
    await driver.waitForAbsent(
      find.byValueKey('conversation-activity-details'),
      timeout: const Duration(seconds: 15),
    );
    final collapsed = await waitFor(
      (current) {
        final activity = conversationActivity(current);
        return activity != null &&
            activity['identity'] == identity &&
            activity['expanded'] == false;
      },
      '${label}_activity_collapsed',
      timeout: const Duration(seconds: 10),
      abort: turnFailed,
    );
    await shot('$label-activity-collapsed');
    checks['activityCollapse'] =
        conversationActivity(collapsed)?['expanded'] == false;
    // Collapsing must persist across a further update of the same identity: the
    // revision must advance and the state must stay `expanded=false`.
    final collapseRevision = _streamingRevision(collapsed);
    final recollapsed = await waitFor(
      (current) {
        final activity = conversationActivity(current);
        return activity != null &&
            activity['identity'] == identity &&
            activity['expanded'] == false &&
            _streamingRevision(current) > collapseRevision;
      },
      '${label}_activity_collapsed_update',
      timeout: const Duration(seconds: 20),
      abort: turnFailed,
    );
    checks['activityCollapsePersists'] =
        conversationActivity(recollapsed)?['expanded'] == false;
    counts['activityCollapseRevisionAfter'] = _streamingRevision(recollapsed);
    // Leave the live activity expanded again so the cross-identity contract
    // observes a real reset on the next activity rather than asserting that an
    // already-collapsed bar stays collapsed.
    await _tap('conversation-activity');
    final reexpanded = await waitFor(
      (current) {
        final activity = conversationActivity(current);
        return activity != null &&
            activity['identity'] == identity &&
            activity['expanded'] == true;
      },
      '${label}_activity_reexpanded',
      timeout: const Duration(seconds: 10),
      abort: turnFailed,
    );
    await shot('$label-activity-reexpanded');
    checks['activityExpandedBeforeSwitch'] =
        conversationActivity(reexpanded)?['expanded'] == true;
    activityLeftExpanded = true;
  }

  /// Scrolls up into history and confirms the user's anchor is retained.
  ///
  /// The timeline is a virtual list: the oldest row may not be laid out at all,
  /// so `scrollUntilVisible` would keep searching for a row that never appears and
  /// leave a Driver command in flight past its timeout. The journey therefore
  /// drags the pointer in fixed bounded steps — the same fixed-step, live-geometry
  /// mechanism the long-body stress journey uses — until the view has really left
  /// the bottom while a live assistant row is the topmost visible row, then
  /// confirms that anchor does not move on a later poll.
  Future<void> _historyAnchorContract(Map<String, dynamic> settled) async {
    if (_assistantRowIds(settled).isEmpty) {
      pendingEvidence.add(
        'history anchor: no assistant final-answer row identity is available in '
        'the current window to anchor on',
      );
      return;
    }
    final settledScroll = timelineScroll(settled);
    final before = settledScroll == null
        ? null
        : _scrollGeometry(settledScroll);
    var current = settled;
    var geometry = before;
    var steps = 0;
    // Per-step live geometry, including the UI10 drag diagnostics, so a reviewer
    // can tell whether the pointer drags ever reached the scroll view
    // (`userDragUpdates`) or were moved by a programmatic restore
    // (`programmaticScroll`).
    final history = <Map<String, Object?>>[
      <String, Object?>{'phase': 'start', ..._historyStep(current)},
    ];
    try {
      // A positive `dy` drags the finger down to reveal earlier content; a
      // negative value would keep feeding content from below. Every step is
      // bounded, so a stuck view reports real geometry instead of hanging.
      while (steps < _maxHistoryScrollSteps) {
        final anchor = _anchorId(timelineScroll(current));
        if (_leftBottom(geometry) &&
            anchor != null &&
            _assistantRowIds(current).contains(anchor)) {
          break;
        }
        await driver.scrollBy(
          find.byValueKey('timeline-scrollable'),
          _historyScrollStep,
          timeout: const Duration(seconds: 15),
        );
        current = await snapshot();
        geometry = _scrollGeometryOf(current);
        steps++;
        history.add(<String, Object?>{
          'phase': 'after_scroll_$steps',
          ..._historyStep(current),
        });
      }
    } catch (error) {
      await _writeJson('realtime-history-anchor.json', <String, Object?>{
        'steps': history,
        'error': '$error',
      });
      pendingEvidence.add(
        'history anchor: Driver could not scroll the timeline ($error)',
      );
      return;
    }
    await _writeJson('realtime-history-anchor.json', <String, Object?>{
      'steps': history,
      'final': _historyStep(current),
    });
    await observe('history_detached', current);
    await shot('history-detached');
    final scroll = timelineScroll(current);
    final anchorBefore = _anchorId(scroll);
    // Leaving the bottom is proven from the live geometry, not from the scroll
    // request: the viewport moved up (pixels decreased) with content still below.
    final detached =
        _leftBottom(geometry) &&
        before != null &&
        geometry != null &&
        geometry.pixels < before.pixels - 1.0;
    checks['historyScrollDetachesBottom'] = detached;
    // The anchor row must be one of the assistant final-answer rows present in
    // the window the observation came from, so a user or tool row cannot stand in
    // for the reading position.
    checks['historyAssistantAnchor'] =
        detached &&
        anchorBefore != null &&
        _assistantRowIds(current).contains(anchorBefore);
    counts['historyScrollSteps'] = steps;
    counts['historyAnchorOffset'] = _anchorOffset(scroll);
    counts['historyDetachedByUser'] = scroll?['detachedByUser'];
    counts['historyProgrammaticScroll'] = scroll?['programmaticScroll'];
    counts['historyUserDragUpdates'] = scroll?['userDragUpdates'];
    counts['historyPixelsBefore'] = before?.pixels;
    counts['historyPixelsAfter'] = geometry?.pixels;
    counts['historyExtentAfter'] = geometry?.extentAfter;
    // A later poll must not move the anchor the user is reading.
    final after = await snapshot();
    final anchorAfter = _anchorId(timelineScroll(after));
    checks['historyAnchorRetained'] =
        anchorBefore != null && anchorBefore == anchorAfter;
  }

  bool _leftBottom(_ScrollGeometry? geometry) =>
      geometry != null &&
      geometry.followingBottom == false &&
      (geometry.extentAfter ?? 0) > _minDetachedExtentAfter;

  _ScrollGeometry? _scrollGeometryOf(Map<String, dynamic> snapshot) {
    final scroll = timelineScroll(snapshot);
    return scroll == null ? null : _scrollGeometry(scroll);
  }

  /// One history-step observation: the live scroll geometry plus the UI10 drag
  /// diagnostics that say whether a real drag reached the scroll view.
  Map<String, Object?> _historyStep(Map<String, dynamic> snapshot) {
    final scroll = timelineScroll(snapshot);
    return <String, Object?>{
      'anchorItemId': _anchorId(scroll),
      'anchorOffset': _anchorOffset(scroll),
      'pixels': _num(scroll?['pixels']),
      'maxScrollExtent': _num(scroll?['maxScrollExtent']),
      'extentAfter': _num(scroll?['extentAfter']),
      'followingBottom': scroll?['followingBottom'],
      'detachedByUser': scroll?['detachedByUser'],
      'programmaticScroll': scroll?['programmaticScroll'],
      'userDragUpdates': scroll?['userDragUpdates'],
    };
  }

  Future<Map<String, dynamic>?> stopFrameRecording() async {
    if (counts['frameRecordingStartedAt'] == null) return null;
    final frames = jsonDecode(
      await driver.requestData(
        'frame-stop',
        timeout: const Duration(seconds: 20),
      ),
    );
    if (frames is! Map<String, dynamic>) return null;
    await File(
      '${output.path}/realtime-frames.json',
    ).writeAsString('${const JsonEncoder.withIndent('  ').convert(frames)}\n');
    counts['frames'] = frames['frames'];
    return frames;
  }

  Future<void> writeSummary({
    required Object? failure,
    required Object? shutdownFailure,
    required Map<String, dynamic>? frames,
  }) async {
    final failedChecks = checks.entries
        .where((entry) => entry.value != true)
        .map((entry) => entry.key)
        .toList(growable: false);
    // `frames` is nullable, so index it with the null-aware operator and narrow
    // the sample list explicitly instead of relying on promotion in a branch.
    final Object? frameSamples = frames?['samples'];
    final summary = <String, Object?>{
      'scenario': 'realtime',
      'verdict': 'pending',
      'status': failure == null && failedChecks.isEmpty ? 'complete' : 'failed',
      'stage': stage,
      'error': failure?.toString(),
      'shutdownError': shutdownFailure?.toString(),
      'failedChecks': failedChecks,
      'checks': checks,
      'timings': timings,
      'counts': counts,
      'observations': observations,
      'transitions': transitions.length,
      'frameSampleCount': frameSamples is List ? frameSamples.length : null,
      'completedAt': DateTime.now().toUtc().toIso8601String(),
      'pendingEvidence': pendingEvidence,
    };
    await File(
      '${output.path}/realtime-summary.json',
    ).writeAsString('${const JsonEncoder.withIndent('  ').convert(summary)}\n');
  }

  Future<void> raiseIfFailed(Object? shutdownFailure) async {
    final failedChecks = checks.entries
        .where((entry) => entry.value != true)
        .map((entry) => entry.key)
        .toList(growable: false);
    if (failedChecks.isNotEmpty) {
      throw StateError('realtime checks failed: ${failedChecks.join(', ')}');
    }
    if (shutdownFailure != null) {
      throw StateError('native GUI shutdown failed: $shutdownFailure');
    }
  }
}

int assistantContentCount(Map<String, dynamic> snapshot) {
  return timelineRows(snapshot)
      .where(
        (row) => const {
          'reasoningSummary',
          'commentary',
          'finalAnswer',
          'toolGroup',
        }.contains(row['type']),
      )
      .length;
}

int reasoningCount(Map<String, dynamic> snapshot) =>
    timelineRows(snapshot)
        .where((row) => row['type'] == 'reasoningSummary')
        .length;

/// A token of observable content progress for the current turn.
///
/// It grows whenever assistant text is appended or a row is added (and includes
/// the Driver-only delivery counters when they are enabled), so a later
/// observation can be proven to be a *new* update rather than a re-read of the
/// same snapshot. It is an evidence token, never a performance measurement.
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

int answerCount(Map<String, dynamic> snapshot) =>
    timelineRows(snapshot).where((row) => row['type'] == 'finalAnswer').length;

bool answerContains(Map<String, dynamic> snapshot, String needle) =>
    timelineRows(snapshot).any(
      (row) =>
          row['type'] == 'finalAnswer' &&
          (row['text'] as String? ?? '').contains(needle),
    );

/// Number of assistant answers carrying `needle`.
///
/// Counted rather than asserted to a fixed value: the parallel scenario waits on
/// both background receipts, and the runtime may batch the delivered results
/// into one model step, so one answer may legitimately cover both commands.
int answerMatchCount(Map<String, dynamic> snapshot, String needle) =>
    timelineRows(snapshot)
        .where(
          (row) =>
              row['type'] == 'finalAnswer' &&
              (row['text'] as String? ?? '').contains(needle),
        )
        .length;

List<Map<String, dynamic>> execTools(Map<String, dynamic> snapshot) {
  final tools = <Map<String, dynamic>>[];
  for (final row in timelineRows(snapshot)) {
    final rowTools = row['tools'];
    if (rowTools is List) {
      tools.addAll(
        rowTools.whereType<Map<String, dynamic>>().where(
          (tool) => tool['name'] == 'exec',
        ),
      );
    }
  }
  return tools;
}

int activeExecCount(Map<String, dynamic> snapshot) =>
    activeExecStatuses(snapshot).length;

List<String?> activeExecStatuses(Map<String, dynamic> snapshot) =>
    execTools(snapshot)
        .map((tool) => tool['status'] as String?)
        .where(_activeToolStatuses.contains)
        .toList();

/// Tools whose status proves real execution rather than a scheduled call.
List<String?> liveExecStatuses(Map<String, dynamic> snapshot) =>
    execTools(snapshot)
        .map((tool) => tool['status'] as String?)
        .where(_liveToolStatuses.contains)
        .toList();

/// Terminal tool states: no further live sample can ever arrive.
const _terminalToolStatuses = <String>{
  'succeeded',
  'failed',
  'denied',
  'cancelled',
  'interrupted',
};

/// The projected exec tool with [callId], when it exists at all.
Map<String, dynamic>? _toolByCallId(
  Map<String, dynamic> snapshot,
  String callId,
) {
  for (final tool in execTools(snapshot)) {
    if (tool['callId'] == callId) return tool;
  }
  return null;
}

/// Whether the timeline tool projection carries the typed in-flight fields.
///
/// Detected on any tool row (not only the sampled call), so an integrated build
/// that omits them for one identity is still distinguishable from a build that
/// never projects them.
bool _timelineExposesInFlightToolFields(Map<String, dynamic> snapshot) {
  for (final row in timelineRows(snapshot)) {
    final tools = row['tools'];
    if (tools is! List) continue;
    for (final tool in tools.whereType<Map<String, dynamic>>()) {
      if (tool.containsKey('output') &&
          tool.containsKey('progressBytes') &&
          tool.containsKey('itemRevision')) {
        return true;
      }
    }
  }
  return false;
}

/// A live sample of one call's typed in-flight stdout.
///
/// Returns null unless the call is genuinely still executing *and* the typed
/// `output`/`progressBytes`/`itemRevision` triple is present: a scheduled,
/// terminal or field-less observation is never a live progress sample.
Map<String, Object?>? _liveToolProgressSample(
  Map<String, dynamic> snapshot,
  String callId,
) {
  final tool = _toolByCallId(snapshot, callId);
  if (tool == null) return null;
  if (!_liveToolStatuses.contains(tool['status'])) return null;
  final output = tool['output'];
  final progressBytes = tool['progressBytes'];
  final itemRevision = tool['itemRevision'];
  if (output is! String || progressBytes is! int || itemRevision is! int) {
    return null;
  }
  return <String, Object?>{
    'status': tool['status'],
    'progressBytes': progressBytes,
    'itemRevision': itemRevision,
    'outputChars': output.length,
    'lines': 'realtime-long-output-line'.allMatches(output).length,
  };
}

/// Whether one exec call really exited and its committed output is observable.
bool execDelivered(
  Map<String, dynamic> snapshot,
  String callId,
  String outputNeedle,
) => execTools(snapshot).any(
  (tool) =>
      tool['callId'] == callId &&
      tool['status'] == 'succeeded' &&
      (tool['result'] as String? ?? '').contains(outputNeedle),
);

String? threadStatus(Map<String, dynamic> snapshot) {
  final status = workspaceOf(snapshot)?['threadStatus'];
  return status is String ? status : null;
}

bool threadFaulted(Map<String, dynamic> snapshot) =>
    threadStatus(snapshot) == 'faulted';

/// Whether the awaited state is unreachable because the Turn already failed or
/// was cancelled, so a wait reports the real observation immediately instead of
/// burning its whole timeout on a state that can no longer arrive.
bool turnFailed(Map<String, dynamic> snapshot) {
  final status = turnStatus(snapshot);
  return threadFaulted(snapshot) || status == 'failed' || status == 'cancelled';
}

int outputTokens(Map<String, dynamic> snapshot) {
  final usage = workspaceOf(snapshot)?['usage'];
  return usage is Map ? (usage['outputTokens'] as num? ?? 0).toInt() : 0;
}

bool isBusy(Map<String, dynamic> snapshot) =>
    workspaceOf(snapshot)?['isBusy'] == true;

bool isDurableSettled(Map<String, dynamic> snapshot) {
  final workspace = workspaceOf(snapshot);
  final persistence = snapshot['persistence'];
  return workspace != null &&
      workspace['isBusy'] == false &&
      workspace['threadStatus'] == 'idle' &&
      workspace['syncState'] == 'ready' &&
      persistence is Map &&
      persistence['kind'] == 'ready' &&
      persistence['pendingCommits'] == 0;
}

String? turnStatus(Map<String, dynamic> snapshot) {
  final workspace = workspaceOf(snapshot);
  if (workspace == null) return null;
  final turn = workspace['turn'];
  if (turn is Map && turn['status'] is String) return turn['status'] as String;
  final lastTurn = workspace['lastTurn'];
  if (lastTurn is Map && lastTurn['status'] is String) {
    return lastTurn['status'] as String;
  }
  return null;
}

String? turnId(Map<String, dynamic> snapshot) {
  final workspace = workspaceOf(snapshot);
  if (workspace == null) return null;
  final turn = workspace['turn'];
  if (turn is Map && turn['id'] is String) return turn['id'] as String;
  final lastTurn = workspace['lastTurn'];
  if (lastTurn is Map && lastTurn['id'] is String) {
    return lastTurn['id'] as String;
  }
  return null;
}

String? interactionKind(Map<String, dynamic> snapshot) {
  final interaction = workspaceOf(snapshot)?['activeInteraction'];
  return interaction is Map ? interaction['kind'] as String? : null;
}

Map<String, dynamic>? workspaceOf(Map<String, dynamic> snapshot) {
  final workspace = snapshot['workspace'];
  return workspace is Map<String, dynamic> ? workspace : null;
}

List<Map<String, dynamic>> timelineRows(Map<String, dynamic> snapshot) {
  final timeline = workspaceOf(snapshot)?['timeline'];
  if (timeline is! List) return const [];
  return timeline.whereType<Map<String, dynamic>>().toList(growable: false);
}

/// The assistant final-answer row identities present in the current window.
List<String> _assistantRowIds(Map<String, dynamic> snapshot) => <String>[
  for (final row in timelineRows(snapshot))
    if (row['type'] == 'finalAnswer' && row['id'] is String)
      row['id'] as String,
];

Map<String, Object?> summarize(Map<String, dynamic> snapshot) {
  final workspace = workspaceOf(snapshot);
  final details = workspace ?? const {};
  final usage = details['usage'];
  final persistence = snapshot['persistence'];
  final rows = timelineRows(snapshot);
  final tools = execTools(snapshot);
  return {
    'threadStatus': details['threadStatus'],
    'isBusy': details['isBusy'],
    'syncState': details['syncState'],
    'turnId': turnId(snapshot),
    'turnStatus': turnStatus(snapshot),
    'interactionKind': interactionKind(snapshot),
    'outputTokens': usage is Map ? usage['outputTokens'] : null,
    'persistenceKind': persistence is Map ? persistence['kind'] : null,
    'pendingCommits': persistence is Map ? persistence['pendingCommits'] : null,
    'rowCount': rows.length,
    'rowIds': [for (final row in rows) row['id']],
    'rowTypes': [for (final row in rows) row['type']],
    'answerLengths': [
      for (final row in rows)
        if (row['type'] == 'finalAnswer') (row['text'] as String? ?? '').length,
    ],
    'tools': [
      for (final tool in tools)
        {
          'name': tool['name'],
          'callId': tool['callId'],
          'status': tool['status'],
          'exitCode': tool['exitCode'],
          'resultLength': (tool['result'] as String? ?? '').length,
          'resultPreview': _resultPreview(tool['result'] as String?),
        },
    ],
  };
}

/// Bounded, single-note preview of a tool result so a reviewer can re-check the
/// observed output without duplicating whole command transcripts.
String? _resultPreview(String? value) {
  if (value == null || value.isEmpty) return null;
  const limit = 200;
  return value.length <= limit ? value : '${value.substring(0, limit)}…';
}

/// The integrated conversation-activity projection, when the GUI exposes it.
Map<String, dynamic>? conversationActivity(Map<String, dynamic> snapshot) {
  final activity = snapshot['conversationActivity'];
  return activity is Map<String, dynamic> ? activity : null;
}

/// The Driver-only content-delivery counters, when the GUI exposes them.
///
/// `bodyUtf8Bytes` is a UTF-8 measure of the body text the UI actually delivered
/// — an *application-payload* number (metric `application-payload`). It is never
/// an FRB/wire byte count and must not be compared with the fixture's synthetic
/// provider output bytes.
Map<String, dynamic>? contentDelivery(Map<String, dynamic> snapshot) {
  final delivery = snapshot['contentDelivery'];
  return delivery is Map<String, dynamic> ? delivery : null;
}

/// The integrated read-only timeline-scroll geometry, when the GUI exposes it.
Map<String, dynamic>? timelineScroll(Map<String, dynamic> snapshot) {
  final scroll = snapshot['timelineScroll'];
  return scroll is Map<String, dynamic> ? scroll : null;
}

double? _num(Object? value) => value is num ? value.toDouble() : null;

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

double? _viewportDimension(Map<String, dynamic>? scroll) =>
    _num(scroll?['viewportDimension']);

class _ScrollGeometry {
  const _ScrollGeometry({
    required this.consistent,
    required this.followingBottom,
    required this.pixels,
    required this.maxScrollExtent,
    required this.extentAfter,
    required this.bottomSlack,
  });

  final bool consistent;
  final bool followingBottom;
  final double pixels;
  final double maxScrollExtent;
  final double? extentAfter;
  final double bottomSlack;
}

_ScrollGeometry? _scrollGeometry(Map<String, dynamic> scroll) {
  final pixels = _num(scroll['pixels']);
  final maxScrollExtent = _num(scroll['maxScrollExtent']);
  if (pixels == null || maxScrollExtent == null) return null;
  return _ScrollGeometry(
    consistent: pixels >= -0.5 && pixels <= maxScrollExtent + 0.5,
    followingBottom: scroll['followingBottom'] == true,
    pixels: pixels,
    maxScrollExtent: maxScrollExtent,
    extentAfter: _num(scroll['extentAfter']),
    bottomSlack: _num(scroll['bottomSlack']) ?? 0,
  );
}
