import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';
import 'realtime_journey.dart';

Future<void> main(List<String> args) async {
  if (args.length != 6) {
    throw ArgumentError('phase protocol VM_URL PROJECT OUTPUT COORD required');
  }
  final driver = await FlutterDriverSession.connect(vmServiceUrl: args[2]);
  final journey = ReplayRecoveryJourney(driver, args);
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
      // The original failure remains in the summary even if the window has exited.
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
      // A completed native shutdown may close the connection before its client.
    }
  }
  await File('${args[4]}/${args[0]}-summary.json').writeAsString(
    jsonEncode({
      'phase': args[0],
      'protocol': args[1],
      'status': failure == null ? 'complete' : 'failed',
      'shutdown': shutdown,
      'error': failure?.toString(),
      'humanVerdict': 'pending',
    }),
  );
  if (failure != null) Error.throwWithStackTrace(failure, failureStack!);
}

class ReplayRecoveryJourney {
  ReplayRecoveryJourney(this.driver, List<String> args)
    : phase = args[0],
      protocol = args[1],
      project = args[3],
      output = args[4],
      coord = args[5];

  final FlutterDriverSession driver;
  final String phase;
  final String protocol;
  final String project;
  final String output;
  final String coord;

  File get observed => File('$coord/observed.json');

  Future<Map<String, dynamic>> waitFor(
    bool Function(Map<String, dynamic>) predicate,
    String label,
  ) async {
    final deadline = DateTime.now().add(const Duration(seconds: 180));
    Map<String, dynamic>? last;
    while (DateTime.now().isBefore(deadline)) {
      last = await driver.readSnapshot();
      if (predicate(last)) return last;
      await Future<void>.delayed(const Duration(milliseconds: 150));
    }
    throw StateError(
      '$phase/$protocol/$label timed out: ${jsonEncode(summarize(last ?? {}))}',
    );
  }

  bool settled(Map<String, dynamic> snapshot) {
    final workspace = workspaceOf(snapshot);
    final persistence = snapshot['persistence'];
    return workspace != null &&
        workspace['isBusy'] == false &&
        workspace['syncState'] == 'ready' &&
        persistence is Map &&
        persistence['kind'] == 'ready' &&
        persistence['pendingCommits'] == 0;
  }

  Future<void> tap(String key) async {
    final finder = find.byValueKey(key);
    await driver.waitFor(finder, timeout: const Duration(seconds: 60));
    await driver.rawTap(finder);
  }

  Future<void> submit(String prompt) async {
    await tap('composer-input');
    await driver.enterText(prompt);
    await tap('composer-submit');
  }

  Future<void> answer(String action, String expected) async {
    await submit('Replay $action $protocol');
    final snapshot = await waitFor(
      (snapshot) => answerContains(snapshot, expected) && settled(snapshot),
      action,
    );
    if (answerMatchCount(snapshot, expected) != 1) {
      throw StateError('$expected was duplicated');
    }
  }

  List<Map<String, dynamic>> frozenRows(Map<String, dynamic> snapshot) {
    final rows = timelineRows(snapshot);
    if (rows.isEmpty || rows.any((row) => row['id'] is! String)) {
      throw StateError('canonical timeline identities missing');
    }
    if (rows.map((row) => row['id']).toSet().length != rows.length) {
      throw StateError('duplicate canonical timeline identity');
    }
    return [
      for (final row in rows)
        {
          for (final key in [
            'id',
            'type',
            'text',
            'channel',
            'callId',
            'toolName',
            'result',
            'status',
          ])
            if (row.containsKey(key)) key: row[key],
          'tools': [
            for (final tool in (row['tools'] as List? ?? const []))
              {
                for (final key in [
                  'itemId',
                  'callId',
                  'name',
                  'status',
                  'arguments',
                  'result',
                  'exitCode',
                ])
                  if ((tool as Map).containsKey(key)) key: tool[key],
              },
          ],
        },
    ];
  }

  Future<void> capture(String label) async {
    final snapshot = await driver.readSnapshot();
    await File('$output/$phase-$label-snapshot.json')
        .writeAsString(jsonEncode(snapshot));
    await File('$output/$phase-$label.png')
        .writeAsBytes(await driver.screenshot());
  }

  Future<void> saveObserved() async {
    final snapshot = await waitFor(settled, 'save');
    final threadId = workspaceOf(snapshot)?['threadId'];
    if (threadId is! String || threadId.isEmpty) {
      throw StateError('canonical Thread id missing');
    }
    await observed.writeAsString(
      jsonEncode({'threadId': threadId, 'rows': frozenRows(snapshot)}),
    );
  }

  Future<void> run() async {
    if (phase == 'first') {
      await tap('sidebar-open-project');
      await tap('add-project-local');
      await tap('add-project-continue-ready');
      await tap('project-path-input');
      await driver.enterText(project);
      await tap('project-path-submit');
      await answer('inspect', 'replay final $protocol first');
      final completed = await driver.readSnapshot();
      if (!timelineRows(completed).any(
        (row) =>
            row['type'] == 'commentary' &&
            row['text'] == 'replay progress $protocol first',
      )) {
        throw StateError('completed commentary disappeared from history');
      }
      await capture('tool-round');
      await answer('continue', 'replay final $protocol second');
      await submit('Replay failed $protocol');
      if (protocol == 'chat') {
        await waitFor(
          (snapshot) => timelineRows(
            snapshot,
          ).any((row) => '${row['text']}'.contains('unfinished replay note')),
          'partial-observed',
        );
        await tap('composer-stop');
      }
      await waitFor(
        (snapshot) =>
            settled(snapshot) &&
            turnStatus(snapshot) ==
                (protocol == 'chat' ? 'cancelled' : 'failed'),
        'failure-saved',
      );
      await saveObserved();
      await capture('before-close');
      return;
    }
    if (phase != 'restart' && phase != 'recheck') {
      throw ArgumentError('unknown phase $phase');
    }
    final prior =
        jsonDecode(await observed.readAsString()) as Map<String, dynamic>;
    final threadId = prior['threadId'] as String;
    await tap('thread-row-$threadId');
    final reopened = await waitFor(
      (snapshot) =>
          workspaceOf(snapshot)?['threadId'] == threadId && settled(snapshot),
      'opened',
    );
    if (jsonEncode(frozenRows(reopened)) != jsonEncode(prior['rows'])) {
      throw StateError(
        'restored timeline differs from the saved identities, order or text',
      );
    }
    await capture('restored');
    await File('$coord/$phase-opened').writeAsString('opened');
    final deadline = DateTime.now().add(const Duration(seconds: 60));
    while (!await File('$coord/$phase-quiet').exists()) {
      if (!DateTime.now().isBefore(deadline)) {
        throw StateError('coordinator did not verify quiet restoration');
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    if (phase == 'restart') {
      await answer('resume', 'replay final $protocol resumed');
      await saveObserved();
      await capture('continued');
    } else {
      if (answerMatchCount(reopened, 'replay final $protocol resumed') != 1) {
        throw StateError('restarted continuation missing or duplicated');
      }
    }
  }
}
