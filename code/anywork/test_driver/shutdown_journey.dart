import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';
// Public snapshot helpers shared with the realtime/history/plan journeys: this
// journey reads the same Driver projection and never forks a second reading.
import 'realtime_journey.dart'
    show
        isDurableSettled,
        summarize,
        timelineRows,
        turnFailed,
        turnId,
        turnStatus,
        workspaceOf;

/// Shutdown-acceptance prompts, aligned with `GUI_SHUTDOWN_PROMPTS`.
const _prompts = <String>[
  'Local GUI shutdown normal exit fixture',
  'Local GUI shutdown second instance continue fixture',
  'Local GUI shutdown duplicate request fixture',
  'Local GUI shutdown dart hang fixture',
  'Local GUI shutdown storage lock prime fixture',
  'Local GUI shutdown storage lock blocked fixture',
  'Local GUI shutdown unresponsive service fixture',
  'Local GUI shutdown mcp reclamation fixture',
  'Local GUI shutdown lsp reclamation fixture',
  'Local GUI shutdown tool reclamation fixture',
  'Local GUI shutdown concurrent stop fixture',
];

/// Deterministic answers, aligned with `GUI_SHUTDOWN_ANSWERS`.
const _answers = <String>[
  'shutdown normal exit answer',
  'shutdown second instance answer',
  'shutdown duplicate request answer',
  'shutdown dart hang answer',
  'shutdown storage lock prime answer',
  'shutdown storage lock blocked answer',
  'shutdown unresponsive service answer',
  'shutdown mcp reclamation answer',
  'shutdown lsp reclamation answer',
  'shutdown tool reclamation answer',
  'shutdown concurrent stop answer',
];

/// A single overall exit budget is 30 seconds (native host). The journey never
/// waits longer than the coordinator's OS process wait; these are local
/// observation windows only.
const _requestTimeout = Duration(seconds: 20);

/// True when any visible timeline row already carries [needle] as its text.
///
/// Used only to prove a paced provider reply actually streamed: unlike
/// `answerContains` this does not depend on the row already having the terminal
/// `finalAnswer` type while the turn is still in flight.
bool _rowTextContains(Map<String, dynamic> snapshot, String needle) =>
    timelineRows(snapshot)
        .any((row) => (row['text'] as String? ?? '').contains(needle));

/// Driver request names frozen with the GUI driver entrypoint. `shutdown` is the
/// typed cleanup acknowledgement (never a fake process exit); the exit requests
/// return `exit_requested` before the real central shutdown is scheduled.
const _cleanupRequest = 'shutdown';
const _exitRequest = 'request-app-exit';

/// Driver-only: repeat the single central exit in one call. It fixes the same
/// first deadline again (a real second `beginExit`) and then triggers the
/// shared exit twice ~500ms apart, reporting the reused remaining budget.
const _twiceRequest = 'request-app-exit-twice';
const _hangRequest = 'shutdown-hang';

/// Driver-only: arm the native single-exit budget without cleaning up or
/// exiting, returning the remaining budget so a repeated request can be shown to
/// reuse the same deadline instead of re-arming it.
const _armRequest = 'arm-exit';

/// Driver-only native identity: the real OS pid of the GUI process so the
/// coordinator OS-waits the native process itself (not the `cargo xtask`/Flutter
/// launcher) with its own `/proc` start-time reuse guard.
const _pidRequest = 'pid';

/// Driver-only read-only exit diagnostics: native remaining deadline, cleanup
/// budget, exit-request count, typed shutdown report and observed stages.
const _exitStatusRequest = 'exit-status';

/// Localized second-instance banners produced by the real fatal widget when the
/// instance lock is refused; matched through the widget tree, never a snapshot
/// guess (see `runtime_banners.dart` / `startupInstanceBusy`).
const _busyTexts = <String>['已有实例在运行', 'Another instance is already running'];

/// Localized fatal-startup banners rendered when initialization truly fails;
/// used to prove a real failure widget instead of an absent-workspace guess.
const _fatalTexts = <String>['糊来帮无法启动', 'anywork could not start'];

Future<void> main(List<String> args) async {
  if (args.length != 5) {
    stderr.writeln(
      'usage: shutdown_journey.dart PHASE VM_URL PROJECT_DIR OUTPUT_DIR COORD_DIR',
    );
    exitCode = 64;
    return;
  }
  final phase = args[0];
  final project = args[2];
  final output = Directory(args[3]);
  final coord = Directory(args[4]);
  final FlutterDriverSession driver;
  try {
    driver = await FlutterDriverSession.connect(vmServiceUrl: args[1]);
  } catch (error, stackTrace) {
    await _recordConnectFailure(phase, coord, output, error);
    // Never returns: the journey below can then rely on `driver` being set.
    Error.throwWithStackTrace(error, stackTrace);
  }
  final journey = ShutdownJourney(
    driver: driver,
    phase: phase,
    project: project,
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
    await journey.captureFailure();
  }
  await journey.writeSummary(failure: failure);
  // The exit phases let the native host close the VM connection first; a close
  // failure is expected there and never changes the coordinator's judgement,
  // which waits on the real OS process.
  try {
    await driver.close().timeout(const Duration(seconds: 5));
  } on Object {
    // Ignored: see above.
  }
  if (failure != null) {
    Error.throwWithStackTrace(failure, failureStack!);
  }
  journey.raiseIfFailed();
}

/// Records a stage marker and a minimal summary when the Driver cannot connect.
Future<void> _recordConnectFailure(
  String phase,
  Directory coord,
  Directory output,
  Object error,
) async {
  await File('${coord.path}/shutdown-stage')
      .writeAsString('${phase}_connect_failed');
  await File('${coord.path}/shutdown-stage-${phase}_connect_failed')
      .writeAsString(phase);
  final summary = <String, Object?>{
    'scenario': 'shutdown',
    'phase': phase,
    'verdict': 'pending',
    'status': 'failed',
    'stage': '${phase}_connect_failed',
    'error': '$error',
    'completedAt': DateTime.now().toUtc().toIso8601String(),
  };
  await File(
    '${output.path}/shutdown-$phase-summary.json',
  ).writeAsString('${const JsonEncoder.withIndent('  ').convert(summary)}\n');
}

class ShutdownJourney {
  ShutdownJourney({
    required this.driver,
    required this.phase,
    required this.project,
    required this.output,
    required this.coord,
  }) : stageFile = File('${coord.path}/shutdown-stage');

  final FlutterDriverSession driver;
  final String phase;
  final String project;
  final Directory output;
  final Directory coord;
  final File stageFile;

  final Map<String, Object?> checks = <String, Object?>{};
  final Map<String, Object?> observations = <String, Object?>{};
  final Map<String, Object?> counts = <String, Object?>{};
  final List<String> pendingEvidence = <String>[];
  String stage = 'created';

  Future<void> run() async {
    switch (phase) {
      case 'normal':
        await _runNormal();
      case 'reopen':
        await _runReopen('normal', requireSettled: true);
      case 'busy-hold':
        await _runBusyHold();
      case 'busy-second':
        await _runBusySecond();
      case 'busy-reopen':
        await _runReopen('busy_continue', requireSettled: true);
      case 'duplicate':
        await _runDuplicate();
      case 'hang':
        await _runHang();
      case 'runtime-unavailable':
        await _runRuntimeUnavailable();
      case 'storage-lock':
        await _runStorageLock();
      case 'storage-verify':
        // The forced exit happened while the writer was blocked, so the reopened
        // persistence may legitimately still be recovering: the committed prime
        // turn and the openable database are the facts, not a settled writer.
        await _runReopen('storage', requireSettled: false);
      case 'service-unresponsive':
        await _runServiceUnresponsive();
      case 'close-during-init':
        await _runCloseDuringInit();
      case 'bridge-unavailable':
        await _runBridgeUnavailable();
      case 'subscription-fault':
        await _runForcedExitFault('subscription-fault');
      case 'dart-error-fallback':
        await _runForcedExitFault('tmp-fallback');
      case 'mcp-reclamation':
        await _runMcpReclamation();
      case 'lsp-reclamation':
        await _runLspReclamation();
      case 'tool-reclamation':
        await _runToolReclamation();
      case 'concurrent-stop':
        await _runConcurrentStop();
      default:
        throw StateError('shutdown: unknown phase $phase');
    }
  }

  // -------------------------------------------------------------- normal ---

  /// One real turn saved and durably settled, the typed cleanup acknowledgement,
  /// then the real application exit. The coordinator judges the OS exit code.
  Future<void> _runNormal() async {
    await mark('normal_connected');
    await _writeNativeIdentity('normal');
    await _openProject();
    final settled = await _settleTurn(0, 'normal');
    await _writeObserved('normal', settled, _answers[0]);
    await mark('normal_settled');
    await _typedCleanupAck();
    await mark('normal_cleanup_ack');
    await _requestAppExit('normal');
  }

  /// Reopens an isolated home and proves the previously saved turn is restored
  /// from durable history (identity, committed answer, settled persistence).
  Future<void> _runReopen(
    String observedKey, {
    required bool requireSettled,
  }) async {
    await mark('${phase}_connected');
    await _writeNativeIdentity(phase);
    final observed = await _readObserved(observedKey);
    final threadId = observed['threadId'] as String;
    final answer = observed['answer'] as String;
    await _ensureThreadOpen(threadId, phase);
    final snapshot = await waitFor(
      (snapshot) =>
          timelineRows(snapshot).any(
            (row) => row['type'] == 'finalAnswer' && row['text'] == answer,
          ) &&
          (!requireSettled || isDurableSettled(snapshot)),
      '${phase}_restored',
      timeout: const Duration(seconds: 120),
    );
    final rows = timelineRows(snapshot);
    checks['${phase}ThreadRestored'] =
        workspaceOf(snapshot)?['threadId'] == threadId;
    checks['${phase}AnswerRestored'] = rows.any(
      (row) => row['type'] == 'finalAnswer' && row['text'] == answer,
    );
    if (requireSettled) {
      checks['${phase}DurableSettled'] = isDurableSettled(snapshot);
    }
    counts['${phase}RowCount'] = rows.length;
    observations['${phase}Restored'] = summarize(snapshot);
    await shot('${phase}_restored');
    await mark('${phase}_restored');
    await _requestAppExit(phase);
  }

  // ---------------------------------------------------------------- busy ---

  /// Keeps the first instance alive while a second instance shares the home, then
  /// proves the first still submits and saves a real turn after the second is
  /// closed.
  Future<void> _runBusyHold() async {
    await mark('busy_hold_connected');
    await _writeNativeIdentity('busy-hold');
    final observed = await _readObserved('normal');
    final threadId = observed['threadId'] as String;
    await _ensureThreadOpen(threadId, 'busy_hold');
    await waitFor(
      (snapshot) => isDurableSettled(snapshot),
      'busy_hold_reopened_settled',
      timeout: const Duration(seconds: 120),
    );
    await mark('busy_hold_reopened');
    // The coordinator closes the second instance and only then signals this
    // instance to continue, so the two requests can never interleave.
    await _awaitSignal('busy-second-closed', const Duration(seconds: 600));
    final continued = await _settleTurn(1, 'busy_continue', prior: threadId);
    checks['busyContinuedSameThread'] = continued.threadId == threadId;
    await _writeObserved('busy_continue', continued, _answers[1]);
    await mark('busy_continued');
    await _requestAppExit('busy');
  }

  /// Records whatever the second instance exposes about the instance lock: the
  /// typed `instanceBusy` code in the snapshot, or (if it never reached a usable
  /// Driver) the connection failure. The coordinator also greps the GUI log.
  Future<void> _runBusySecond() async {
    await mark('busy_second_connected');
    await _writeNativeIdentity('busy-second');
    // The second instance renders the real instance-busy banner
    // (`runtime_banners.dart` `startupInstanceBusy`); a snapshot-only check
    // cannot prove the widget, so match the live widget tree text.
    final busyText = await _awaitAnyText(
      _busyTexts,
      const Duration(seconds: 90),
    );
    // Read the live projection into a local that does *not* shadow the
    // `snapshot()` method (a same-named local would be referenced before it is
    // declared inside its own initializer).
    final snap = await snapshot();
    final encoded = jsonEncode(snap);
    final busy =
        encoded.contains('instanceBusy') ||
        encoded.contains('已有实例') ||
        encoded.contains('instance busy');
    checks['busySecondInstanceBusy'] = busy || busyText != null;
    checks['busySecondBusyWidget'] = busyText != null;
    observations['busySecondBusyText'] = busyText;
    observations['busySecondSnapshot'] = summarize(snap);
    observations['busySecondNavigation'] = snap['navigation'];
    observations['busySecondRecoveryIssues'] = snap['recoveryIssues'];
    // The snapshot may legitimately not carry the typed startup failure, so the
    // widget text above is the primary busy proof. The read-only `exit-status`
    // projection is then used to record the typed `instanceBusy` code and its
    // non-empty bridge correlation rather than guessing them from UI text. These
    // are evidence checks: the coordinator judges this instance from the widget
    // text and the real OS exit report, never from these fields alone.
    final status = await _exitStatus();
    final startup = status?['startup'];
    observations['busySecondStartup'] = startup;
    observations['busySecondNativeExit'] = status?['nativeExit'];
    checks['busySecondTypedStartupFailure'] =
        startup is Map && startup['failureCode'] == 'instanceBusy';
    checks['busySecondStartupCorrelation'] =
        startup is Map &&
        (startup['correlationId'] as String?)?.isNotEmpty == true;
    await shot('busy_second');
    await mark('busy_observed');
    // The coordinator closes this second instance through a real
    // `WM_DELETE_WINDOW` on its isolated display, so the journey must NOT request
    // the Dart `beginExit` path here: that would pre-empt the native GTK close
    // hook the delete event is meant to exercise.
  }

  // ----------------------------------------------------------- duplicate ---

  /// The first arm anchors the single 30-second budget. Several seconds later
  /// the same close/exit request is repeated; the coordinator measures the wall
  /// clock from the first arm, and the Driver-only `exit-status` reading proves
  /// the remaining deadline only decreased (never reset to a fresh 30s).
  ///
  /// This phase runs through the existing Driver-only subscription-fault
  /// entrypoint (`shutdown_fault_driver.dart`): its progress subscription errors
  /// and its cancel stays pending for the coordinator's bounded 2s, so the one
  /// central cleanup outlives the 500ms between the two repeated requests and
  /// both are genuinely delivered to the same shared budget.
  Future<void> _runDuplicate() async {
    await mark('duplicate_connected');
    await _writeNativeIdentity('duplicate');
    await _openProject();
    final settled = await _settleTurn(2, 'duplicate');
    await _writeObserved('duplicate', settled, _answers[2]);
    await mark('duplicate_settled');
    await mark('duplicate_exit_requested');
    // First arm: fixes the single native deadline and reports its remaining
    // budget. `arm-exit` never exits, so this reading is reliable.
    final armed = await _requestJson(_armRequest, const Duration(seconds: 15));
    final armedRemainingMs = armed?['remainingMs'];
    checks['duplicateArmed'] =
        armedRemainingMs is int &&
        armedRemainingMs > 0 &&
        armedRemainingMs <= 30000;
    observations['duplicateArmedRemainingMs'] = armedRemainingMs;
    // Delay several seconds so the shared deadline has clearly advanced before
    // the repeated close.
    await Future<void>.delayed(const Duration(seconds: 6));
    // Repeat the close in one Driver call: `request-app-exit-twice` fixes the
    // same first deadline again (a real second `beginExit`) and reports the
    // reused remaining budget, then triggers the single central exit twice
    // ~500ms apart on the shared future. A refreshed budget would make the
    // second `armedRemainingMs` jump *back up*; a strictly smaller value is the
    // proof the first arm still owns it (and that the budget is ticking). The
    // raw receipt is recorded so the phase never assumes a delivery it did not
    // observe.
    final twice = await _requestJson(
      _twiceRequest,
      const Duration(seconds: 20),
    );
    observations['duplicateTwice'] = twice;
    final twiceArmedRemainingMs = twice?['armedRemainingMs'];
    observations['duplicateTwiceArmedRemainingMs'] = twiceArmedRemainingMs;
    checks['duplicateDeadlineNotRefreshed'] =
        armedRemainingMs is int &&
        twiceArmedRemainingMs is int &&
        twiceArmedRemainingMs < armedRemainingMs &&
        twiceArmedRemainingMs >= armedRemainingMs - 12000;
    // Observe the delivered request count from the read-only native projection
    // while the bounded cleanup is still running: two requests must have reached
    // the same budget. A missing reading is recorded as "not observed", never
    // counted as a pass.
    await _recordDuplicateExitRequests();
  }

  /// Polls the read-only exit diagnostics for the delivered exit-request count
  /// after the repeated close, so `requests >= 2` is judged from a real reading
  /// of the shared native budget rather than merely from the command being sent.
  ///
  /// The reused fault entrypoint keeps the single central cleanup pending for
  /// the coordinator's bounded cancel window, so the isolate lives well past the
  /// 500ms between the two requests and the projection really does report both.
  /// The typed Degraded report is captured too when the poll reaches it.
  Future<void> _recordDuplicateExitRequests() async {
    int? maxRequests;
    int? lastRemainingMs;
    Map<String, dynamic>? report;
    for (var attempt = 0; attempt < 24; attempt++) {
      final status = await _exitStatus();
      final count = _nativeRequests(status);
      if (count != null) {
        maxRequests = (maxRequests == null || count > maxRequests)
            ? count
            : maxRequests;
      }
      lastRemainingMs = _nativeRemainingMs(status) ?? lastRemainingMs;
      final shutdownReport = status?['shutdownReport'];
      if (shutdownReport is Map) {
        report = Map<String, dynamic>.from(shutdownReport);
      }
      if (maxRequests != null && maxRequests >= 2 && report != null) break;
      await Future<void>.delayed(const Duration(milliseconds: 150));
    }
    observations['duplicateMaxRequests'] = maxRequests;
    observations['duplicatePostRemainingMs'] = lastRemainingMs;
    observations['duplicateReport'] = report;
    observations['duplicateReportOutcome'] = report?['outcome'];
    observations['duplicateReportPersistence'] = report?['persistence'];
    // Hard gate: both repeated requests must have reached the one shared budget,
    // judged from a real read of the native projection. A missing read fails the
    // phase; it is never treated as a pass.
    checks['duplicateRequestsAtLeastTwo'] =
        maxRequests != null && maxRequests >= 2;
  }

  // ---------------------------------------------------------------- hang ---

  /// The Driver hangs the isolate after the native host armed the single exit
  /// budget; the native host must force the process out with exit code 1.
  Future<void> _runHang() async {
    await mark('hang_connected');
    await _writeNativeIdentity('hang');
    await _openProject();
    final settled = await _settleTurn(3, 'hang');
    await _writeObserved('hang', settled, _answers[3]);
    await mark('hang_settled');
    await mark('hang_exit_requested');
    await _request('hang', _hangRequest, const Duration(seconds: 45));
  }

  // --------------------------------------------------- runtime-unavailable ---

  /// The runtime never installs (isolated unreadable home); the diagnostics
  /// teardown and the native exit must still work and leave logs.
  Future<void> _runRuntimeUnavailable() async {
    await mark('runtime_unavailable_connected');
    await _writeNativeIdentity('runtime-unavailable');
    final snap = await snapshot();
    final fatalBanner = await _awaitAnyText(
      _fatalTexts,
      const Duration(seconds: 60),
    );
    final status = await _exitStatus();
    _recordRuntimeUnavailable(snap, fatalBanner, status);
    await shot('runtime_unavailable');
    await mark('runtime_unavailable_observed');
    await _requestAppExit('runtime_unavailable');
  }

  void _recordRuntimeUnavailable(
    Map<String, dynamic> snapshot,
    String? fatalBanner,
    Map<String, dynamic>? status,
  ) {
    observations['runtimeUnavailableSnapshot'] = summarize(snapshot);
    observations['runtimeUnavailableRecoveryIssues'] =
        snapshot['recoveryIssues'];
    observations['runtimeUnavailableNavigation'] = snapshot['navigation'];
    observations['runtimeUnavailableFatalText'] = fatalBanner;
    observations['runtimeUnavailableTypedStartup'] = status?['startup'];
    checks['runtimeUnavailableWorkspaceAbsent'] = workspaceOf(snapshot) == null;
    // A missing workspace alone proves nothing; the real fatal widget must have
    // rendered (see `runtime_banners.dart` `runtimeFatalTitle`).
    checks['runtimeUnavailableFatalWidget'] = fatalBanner != null;
  }

  // --------------------------------------------------------- storage lock ---

  /// A durable prime turn, then a paced turn the coordinator's exclusive writer
  /// lock keeps from committing. The forced exit must not lose the prime, and a
  /// reopen must still open the database.
  Future<void> _runStorageLock() async {
    await mark('storage_lock_connected');
    await _writeNativeIdentity('storage-lock');
    await _openProject();
    final settled = await _settleTurn(4, 'storage_prime');
    await _writeObserved('storage', settled, _answers[4]);
    await mark('storage_prime_settled');
    // The coordinator takes the exclusive writer lock now and signals back.
    await _awaitSignal('storage-lock-held', const Duration(seconds: 300));
    await _submit(_prompts[5]);
    // The paced reply must actually stream the blocked turn's text (the prime
    // answer already contains "shutdown storage lock", and the user prompt
    // already contains "storage lock blocked"), which proves the fixture
    // accepted the request before the app is asked to exit.
    await waitFor(
      (snapshot) =>
          turnId(snapshot) != null &&
          !isDurableSettled(snapshot) &&
          _rowTextContains(snapshot, 'blocked answer'),
      'storage_blocked_inflight',
      timeout: const Duration(seconds: 90),
      abort: turnFailed,
    );
    await mark('storage_blocked');
    // Now the coordinator lets the app try to exit while the lock is still held.
    await _awaitSignal('storage-exit', const Duration(seconds: 300));
    await _requestAppExit('storage');
  }

  // ------------------------------------------------------ service unresponsive ---

  /// The local provider fixture is frozen (SIGSTOP) mid-turn; the real exit must
  /// still complete without waiting on the unresponsive service.
  Future<void> _runServiceUnresponsive() async {
    await mark('service_connected');
    await _writeNativeIdentity('service-unresponsive');
    await _openProject();
    await _submit(_prompts[6]);
    // The paced reply streams its first chunk while the turn is still live;
    // that arrival proves the fixture accepted the request, so the coordinator
    // can then freeze it mid-stream without stranding the request unread.
    await waitFor(
      (snapshot) =>
          turnId(snapshot) != null &&
          !isDurableSettled(snapshot) &&
          _rowTextContains(snapshot, 'shutdown-service'),
      'service_inflight',
      timeout: const Duration(seconds: 60),
      abort: turnFailed,
    );
    await mark('service_inflight');
    await _awaitSignal('service-stopped', const Duration(seconds: 300));
    await _requestAppExit('service');
  }

  // ----------------------------------------------------- close during init ---

  /// The runtime is stuck initializing (Driver-only `pending-init` fault). An
  /// exit requested now must report Degraded + Unknown and force exit 1, never a
  /// fabricated NotStarted/Stopped. The typed report is written for the
  /// coordinator because the process is gone before it can be read live.
  Future<void> _runCloseDuringInit() async {
    await mark('init-close_connected');
    await _writeNativeIdentity('init-close');
    final status = await _exitStatus();
    observations['initCloseStartup'] = status?['startup'];
    final report = await _requestJson(
      _cleanupRequest,
      const Duration(seconds: 30),
    );
    observations['initCloseCleanupReport'] = report;
    final outcome = report?['outcome'];
    final persistence = report?['persistence'];
    checks['closeDuringInitDegraded'] =
        report?['shutdown'] == 'degraded' || outcome == 'degraded';
    checks['closeDuringInitUnknown'] = persistence == 'unknown';
    checks['closeDuringInitNotNotStarted'] = outcome != 'notStarted';
    await File(
      '${output.path}/shutdown-close-during-init-ack.json',
    ).writeAsString(
      '${const JsonEncoder.withIndent('  ').convert(report ?? <String, Object?>{})}\n',
    );
    await _requestAppExit('init-close');
  }

  // --------------------------------------------------- bridge unavailable ---

  /// The runtime never installs (Driver-only `bridge-load-error` fault); the
  /// clean close must still be a NotStarted exit 0 with no fabricated Stopped.
  Future<void> _runBridgeUnavailable() async {
    await mark('bridge_connected');
    await _writeNativeIdentity('bridge');
    final status = await _exitStatus();
    observations['bridgeStartup'] = status?['startup'];
    final report = await _requestJson(
      _cleanupRequest,
      const Duration(seconds: 30),
    );
    observations['bridgeCleanupReport'] = report;
    checks['bridgeUnavailableNotStarted'] = report?['outcome'] == 'notStarted';
    checks['bridgeUnavailableCleanExit'] = report?['shutdown'] == 'completed';
    await File(
      '${output.path}/shutdown-bridge-unavailable-ack.json',
    ).writeAsString(
      '${const JsonEncoder.withIndent('  ').convert(report ?? <String, Object?>{})}\n',
    );
    await _requestAppExit('bridge');
  }

  // -------------------------------------------- subscription / temp fallback ---

  /// A Driver-only fault entrypoint owns the exit path; the journey only records
  /// the native identity and requests the same central exit. The coordinator
  /// judges the real OS exit and the typed diagnostics it leaves behind.
  Future<void> _runForcedExitFault(String label) async {
    await mark('${label}_connected');
    await _writeNativeIdentity(label);
    await _requestAppExit(label);
  }

  // --------------------------------------------------- native subtree reclaim ---

  /// A real turn makes the thread acquire the MCP service lease, so the product
  /// starts the scenario-owned stdio MCP server (which records its grandchild's
  /// pid). Once the coordinator has proven the live subtree and started its own
  /// monitor, this isolate requests the Driver-only hang: the native host arms
  /// the single 30-second deadline and forces the process out (exit 1) while its
  /// supervisor reclaims that whole subtree; the coordinator OS-polls the real
  /// business pids.
  Future<void> _runMcpReclamation() async {
    await mark('mcp-reclamation_connected');
    await _writeNativeIdentity('mcp-reclamation');
    await _openProject();
    final settled = await _settleTurn(7, 'mcp_reclamation');
    await _writeObserved('mcp-reclamation', settled, _answers[7]);
    await _awaitFile('shutdown-mcp-coord.json', const Duration(seconds: 240));
    checks['mcpServerStarted'] = true;
    await mark('mcp-reclamation_served');
    // The coordinator resolves the real native host plus the live business pids
    // and starts its independent monitor before this isolate is allowed to
    // request the exit; that private handshake is what makes the later "pids
    // gone" observation meaningful instead of a race with a fast normal exit.
    await _awaitSignal('mcp-reclamation-armed', const Duration(seconds: 180));
    await mark('mcp-reclamation_exit_requested');
    // The Driver-only `shutdown-hang` arms the single native 30-second deadline
    // and then blocks this isolate, so the native host itself must force the
    // process out (exit code 1).
    await _request(
      'mcp-reclamation',
      _hangRequest,
      const Duration(seconds: 45),
    );
  }

  // --------------------------------------------------- lsp subtree reclaim ---

  /// A real `lsp_query` tool turn makes the runtime start the configured
  /// scenario-owned fake language server (and its grandchild). Once the
  /// coordinator has proven the live subtree and started its own monitor, this
  /// isolate requests the Driver-only hang: the native host arms the single
  /// 30-second deadline and forces the process out (exit 1) while its supervisor
  /// reclaims that whole subtree; the coordinator OS-polls the real business
  /// pids the server recorded.
  Future<void> _runLspReclamation() async {
    await mark('lsp-reclamation_connected');
    await _writeNativeIdentity('lsp-reclamation');
    await _openProject();
    final settled = await _settleTurn(8, 'lsp_reclamation');
    await _writeObserved('lsp-reclamation', settled, _answers[8]);
    await _awaitFile('shutdown-lsp-coord.json', const Duration(seconds: 240));
    checks['lspServerStarted'] = true;
    await mark('lsp-reclamation_served');
    await _awaitSignal('lsp-reclamation-armed', const Duration(seconds: 180));
    await mark('lsp-reclamation_exit_requested');
    await _request(
      'lsp-reclamation',
      _hangRequest,
      const Duration(seconds: 45),
    );
  }

  // -------------------------------------------------- tool subtree reclaim ---

  /// A real background `exec` tool turn makes the runtime start the
  /// scenario-owned `--tool-peer` (and its grandchild). The command outlives the
  /// foreground window, so the turn is a live background task. Once the
  /// coordinator has proven the live subtree and started its own monitor, this
  /// isolate requests the Driver-only hang: the native host arms the single
  /// 30-second deadline and forces the process out (exit 1) while its supervisor
  /// reclaims the whole subtree; the coordinator OS-polls the business pids the
  /// peer recorded.
  Future<void> _runToolReclamation() async {
    await mark('tool-reclamation_connected');
    await _writeNativeIdentity('tool-reclamation');
    await _openProject();
    await _submit(_prompts[9]);
    await _awaitFile('shutdown-tool-coord.json', const Duration(seconds: 240));
    checks['toolPeerStarted'] = true;
    await mark('tool-reclamation_served');
    await _awaitSignal('tool-reclamation-armed', const Duration(seconds: 180));
    await mark('tool-reclamation_exit_requested');
    await _request(
      'tool-reclamation',
      _hangRequest,
      const Duration(seconds: 45),
    );
  }

  // ------------------------------------------------------ concurrent stop ---

  /// One turn starts two independent supervised resources: the scenario-owned
  /// stdio MCP server (through the thread's service lease, because the isolated
  /// home declares it) and the background `--tool-peer` (through the real
  /// supervised tool worker by the scripted `exec` call). Once both peers record
  /// their pids the coordinator first waits for the strict fixture to accept the
  /// turn's required receipt continuation (its live counters report every
  /// remaining step optional), then takes the exclusive writer lock, so the
  /// turn's terminal save cannot drain while the two resources must be reclaimed
  /// on their own. The coordinator OS-waits the native process and polls the
  /// real business pids, so this journey only records the native identity and
  /// requests the real exit.
  Future<void> _runConcurrentStop() async {
    await mark('concurrent-stop_connected');
    await _writeNativeIdentity('concurrent-stop');
    await _openProject();
    await _submit(_prompts[10]);
    // Both peer coordination files prove the two independent subtrees started:
    // the MCP server through the thread's service lease and the background tool
    // through the real tool worker.
    await _awaitFile(
      'shutdown-concurrent-mcp-coord.json',
      const Duration(seconds: 240),
    );
    await _awaitFile(
      'shutdown-concurrent-tool-coord.json',
      const Duration(seconds: 240),
    );
    checks['concurrentMcpStarted'] = true;
    checks['concurrentToolStarted'] = true;
    // The turn is still in flight. The coordinator now waits for the strict
    // fixture to accept the turn's required receipt continuation, then takes the
    // exclusive writer lock before that terminal save, and only then writes the
    // arm signal this journey is blocked on.
    await mark('concurrent-stop_served');
    // The coordinator holds the writer lock and has started its independent
    // monitor before this signal, so the later "pids gone while the app is still
    // alive" observation is meaningful instead of a race with a fast exit.
    await _awaitSignal('concurrent-stop-armed', const Duration(seconds: 240));
    await _requestAppExit('concurrent-stop');
  }

  // ------------------------------------------------------------- helpers ---

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

  Future<void> _ensureThreadOpen(String threadId, String label) async {
    await driver.waitFor(
      find.byValueKey('thread-row-$threadId'),
      timeout: const Duration(seconds: 90),
    );
    try {
      await waitFor(
        (snapshot) => threadIdOf(snapshot) == threadId,
        '${label}_already_open',
        timeout: const Duration(seconds: 10),
      );
      return;
    } on StateError {
      // Not the open workspace yet: switch through the real sidebar row.
    }
    await _tap('thread-row-$threadId');
    await waitFor(
      (snapshot) => threadIdOf(snapshot) == threadId,
      '${label}_opened',
      timeout: const Duration(seconds: 60),
    );
  }

  /// Submits [index] and waits for the new Turn to complete and become durable.
  Future<_SettledTurn> _settleTurn(
    int index,
    String label, {
    String? prior,
  }) async {
    // The Turn identity before the submission: the wait below must observe a
    // *new* completed Turn, never the already-settled predecessor on the same
    // Thread (the busy-continue phase reuses the reopened Thread).
    final previousTurn = turnId(await snapshot());
    await _submit(_prompts[index]);
    // Resolve the new Thread identity with an explicit `String` local: the
    // `prior ?? await ...` form left the analyzer inferring a nullable result
    // (the reused Thread case feeds this identity back to `_SettledTurn`).
    final String threadId;
    if (prior != null) {
      threadId = prior;
    } else {
      final opened = await waitFor(
        (snapshot) => (threadIdOf(snapshot) ?? '').isNotEmpty,
        '${label}_thread',
        timeout: const Duration(seconds: 60),
      );
      threadId = threadIdOf(opened)!;
    }
    final settled = await waitFor(
      (snapshot) =>
          threadIdOf(snapshot) == threadId &&
          turnId(snapshot) != null &&
          turnId(snapshot) != previousTurn &&
          turnStatus(snapshot) == 'completed' &&
          isDurableSettled(snapshot),
      '${label}_settled',
      timeout: const Duration(seconds: 300),
      abort: turnFailed,
    );
    return _SettledTurn(threadId: threadId, turnId: turnId(settled));
  }

  String? threadIdOf(Map<String, dynamic> snapshot) {
    final value = workspaceOf(snapshot)?['threadId'];
    return value is String ? value : null;
  }

  /// The typed cleanup acknowledgement. A degraded result is a real failure, not
  /// a faked `Stopped`; it is recorded and rethrown so the run fails closed.
  Future<void> _typedCleanupAck() async {
    try {
      final raw = await driver
          .requestData(_cleanupRequest, timeout: const Duration(seconds: 60))
          .timeout(const Duration(seconds: 65));
      observations['cleanupAck'] = raw;
      final decoded = jsonDecode(raw);
      final clean =
          decoded is Map &&
          (decoded['shutdown'] == 'completed' ||
              decoded['outcome'] == 'clean' ||
              (decoded['report'] is Map &&
                  (decoded['report'] as Map)['outcome'] == 'clean'));
      checks['cleanupAckClean'] = clean;
      if (!clean) {
        throw StateError('typed cleanup acknowledgement was not clean: $raw');
      }
    } on StateError {
      rethrow;
    } on Object catch (error) {
      checks['cleanupAckClean'] = false;
      throw StateError('typed cleanup acknowledgement failed: $error');
    }
  }

  /// Issues an exit request after recording the request marker. The VM may
  /// disconnect first; the OS process wait is the authoritative signal.
  Future<void> _requestAppExit(String label) async {
    await mark('${label}_exit_requested');
    await _request(label, _exitRequest, _requestTimeout);
  }

  Future<void> _request(String label, String message, Duration timeout) async {
    try {
      final raw = await driver
          .requestData(message, timeout: timeout)
          .timeout(timeout + const Duration(seconds: 5));
      observations['${label}_$message'] = raw;
    } on Object catch (error) {
      // A disconnect after the request was delivered is expected while the
      // process exits; the coordinator's real process wait decides the outcome,
      // so the observation is recorded rather than swallowed as success.
      observations['${label}_${message}_error'] = '$error';
    }
  }

  /// Sends [message] and decodes the JSON object reply; returns `null` when the
  /// Driver answered with an error object, an unsupported-request sentinel, a
  /// non-object value, or the connection dropped while the process exited.
  Future<Map<String, dynamic>?> _requestJson(
    String message,
    Duration timeout,
  ) async {
    String raw;
    try {
      raw = await driver
          .requestData(message, timeout: timeout)
          .timeout(timeout + const Duration(seconds: 5));
    } on Object catch (error) {
      observations['${message}_error'] = '$error';
      return null;
    }
    Object? decoded;
    try {
      decoded = jsonDecode(raw);
    } on Object {
      observations['${message}_raw'] = raw;
      return null;
    }
    if (decoded is Map && decoded['error'] == null) {
      return Map<String, dynamic>.from(decoded);
    }
    observations['${message}_raw'] = raw;
    return null;
  }

  /// Resolves the real native process identity via the Driver `pid` request.
  ///
  /// Returns `null` when the entry point does not implement it, so the
  /// coordinator can fail closed on a missing native-pid contract instead of
  /// silently judging the launcher's pid.
  Future<Map<String, dynamic>?> _nativeIdentity() =>
      _requestJson(_pidRequest, const Duration(seconds: 10));

  /// Reads the Driver-only exit diagnostics while the process is still alive.
  Future<Map<String, dynamic>?> _exitStatus() =>
      _requestJson(_exitStatusRequest, const Duration(seconds: 3));

  /// The `nativeExit.remainingMs` from an `exit-status` reading, if present.
  int? _nativeRemainingMs(Map<String, dynamic>? status) {
    final native = status?['nativeExit'];
    final remaining = native is Map ? native['remainingMs'] : null;
    return remaining is int ? remaining : null;
  }

  /// The `nativeExit.requests` count from an `exit-status` reading, if present.
  int? _nativeRequests(Map<String, dynamic>? status) {
    final native = status?['nativeExit'];
    final requests = native is Map ? native['requests'] : null;
    return requests is int ? requests : null;
  }

  /// Waits, bounded, for any of [candidates] to appear as real widget text.
  ///
  /// `find.text` matches the live widget tree, so this proves the product
  /// actually rendered the banner and not merely that some snapshot field is
  /// absent. Each probe is short; the outer loop owns the total deadline.
  Future<String?> _awaitAnyText(
    List<String> candidates,
    Duration timeout,
  ) async {
    final deadline = DateTime.now().add(timeout);
    while (DateTime.now().isBefore(deadline)) {
      for (final text in candidates) {
        try {
          await driver.waitFor(
            find.text(text),
            timeout: const Duration(milliseconds: 500),
          );
          return text;
        } on Object {
          // Not rendered yet (or not this locale); try the next candidate.
        }
      }
      await Future<void>.delayed(const Duration(milliseconds: 200));
    }
    return null;
  }

  Future<Map<String, dynamic>> snapshot() => driver.readSnapshot();

  /// Persists the real native process identity for [label].
  ///
  /// The coordinator OS-waits this pid (with the process start time as a
  /// PID-reuse guard) instead of the `cargo xtask`/Flutter launcher, and the
  /// missing-contract case fails closed: the check stays false and the
  /// coordinator refuses to judge the launcher's pid in its place.
  Future<void> _writeNativeIdentity(String label) async {
    final identity = await _nativeIdentity();
    observations['${label}NativeIdentity'] = identity;
    final pid = identity?['pid'];
    checks['${label}NativePidResolved'] = pid is int && pid > 0;
    // The process start time (the PID-reuse guard) is read by the coordinator
    // from `/proc/<pid>/stat` itself, so the Driver only has to expose the real
    // native pid; requiring a Driver-provided start tick would be a second,
    // drifting source of truth.
    await File('${output.path}/shutdown-native-$label.json').writeAsString(
      '${const JsonEncoder.withIndent('  ').convert(identity ?? <String, Object?>{'pid': null})}\n',
    );
  }

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
      if (abort != null && abort(current)) {
        throw StateError('$label aborted: ${summarize(current)}');
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    throw StateError(
      'timed out waiting for $label: '
      '${last == null ? const {} : summarize(last)}',
    );
  }

  Future<void> _awaitSignal(String name, Duration timeout) async {
    final marker = File('${coord.path}/shutdown-signal-$name');
    final deadline = DateTime.now().add(timeout);
    while (DateTime.now().isBefore(deadline)) {
      if (await marker.exists()) return;
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    throw StateError('timed out waiting for the coordinator signal $name');
  }

  /// Waits, bounded, for a coordination file the coordinator or a
  /// scenario-owned server writes.
  Future<void> _awaitFile(String name, Duration timeout) async {
    final file = File('${coord.path}/$name');
    final deadline = DateTime.now().add(timeout);
    while (DateTime.now().isBefore(deadline)) {
      if (await file.exists()) return;
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    throw StateError('timed out waiting for the coordination file $name');
  }

  Future<void> mark(String value) async {
    stage = value;
    // The per-stage marker is an append-only fact: shutdown and the final
    // summary rewrite the latest-stage file within moments, so a coordinator
    // polling that file's content could skip a stage between two polls.
    await File('${coord.path}/shutdown-stage-$value').writeAsString(value);
    await stageFile.writeAsString(value);
  }

  Future<void> shot(String name) async {
    await File('${output.path}/shutdown-$name.png')
        .writeAsBytes(await driver.screenshot());
  }

  Future<void> _writeObserved(
    String key,
    _SettledTurn settled,
    String answer,
  ) async {
    await File('${output.path}/shutdown-observed-$key.json').writeAsString(
      '${const JsonEncoder.withIndent('  ').convert(<String, Object?>{'threadId': settled.threadId, 'turnId': settled.turnId, 'answer': answer})}\n',
    );
  }

  Future<Map<String, dynamic>> _readObserved(String key) async {
    final file = File('${output.path}/shutdown-observed-$key.json');
    return jsonDecode(await file.readAsString()) as Map<String, dynamic>;
  }

  /// Saves the last snapshot and a screenshot for a failed journey.
  Future<void> captureFailure() async {
    try {
      await File(
        '${output.path}/shutdown-$phase-failure-snapshot.json',
      ).writeAsString(
        '${const JsonEncoder.withIndent('  ').convert(await driver.readSnapshot().timeout(const Duration(seconds: 10)))}\n',
      );
    } on Object {
      // Ignored: the summary still carries the original error and stage.
    }
    try {
      final png = await driver.screenshot().timeout(
        const Duration(seconds: 20),
      );
      await File('${output.path}/shutdown-$phase-failure.png')
          .writeAsBytes(png);
    } on Object {
      // Ignored: see above.
    }
  }

  Future<void> writeSummary({required Object? failure}) async {
    final failedChecks = checks.entries
        .where((entry) => entry.value != true)
        .map((entry) => entry.key)
        .toList(growable: false);
    final summary = <String, Object?>{
      'scenario': 'shutdown',
      'phase': phase,
      'verdict': 'pending',
      'status': failure == null && failedChecks.isEmpty ? 'complete' : 'failed',
      'stage': stage,
      'error': failure?.toString(),
      'failedChecks': failedChecks,
      'checks': checks,
      'observations': observations,
      'counts': counts,
      'completedAt': DateTime.now().toUtc().toIso8601String(),
      'pendingEvidence': pendingEvidence,
    };
    await File(
      '${output.path}/shutdown-$phase-summary.json',
    ).writeAsString('${const JsonEncoder.withIndent('  ').convert(summary)}\n');
  }

  void raiseIfFailed() {
    final failedChecks = checks.entries
        .where((entry) => entry.value != true)
        .map((entry) => entry.key)
        .toList(growable: false);
    if (failedChecks.isNotEmpty) {
      throw StateError(
        'shutdown $phase checks failed: ${failedChecks.join(', ')}',
      );
    }
  }
}

/// A turn that settled durably; carried so the reopen verification can match the
/// real Thread and Turn identities instead of guessing from the current window.
class _SettledTurn {
  const _SettledTurn({required this.threadId, required this.turnId});

  final String threadId;
  final String? turnId;
}
