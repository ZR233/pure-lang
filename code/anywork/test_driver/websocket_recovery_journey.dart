// Manual native-GUI evidence journey, not a Flutter automated-test gate.
import 'dart:convert';
import 'dart:io';

import 'context_replay_recovery_journey.dart' show ReplayRecoveryJourney;
import 'flutter_driver_session.dart';
import 'realtime_journey.dart';

Future<void> main(List<String> args) async {
  await runRecoveryJourney(args, WebSocketRecoveryJourney.new);
}

Future<void> runRecoveryJourney(
  List<String> args,
  ReplayRecoveryJourney Function(FlutterDriverSession, List<String>) create, {
  String protocol = 'responsesWebSocket',
}) async {
  if (args.length != 6 && args.length != 7) {
    throw ArgumentError('phase protocol VM_URL PROJECT OUTPUT COORD required');
  }
  final driver = await FlutterDriverSession.connect(vmServiceUrl: args[2]);
  final journey = create(driver, args);
  Object? failure;
  StackTrace? failureStack;
  var shutdown = 'failed';
  try {
    await journey.run();
  } catch (error, stack) {
    failure = error;
    failureStack = stack;
    try {
      await journey.capture('failure');
    } catch (_) {
      // Preserve the original failure if the window is no longer available.
    }
  }
  try {
    final reply = jsonDecode(
      await driver.requestData(
        'shutdown',
        timeout: const Duration(seconds: 60),
      ),
    );
    if (reply is! Map || reply['shutdown'] != 'completed') {
      throw StateError('native shutdown did not complete');
    }
    shutdown = 'completed';
  } catch (error, stack) {
    failure ??= error;
    failureStack ??= stack;
  } finally {
    try {
      await driver.close().timeout(const Duration(seconds: 5));
    } catch (_) {
      // Normal shutdown can close the VM connection before its client.
    }
  }
  await File('${args[4]}/${args[0]}-summary.json').writeAsString(
    jsonEncode({
      'phase': args[0],
      'protocol': protocol,
      'status': failure == null ? 'complete' : 'failed',
      'shutdown': shutdown,
      'error': failure?.toString(),
      'humanVerdict': 'pending',
    }),
  );
  if (failure != null) Error.throwWithStackTrace(failure, failureStack!);
}

class WebSocketRecoveryJourney extends ReplayRecoveryJourney {
  WebSocketRecoveryJourney(super.driver, super.args);

  @override
  Future<void> tap(String key) async {
    stdout.writeln('${DateTime.now().toIso8601String()} tap $key');
    await super.tap(key);
    stdout.writeln('${DateTime.now().toIso8601String()} tapped $key');
  }

  @override
  Future<Map<String, dynamic>> waitFor(
    bool Function(Map<String, dynamic>) predicate,
    String label,
  ) async {
    final deadline = DateTime.now().add(const Duration(seconds: 180));
    Map<String, dynamic>? last;
    String? prior;
    while (DateTime.now().isBefore(deadline)) {
      last = await driver.readSnapshot();
      final observation = jsonEncode(summarize(last));
      if (observation != prior) {
        prior = observation;
        stdout.writeln(
          '${DateTime.now().toIso8601String()} $label $observation',
        );
        await File('$output/$phase-observations.jsonl').writeAsString(
          '${jsonEncode({'at': DateTime.now().toIso8601String(), 'stage': label, 'snapshot': last})}\n',
          mode: FileMode.append,
        );
      }
      if (predicate(last)) return last;
      await Future<void>.delayed(const Duration(milliseconds: 150));
    }
    throw StateError(
      '$phase/ws/$label timed out: ${jsonEncode(summarize(last ?? {}))}',
    );
  }

  bool contains(Map<String, dynamic> snapshot, String text) =>
      timelineRows(snapshot).any((row) => '${row['text']}'.contains(text));

  void single(Map<String, dynamic> snapshot, String text) {
    if (timelineRows(snapshot).where((row) => row['text'] == text).length !=
        1) {
      throw StateError('$text missing or duplicated');
    }
  }

  Future<void> coordinator(String stage) async {
    await File('$coord/$stage-opened').writeAsString('opened');
    final deadline = DateTime.now().add(const Duration(seconds: 60));
    while (!await File('$coord/$stage-quiet').exists()) {
      if (!DateTime.now().isBefore(deadline)) {
        throw StateError('coordinator did not verify $stage');
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
  }

  @override
  Future<void> run() async {
    if (phase == 'restart') {
      final prior = jsonDecode(await observed.readAsString()) as Map;
      final id = prior['threadId'] as String;
      await tap('thread-row-$id');
      final restored = await waitFor(
        (snapshot) =>
            workspaceOf(snapshot)?['threadId'] == id && settled(snapshot),
        'reopened',
      );
      if (jsonEncode(frozenRows(restored)) != jsonEncode(prior['rows'])) {
        throw StateError('reopened history changed identity, order or text');
      }
      await capture('restored');
      await coordinator('restart');
      return;
    }
    if (phase != 'first') throw ArgumentError('unknown phase $phase');
    await tap('sidebar-open-project');
    await tap('add-project-local');
    await tap('add-project-continue-ready');
    await tap('project-path-input');
    await driver.enterText(project);
    await tap('project-path-submit');

    await submit('WebSocket recover');
    final retrying = await waitFor(
      (snapshot) =>
          contains(snapshot, 'WS abandoned fragment') &&
          contains(snapshot, '正在重试（1/5）') &&
          isBusy(snapshot),
      'retrying',
    );
    final recoveringTurn = turnId(retrying);
    await capture('retrying');
    final recovered = await waitFor(
      (snapshot) =>
          answerContains(snapshot, 'WS recovered answer') &&
          contains(snapshot, '连接已恢复') &&
          settled(snapshot),
      'recovered',
    );
    if (turnId(recovered) != recoveringTurn ||
        turnStatus(recovered) != 'completed') {
      throw StateError(
        'transparent recovery changed or failed the original Turn',
      );
    }
    single(recovered, 'WS abandoned fragment');
    single(recovered, 'WS recovered answer');
    await capture('recovered');

    final priorIds = timelineRows(recovered).map((row) => row['id']).toSet();
    await submit('WebSocket cancel');
    await waitFor(
      (snapshot) =>
          contains(snapshot, 'WS cancelled fragment') &&
          timelineRows(snapshot).any(
            (row) =>
                '${row['id']}'.contains('recovery:retry:1') &&
                !priorIds.contains(row['id']),
          ) &&
          turnId(snapshot) != recoveringTurn &&
          isBusy(snapshot),
      'cancel-backoff',
    );
    await capture('cancel-backoff');
    await tap('composer-stop');
    await waitFor(
      (snapshot) =>
          settled(snapshot) &&
          turnStatus(snapshot) == 'cancelled' &&
          contains(snapshot, '连接恢复已取消'),
      'cancelled',
    );
    await capture('cancelled');
    // The coordinator observes fixture counters beyond the advertised 30s hint.
    await coordinator('cancel');
    await submit('WebSocket next');
    final next = await waitFor(
      (snapshot) =>
          answerContains(snapshot, 'WS next turn answer') && settled(snapshot),
      'next-turn',
    );
    if (turnStatus(next) != 'completed') throw StateError('next Turn failed');
    for (final text in [
      'WS abandoned fragment',
      'WS recovered answer',
      'WS cancelled fragment',
      'WS next turn answer',
    ]) {
      single(next, text);
    }
    await saveObserved();
    await capture('before-close');
  }
}
