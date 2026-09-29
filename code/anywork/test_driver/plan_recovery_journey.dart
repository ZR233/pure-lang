import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';
// Public snapshot helpers shared with the realtime/history journeys: this
// journey reads the same Driver projection and must not fork a second reading.
import 'realtime_journey.dart';

/// The Approve-branch prompt; it matches `GUI_PLAN_RECOVERY_APPROVE_PROMPT`.
const _approvePrompt = 'Local GUI plan recovery approve fixture';

/// The Revise-branch prompt; it matches `GUI_PLAN_RECOVERY_REVISE_PROMPT`.
const _revisePrompt = 'Local GUI plan recovery revise fixture';

/// The Approve-branch Plan markdown; it matches `GUI_PLAN_RECOVERY_APPROVE_PLAN`
/// byte for byte, so the restored card body and the continuation identity are
/// equality checks, never substring guesses.
const _approvePlan =
    '# Plan Recovery Approve Blueprint\n'
    '\n'
    '## Goal\n'
    '\n'
    'Prove a pending Plan confirmation survives a normal GUI shutdown and restart.\n'
    '\n'
    '## Steps\n'
    '\n'
    '1. Keep the pending confirmation restorable across a normal shutdown.\n'
    '2. Reopen the same session and restore the exact Plan body and identity.\n'
    '3. Approve the restored Plan and continue exactly once.';

/// The Revise-branch Plan markdown; it matches `GUI_PLAN_RECOVERY_REVISE_PLAN`.
const _revisePlan =
    '# Plan Recovery Revision Blueprint\n'
    '\n'
    '## Goal\n'
    '\n'
    'Exercise the Revise path on a restored pending Plan confirmation.\n'
    '\n'
    '## Steps\n'
    '\n'
    '1. Restore the pending Plan after the second normal shutdown.\n'
    '2. Request a revision with concrete feedback.\n'
    '3. Approve the rewritten Plan and finish the continuation.';

/// The rewritten Plan; it matches `GUI_PLAN_RECOVERY_REVISED_PLAN`.
const _revisedPlan =
    '# Plan Recovery Revision Blueprint\n'
    '\n'
    '## Goal\n'
    '\n'
    'Exercise the Revise path on a restored pending Plan confirmation.\n'
    '\n'
    '## Revised Steps\n'
    '\n'
    '1. Restore the pending Plan after the second normal shutdown.\n'
    '2. Request a revision with concrete feedback that tightens the wording.\n'
    '3. Approve the rewritten Plan and finish the continuation exactly once.';

/// Final answers of the two continuations; they match
/// `GUI_PLAN_RECOVERY_APPROVE_ANSWER` / `GUI_PLAN_RECOVERY_REVISE_ANSWER`.
const _approveAnswer = 'plan recovery approve answer complete';
const _reviseAnswer = 'plan recovery revise answer complete';

/// The revision feedback the journey submits through the plan feedback bar.
const _revisionFeedback =
    'Tighten the wording of the final step and keep the delivery scope '
    'unchanged.';

/// Turn statuses from which the awaited settle can no longer arrive. A
/// `completed` Turn is deliberately absent: while persistence is still
/// flushing it must keep waiting for the durable settle, not abort.
const _failedTurnStatuses = <String>{'failed', 'cancelled', 'budgetLimited'};

/// Marker files the coordinator writes after it proved the reopened card window
/// recorded no additional provider request.
const _quietRestartMarker = 'plan-recovery-quiet-restart';
const _quietRecheckMarker = 'plan-recovery-quiet-recheck';

/// The identity evidence shared between journey phases through the output dir.
const _observedFile = 'plan-recovery-observed.json';

Future<void> main(List<String> args) async {
  if (args.length != 5 ||
      !const ['first', 'restart', 'recheck'].contains(args[0])) {
    stderr.writeln(
      'usage: plan_recovery_journey.dart first|restart|recheck VM_URL '
      'PROJECT_DIR OUTPUT_DIR COORD_DIR',
    );
    exitCode = 64;
    return;
  }
  final phase = args[0];
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
  final journey = PlanRecoveryJourney(
    driver: driver,
    phase: phase,
    project: args[2],
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
  String phase,
  Directory coord,
  Directory output,
  Object error,
) async {
  try {
    await File('${coord.path}/plan-recovery-stage')
        .writeAsString('${phase}_connect_failed');
    final summary = <String, Object?>{
      'scenario': 'plan-recovery',
      'phase': phase,
      'verdict': 'pending',
      'status': 'failed',
      'stage': '${phase}_connect_failed',
      'error': '$error',
      'shutdownError': null,
      'failedChecks': const <String>[],
      'checks': const <String, Object?>{},
      'observations': const <String, Object?>{},
      'counts': const <String, Object?>{},
      'completedAt': DateTime.now().toUtc().toIso8601String(),
      'pendingEvidence': const <String>[],
    };
    await File(
      '${output.path}/plan-recovery-$phase-summary.json',
    ).writeAsString('${const JsonEncoder.withIndent('  ').convert(summary)}\n');
  } catch (_) {
    // The coordinator still records the exit status and the sanitized log.
  }
}

/// Acceptance journey for the Plan confirmation recovery contract.
///
/// Three GUI lifecycles share one isolated home and one strict fixture script.
/// The `first` phase submits a real `plan_submit` through the GUI and shuts the
/// GUI down normally while the confirmation is pending. The `restart` phase
/// proves the reopened session restores the exact Plan body and interaction
/// identity, that no provider request happens before the answer (the
/// coordinator's quiet marker), then approves and creates the Revise-branch
/// session before a second normal shutdown. The `recheck` phase restores that
/// second pending Plan, answers Revise (which must produce a rewritten Plan
/// confirmation with a fresh interaction identity), approves it, and proves
/// neither answered Plan re-pops. The strict fixture script rejects any extra
/// or duplicated model request, so the journey itself never needs to guess
/// whether a continuation ran twice.
class PlanRecoveryJourney {
  PlanRecoveryJourney({
    required this.driver,
    required this.phase,
    required this.project,
    required this.output,
    required this.coord,
  });

  final FlutterDriverSession driver;
  final String phase;
  final String project;
  final Directory output;
  final Directory coord;
  final Map<String, Object?> checks = <String, Object?>{};
  final Map<String, Object?> observations = <String, Object?>{};
  final Map<String, Object?> counts = <String, Object?>{};
  String stage = 'created';
  String? shutdownError;

  /// Observations that stay with the human reviewer rather than being asserted
  /// here; the coordinator records the provider-side request counts.
  final List<String> pendingEvidence = <String>[
    'the coordinator owns the fixture request counters: the quiet windows '
        'before each answer are proved from the fixture status file, not from '
        'the Driver clock alone (see plan-recovery-requests-*.json)',
    'the strict fixture script rejects any unexpected model request, so a '
        'duplicated continuation or an early execution surfaces as a rejected '
        'request in fixture-status.json rather than a GUI observation',
  ];

  File get stageFile => File('${coord.path}/plan-recovery-stage');

  bool get failed => checks.values.any((value) => value != true);

  Future<void> run() async {
    switch (phase) {
      case 'first':
        await _runFirst();
      case 'restart':
        await _runRestart();
      case 'recheck':
        await _runRecheck();
      default:
        throw StateError('plan-recovery: unknown phase $phase');
    }
  }

  // ----------------------------------------------------------------- first --

  /// Submits the Approve-branch Plan and leaves the pending card on screen.
  Future<void> _runFirst() async {
    await mark('plan_recovery_first_connected');
    await _openProject();
    // A fresh home opens on the start page: no Thread exists yet (`workspace`
    // is null), so waiting for one before submitting can never succeed. The
    // submission itself creates session A through the real start-page composer
    // (`startNewThread`), exactly like the statistics journey's first turn.
    final existingThread = threadIdOf(await snapshot());
    if (existingThread != null) {
      throw StateError(
        'plan-recovery: first phase expected the start page, found thread '
        '$existingThread',
      );
    }
    await _submit(_approvePrompt);
    final initial = await waitFor(
      (snapshot) => (threadIdOf(snapshot) ?? '').isNotEmpty,
      'first_thread_ready',
    );
    final threadId = threadIdOf(initial)!;
    final card = await _waitForPlanCard('first_approve', _approvePlan);
    // The Thread is brand new, so its only Turn is the submission's: an
    // attached card Turn keeps proving the submission drove the model Turn.
    checks['firstCardTurnAttached'] = (turnId(card)?.isNotEmpty) ?? false;
    final interaction = activeInteractionOf(card)!;
    checks['firstCardIsUserInput'] = interaction['kind'] == 'userInput';
    checks['firstCardBodyMatchesPlan'] = interaction['body'] == _approvePlan;
    counts['approveThreadId'] = threadId;
    counts['approveInteractionId'] = interaction['id'];
    counts['approveTurnId'] = interaction['turnId'];
    observations['firstCard'] = summarize(card);
    await _openPlanDetails('first');
    await shot('first-card');
    await _writeObserved(<String, Object?>{
      'approveBranch': <String, Object?>{
        'threadId': threadId,
        'interactionId': interaction['id'],
        'planMarkdown': interaction['body'],
        'turnId': interaction['turnId'],
      },
    });
    await mark('plan_recovery_first_card');
  }

  // --------------------------------------------------------------- restart --

  /// Restores and approves the first Plan, then stages the Revise branch.
  Future<void> _runRestart() async {
    await mark('plan_recovery_restart_connected');
    final observed = await _readObserved();
    final branch = _branchOf(observed, 'approveBranch');
    final threadId = branch['threadId'] as String;
    final expectedInteractionId = branch['interactionId'] as String;
    await _ensureThreadOpen(threadId, 'restart_approve');
    final card = await _waitForPlanCard(
      'restart_approve',
      _approvePlan,
      expectedInteractionId: expectedInteractionId,
    );
    final interaction = activeInteractionOf(card)!;
    checks['approveIdentityRestored'] =
        interaction['id'] == branch['interactionId'];
    checks['approveTurnIdentityRestored'] =
        interaction['turnId'] == branch['turnId'];
    checks['approveBodyRestored'] = interaction['body'] == _approvePlan;
    counts['reopenedApproveInteractionId'] = interaction['id'];
    observations['restartCard'] = summarize(card);
    await _openPlanDetails('restart');
    await shot('restart-card-restored');
    await mark('plan_recovery_reopened');
    // The coordinator proves this window recorded no provider request before
    // the journey is allowed to answer.
    await _awaitMarker(
      _quietRestartMarker,
      'restart_quiet',
      const Duration(seconds: 180),
    );

    final priorTurn = turnId(card)!;
    final priorUserRows = userMessageCount(card);
    await _tapForEffect(
      'plan-approve',
      (snapshot) => activeInteractionOf(snapshot) == null,
      'restart_approve',
    );
    await _cardDismissed('restart_approve');
    await _awaitAnswered(
      'restart_approve',
      priorTurn,
      _approveAnswer,
      priorUserRows,
    );
    await shot('restart-answered');

    // Stage the Revise branch: a second session with its own pending Plan, so
    // the second normal shutdown also happens while a confirmation is pending.
    await _tapForEffect(
      'sidebar-new-session',
      threadDeselected,
      'restart_new_session',
    );
    await driver.waitFor(
      find.byValueKey('composer-input'),
      timeout: const Duration(seconds: 60),
    );
    // `beginNewThread` returns to the start page (`selectedThreadId` null);
    // session B is created by the submission itself, so the prompt is sent
    // first and the distinct Thread id is awaited afterwards.
    await _submit(_revisePrompt);
    final reviseInitial = await waitFor((snapshot) {
      final current = threadIdOf(snapshot);
      return current != null && current != threadId;
    }, 'restart_revise_thread_ready');
    final reviseThreadId = threadIdOf(reviseInitial)!;
    final reviseCard = await _waitForPlanCard('restart_revise', _revisePlan);
    checks['reviseCardTurnAttached'] =
        (turnId(reviseCard)?.isNotEmpty) ?? false;
    final reviseInteraction = activeInteractionOf(reviseCard)!;
    checks['reviseCardBodyMatchesPlan'] =
        reviseInteraction['body'] == _revisePlan;
    checks['reviseInteractionDistinct'] =
        reviseInteraction['id'] != expectedInteractionId;
    counts['reviseThreadId'] = reviseThreadId;
    counts['reviseInteractionId'] = reviseInteraction['id'];
    observations['restartReviseCard'] = summarize(reviseCard);
    await _openPlanDetails('restart_revise');
    await shot('restart-revise-card');
    await _writeObserved(<String, Object?>{
      'reviseBranch': <String, Object?>{
        'threadId': reviseThreadId,
        'interactionId': reviseInteraction['id'],
        'planMarkdown': reviseInteraction['body'],
        'turnId': reviseInteraction['turnId'],
      },
    });
    await mark('plan_recovery_revise_card');
  }

  // --------------------------------------------------------------- recheck --

  /// Restores the second Plan, answers Revise then Approve, and proves neither
  /// answered Plan re-pops.
  Future<void> _runRecheck() async {
    await mark('plan_recovery_recheck_connected');
    final observed = await _readObserved();
    final branch = _branchOf(observed, 'reviseBranch');
    final threadId = branch['threadId'] as String;
    final expectedInteractionId = branch['interactionId'] as String;
    final approveThreadId =
        _branchOf(observed, 'approveBranch')['threadId'] as String;
    await _ensureThreadOpen(threadId, 'recheck_revise');
    final card = await _waitForPlanCard(
      'recheck_revise',
      _revisePlan,
      expectedInteractionId: expectedInteractionId,
    );
    final interaction = activeInteractionOf(card)!;
    checks['reviseIdentityRestored'] =
        interaction['id'] == branch['interactionId'];
    checks['reviseTurnIdentityRestored'] =
        interaction['turnId'] == branch['turnId'];
    checks['reviseBodyRestored'] = interaction['body'] == _revisePlan;
    counts['reopenedReviseInteractionId'] = interaction['id'];
    observations['recheckCard'] = summarize(card);
    await _openPlanDetails('recheck');
    await shot('recheck-card-restored');
    await mark('plan_recovery_recheck_reopened');
    await _awaitMarker(
      _quietRecheckMarker,
      'recheck_quiet',
      const Duration(seconds: 180),
    );

    // Revise: the feedback bar is the only writer of the Revise answer. The
    // focus tap goes through the raw pointer path like the composer: the
    // stock `Tap` command's `hitTestable()` pre-filter never yields
    // candidates on the Linux embedder.
    await _tap('plan-feedback-input');
    await driver.enterText(_revisionFeedback);
    await _tapForEffect(
      'plan-submit-revision',
      (snapshot) =>
          activeInteractionOf(snapshot) == null ||
          activeInteractionOf(snapshot)!['id'] != expectedInteractionId,
      'recheck_revise',
    );
    await _cardDismissed(
      'recheck_revise',
      replacedInteractionId: expectedInteractionId,
    );

    // The rewritten Plan must arrive as a new pending confirmation with its own
    // interaction identity; a stale identity would mean the old card was reused.
    final revisedCard = await _waitForPlanCard('recheck_revised', _revisedPlan);
    final revisedInteraction = activeInteractionOf(revisedCard)!;
    checks['revisedCardBodyMatchesPlan'] =
        revisedInteraction['body'] == _revisedPlan;
    checks['revisedCardNewIdentity'] =
        revisedInteraction['id'] is String &&
        (revisedInteraction['id'] as String).isNotEmpty &&
        revisedInteraction['id'] != expectedInteractionId;
    counts['revisedInteractionId'] = revisedInteraction['id'];
    observations['recheckRevisedCard'] = summarize(revisedCard);
    await _openPlanDetails('recheck_revised');
    await shot('recheck-revised-card');

    final revisedTurn = turnId(revisedCard)!;
    final revisedUserRows = userMessageCount(revisedCard);
    await _tapForEffect(
      'plan-approve',
      (snapshot) => activeInteractionOf(snapshot) == null,
      'recheck_revised',
    );
    await _cardDismissed('recheck_revised');
    await _awaitAnswered(
      'recheck_revised',
      revisedTurn,
      _reviseAnswer,
      revisedUserRows,
    );
    await shot('recheck-answered');

    // Neither answered Plan may re-pop, in either reopened session.
    await _ensureThreadOpen(approveThreadId, 'recheck_approve_thread');
    final approveThread = await _assertNoPendingPlan('recheck_approve_thread');
    checks['answeredApprovePlanNotRepop'] =
        activeInteractionOf(approveThread) == null;
    observations['recheckApproveThreadFinal'] = summarize(approveThread);
    await _ensureThreadOpen(threadId, 'recheck_revise_thread');
    final reviseThread = await _assertNoPendingPlan('recheck_revise_thread');
    checks['answeredRevisePlanNotRepop'] =
        activeInteractionOf(reviseThread) == null;
    observations['recheckReviseThreadFinal'] = summarize(reviseThread);
    await shot('recheck-no-repop');
    await mark('plan_recovery_recheck_complete');
  }

  // --------------------------------------------------------------- helpers --

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

  /// Taps [key] and proves the action through an explicit desired widget
  /// state instead of the tap response alone.
  ///
  /// The Driver transport can drop a command response after the app already
  /// performed the action (the 03 run opened the details panel while the
  /// rawTap response never arrived), so a timed-out tap with the desired
  /// widget already visible counts as done. The tap is never replayed, any
  /// other error propagates untouched, and a missing effect rethrows the
  /// original timeout.
  Future<void> _tapForWidget(
    String key,
    String desiredKey,
    String label, {
    required bool present,
  }) async {
    try {
      await _tap(key);
    } on TimeoutException catch (error, stackTrace) {
      var recovered = false;
      try {
        // The tap timeout closed the observation client; a snapshot read
        // re-establishes it before the widget check.
        await snapshot();
        recovered = await _widgetHolds(desiredKey, present: present);
      } on Object {
        // The confirmation itself failed: the original timeout stays the
        // reported failure.
      }
      if (!recovered) {
        Error.throwWithStackTrace(error, stackTrace);
      }
      counts['${label}TapConfirmedByEffect'] = true;
      return;
    }
    if (present) {
      await driver.waitFor(
        find.byValueKey(desiredKey),
        timeout: const Duration(seconds: 30),
      );
    } else {
      await driver.waitForAbsent(
        find.byValueKey(desiredKey),
        timeout: const Duration(seconds: 30),
      );
    }
  }

  /// Whether the widget exists (or is gone), bounded for one confirmation.
  Future<bool> _widgetHolds(String key, {required bool present}) async {
    try {
      if (present) {
        await driver.waitFor(
          find.byValueKey(key),
          timeout: const Duration(seconds: 10),
        );
      } else {
        await driver.waitForAbsent(
          find.byValueKey(key),
          timeout: const Duration(seconds: 10),
        );
      }
      return true;
    } on Object {
      return false;
    }
  }

  /// Taps [key] and proves the action through a snapshot-level [effect].
  ///
  /// Same recovery rule as [_tapForWidget]: a timed-out tap whose effect
  /// arrives within [grace] counts as done and is never replayed; otherwise
  /// the original timeout is rethrown. The effect wait reads through the
  /// reconnecting snapshot channel.
  Future<void> _tapForEffect(
    String key,
    bool Function(Map<String, dynamic>) effect,
    String label, {
    Duration grace = const Duration(seconds: 30),
  }) async {
    try {
      await _tap(key);
    } on TimeoutException catch (error, stackTrace) {
      var recovered = false;
      try {
        await waitFor(effect, '${label}_tap_effect', timeout: grace);
        recovered = true;
      } on Object {
        // The effect never arrived: the original timeout stays the failure.
      }
      if (!recovered) {
        Error.throwWithStackTrace(error, stackTrace);
      }
      counts['${label}TapConfirmedByEffect'] = true;
      return;
    }
  }

  String? threadIdOf(Map<String, dynamic> snapshot) {
    final value = workspaceOf(snapshot)?['threadId'];
    return value is String ? value : null;
  }

  /// Whether the start page is active again (no Thread selected), the effect
  /// of the sidebar's new-session action.
  bool threadDeselected(Map<String, dynamic> snapshot) {
    final navigation = snapshot['navigation'];
    return navigation is Map && navigation['selectedThreadId'] == null;
  }

  Map<String, dynamic>? activeInteractionOf(Map<String, dynamic> snapshot) {
    final value = workspaceOf(snapshot)?['activeInteraction'];
    return value is Map<String, dynamic> ? value : null;
  }

  /// The pending-confirmation settle projection.
  ///
  /// A Thread waiting on a Plan confirmation is deliberately not `idle`: its
  /// status is `waitingInteraction`, so the generic idle-settle wait never
  /// applies here. What must still hold is that nothing is running and durable
  /// persistence is ready with nothing pending, which is exactly the checkpoint
  /// state a later normal shutdown has to persist.
  bool planCardSettled(Map<String, dynamic> snapshot) {
    final workspace = workspaceOf(snapshot);
    final persistence = snapshot['persistence'];
    return workspace != null &&
        workspace['threadStatus'] == 'waitingInteraction' &&
        workspace['isBusy'] == false &&
        workspace['syncState'] == 'ready' &&
        persistence is Map &&
        persistence['kind'] == 'ready' &&
        persistence['pendingCommits'] == 0;
  }

  /// Whether durable persistence is faulted: a write failed with an error
  /// (`degraded`) or the writer is blocked. Both need intervention, so a wait
  /// for the durable settle reports them immediately instead of burning its
  /// timeout; `flushing` and `recovering` are healthy transients that keep
  /// waiting.
  bool persistenceFaulted(Map<String, dynamic> snapshot) {
    final persistence = snapshot['persistence'];
    return persistence is Map &&
        const ['degraded', 'blocked'].contains(persistence['kind']);
  }

  /// Visible user rows in the current timeline window. The plan continuation is
  /// delivered with the hidden presentation, so it must never add one.
  int userMessageCount(Map<String, dynamic> snapshot) =>
      timelineRows(snapshot)
          .where((row) => row['type'] == 'userMessage')
          .length;

  /// Waits for the pending Plan confirmation card and its durable checkpoint.
  ///
  /// When [expectedInteractionId] is non-null the restored identity must equal
  /// it; the caller records the equality as its own check. Brand-new sessions
  /// prove the submission drove the Turn through the attached card Turn check
  /// instead: a fresh Thread has no earlier Turn to differ from.
  Future<Map<String, dynamic>> _waitForPlanCard(
    String label,
    String expectedMarkdown, {
    String? expectedInteractionId,
  }) async {
    await driver.waitFor(
      find.byValueKey('plan-summary'),
      timeout: const Duration(seconds: 90),
    );
    await driver.waitFor(
      find.byValueKey('plan-approve'),
      timeout: const Duration(seconds: 30),
    );
    await driver.waitFor(
      find.byValueKey('plan-feedback-input'),
      timeout: const Duration(seconds: 30),
    );
    // The pending confirmation must also be durably checkpointed before the
    // journey relies on it, so a shutdown here can never lose the card.
    final card = await waitFor(
      (snapshot) {
        final interaction = activeInteractionOf(snapshot);
        return interaction != null &&
            interaction['body'] == expectedMarkdown &&
            (expectedInteractionId == null ||
                interaction['id'] == expectedInteractionId) &&
            planCardSettled(snapshot);
      },
      '${label}_card_settled',
      timeout: const Duration(seconds: 180),
      abort: threadFaulted,
    );
    // The normal composer must be replaced while the Plan is pending.
    try {
      await driver.waitForAbsent(
        find.byValueKey('composer-input'),
        timeout: const Duration(seconds: 10),
      );
      checks['${label}ComposerReplaced'] = true;
    } on Object {
      checks['${label}ComposerReplaced'] = false;
    }
    return card;
  }

  /// Opens and closes the read-only Plan details panel through the product UI.
  Future<void> _openPlanDetails(String label) async {
    await _tapForWidget(
      'plan-summary',
      'plan-details',
      '$label-details-open',
      present: true,
    );
    await driver.waitFor(
      find.byValueKey('plan-details-scroll'),
      timeout: const Duration(seconds: 30),
    );
    await shot('$label-details');
    await _tapForWidget(
      'plan-details-close',
      'plan-details',
      '$label-details-close',
      present: false,
    );
  }

  /// Waits until the confirmation card is dismissed.
  ///
  /// On the Approve path nothing replaces the card, so its widgets, the
  /// composer and the interaction itself must all be gone. On the Revise path
  /// a rewritten card follows immediately and the transient gap between the
  /// two cards is shorter than a reliable poll, so the dismissal fact there is
  /// the old interaction identity no longer being active; the caller then
  /// verifies the replacement card itself (widgets, body and identity).
  Future<void> _cardDismissed(
    String label, {
    String? replacedInteractionId,
  }) async {
    if (replacedInteractionId == null) {
      await driver.waitForAbsent(
        find.byValueKey('plan-summary'),
        timeout: const Duration(seconds: 60),
      );
      await driver.waitForAbsent(
        find.byValueKey('plan-approve'),
        timeout: const Duration(seconds: 30),
      );
      await driver.waitForAbsent(
        find.byValueKey('plan-feedback-input'),
        timeout: const Duration(seconds: 30),
      );
      await driver.waitFor(
        find.byValueKey('composer-input'),
        timeout: const Duration(seconds: 60),
      );
    }
    // The dismissal fact is the interaction itself going away - or, on the
    // Revise path, being replaced - while the continuation Turn may already be
    // running, so busy-ness deliberately stays out of this predicate.
    final dismissed = await waitFor(
      (snapshot) {
        final interaction = activeInteractionOf(snapshot);
        return interaction == null ||
            (replacedInteractionId != null &&
                interaction['id'] != replacedInteractionId);
      },
      '${label}_card_dismissed',
      timeout: const Duration(seconds: 60),
    );
    checks['${label}CardDismissed'] = true;
    counts['${label}DismissedAt'] = DateTime.now().millisecondsSinceEpoch;
    observations['${label}Dismissed'] = summarize(dismissed);
  }

  /// Waits for the continuation Turn to complete and settle with exactly one
  /// answer and no new visible user row.
  Future<Map<String, dynamic>> _awaitAnswered(
    String label,
    String priorTurnId,
    String answer,
    int priorUserRows,
  ) async {
    final running = await waitFor(
      (snapshot) => turnId(snapshot) != null && turnId(snapshot) != priorTurnId,
      '${label}_continuation_running',
      timeout: const Duration(seconds: 90),
      abort: threadFaulted,
    );
    final turn = turnId(running)!;
    final settled = await waitFor(
      (snapshot) =>
          turnId(snapshot) == turn &&
          turnStatus(snapshot) == 'completed' &&
          isDurableSettled(snapshot),
      '${label}_continuation_settled',
      timeout: const Duration(seconds: 240),
      // Only genuine failures abort: a completed Turn may still be flushing
      // to durable storage, which is exactly the settle being waited for.
      abort: (snapshot) =>
          turnFailed(snapshot) ||
          persistenceFaulted(snapshot) ||
          (turnId(snapshot) == turn &&
              _failedTurnStatuses.contains(turnStatus(snapshot))),
    );
    final matches = answerMatchCount(settled, answer);
    checks['${label}ContinuationAnswered'] = matches >= 1;
    checks['${label}AnswerExactlyOnce'] = matches == 1;
    checks['${label}NoVisibleContinuationUserRow'] =
        userMessageCount(settled) == priorUserRows;
    counts['${label}AnswerMatches'] = matches;
    counts['${label}ContinuationTurnId'] = turn;
    return settled;
  }

  /// Ensures exactly the target session is the open workspace.
  Future<void> _ensureThreadOpen(String threadId, String label) async {
    await driver.waitFor(
      find.byValueKey('thread-row-$threadId'),
      timeout: const Duration(seconds: 60),
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
    await _tapForEffect(
      'thread-row-$threadId',
      (snapshot) => threadIdOf(snapshot) == threadId,
      '${label}_open',
      grace: const Duration(seconds: 60),
    );
    await waitFor(
      (snapshot) => threadIdOf(snapshot) == threadId,
      '${label}_opened',
      timeout: const Duration(seconds: 60),
    );
  }

  /// Proves no pending Plan confirmation exists in the current workspace.
  Future<Map<String, dynamic>> _assertNoPendingPlan(String label) async {
    await driver.waitForAbsent(
      find.byValueKey('plan-summary'),
      timeout: const Duration(seconds: 30),
    );
    await driver.waitForAbsent(
      find.byValueKey('plan-approve'),
      timeout: const Duration(seconds: 30),
    );
    await driver.waitFor(
      find.byValueKey('composer-input'),
      timeout: const Duration(seconds: 60),
    );
    return waitFor(
      (snapshot) =>
          activeInteractionOf(snapshot) == null && isDurableSettled(snapshot),
      '${label}_no_pending_plan',
      timeout: const Duration(seconds: 60),
    );
  }

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

  /// Reads the shared identity evidence written by earlier phases.
  Future<Map<String, dynamic>> _readObserved() async {
    final file = File('${output.path}/$_observedFile');
    if (!await file.exists()) {
      throw StateError(
        'plan-recovery: $phase needs ${file.path} from an earlier phase',
      );
    }
    final value = jsonDecode(await file.readAsString());
    if (value is! Map<String, dynamic>) {
      throw StateError(
        'plan-recovery: observed identity file is not an object',
      );
    }
    return value;
  }

  Map<String, dynamic> _branchOf(Map<String, dynamic> observed, String branch) {
    final value = observed[branch];
    if (value is! Map<String, dynamic> ||
        (value['threadId'] as String?) == null ||
        (value['interactionId'] as String?) == null ||
        (value['turnId'] as String?) == null) {
      throw StateError(
        'plan-recovery: observed identity file has no complete $branch branch',
      );
    }
    return value;
  }

  /// Merges one branch into the shared identity file without dropping earlier
  /// branches, so later phases can still read them.
  Future<void> _writeObserved(Map<String, Object?> branch) async {
    final file = File('${output.path}/$_observedFile');
    Map<String, dynamic> current = <String, dynamic>{};
    if (await file.exists()) {
      final value = jsonDecode(await file.readAsString());
      if (value is Map<String, dynamic>) current = value;
    }
    current.addAll(branch);
    await file.writeAsString(
      '${const JsonEncoder.withIndent('  ').convert(current)}\n',
    );
  }

  Future<void> mark(String value) async {
    stage = value;
    // The per-stage marker is an append-only fact: shutdown and the final
    // summary rewrite the latest-stage file within moments, so a coordinator
    // polling that file's content could skip a stage between two polls.
    await File('${coord.path}/plan-recovery-stage-$value').writeAsString(value);
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

  Future<void> shot(String name) async {
    await File('${output.path}/plan-recovery-$name.png')
        .writeAsBytes(await driver.screenshot());
  }

  /// Saves the last snapshot and a screenshot for a failed journey.
  Future<void> captureFailure() async {
    try {
      await File(
        '${output.path}/plan-recovery-$phase-failure-snapshot.json',
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
      await File('${output.path}/plan-recovery-$phase-failure.png')
          .writeAsBytes(png);
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
      'scenario': 'plan-recovery',
      'phase': phase,
      'verdict': 'pending',
      'status': failure == null && failedChecks.isEmpty ? 'complete' : 'failed',
      'stage': stage,
      'error': failure?.toString(),
      'shutdownError': shutdownFailure?.toString() ?? shutdownError,
      'failedChecks': failedChecks,
      'checks': checks,
      'observations': observations,
      'counts': counts,
      'completedAt': DateTime.now().toUtc().toIso8601String(),
      'pendingEvidence': pendingEvidence,
    };
    await File(
      '${output.path}/plan-recovery-$phase-summary.json',
    ).writeAsString('${const JsonEncoder.withIndent('  ').convert(summary)}\n');
  }

  void raiseIfFailed(Object? shutdownFailure) {
    final failedChecks = checks.entries
        .where((entry) => entry.value != true)
        .map((entry) => entry.key)
        .toList(growable: false);
    if (failedChecks.isNotEmpty) {
      throw StateError(
        'plan-recovery $phase checks failed: ${failedChecks.join(', ')}',
      );
    }
    if (shutdownFailure != null || shutdownError != null) {
      throw StateError(
        'plan-recovery $phase native GUI shutdown failed: '
        '${shutdownFailure ?? shutdownError}',
      );
    }
  }
}
