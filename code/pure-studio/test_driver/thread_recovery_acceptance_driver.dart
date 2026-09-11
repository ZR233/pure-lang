// Runs after restarting a real GUI with an existing Studio home.
// Arguments: VM URL, Thread ID, prompt file, expected tool, output prefix.
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

Future<void> main(List<String> args) async {
  if (args.length != 5) {
    throw ArgumentError(
      'Expected VM URL, Thread ID, prompt file, expected tool, output prefix',
    );
  }
  final threadId = args[1];
  final prompt = await File(args[2]).readAsString();
  final expectedTool = args[3];
  final output = args[4];
  await File(output).parent.create(recursive: true);
  final session = await FlutterDriverSession.connect(vmServiceUrl: args[0]);
  try {
    await session.waitFor(
      find.byValueKey('studio-shell'),
      timeout: const Duration(minutes: 2),
    );
    var before = await session.readSnapshot();
    if ((before['workspace'] as Map?)?['threadId'] != threadId) {
      await session.tap(find.byValueKey('thread-row-$threadId'));
    }
    final readyDeadline = DateTime.now().add(const Duration(seconds: 30));
    while (true) {
      before = await session.readSnapshot();
      final workspace = before['workspace'] as Map?;
      if (workspace?['threadId'] == threadId && workspace?['lastTurn'] is Map) {
        break;
      }
      if (DateTime.now().isAfter(readyDeadline)) {
        throw StateError('Saved Thread did not load: $threadId');
      }
      await Future<void>.delayed(const Duration(milliseconds: 250));
    }
    await File('$output.before.json').writeAsString(jsonEncode(before));
    final previousTurn =
        ((before['workspace'] as Map)['lastTurn'] as Map)['id'];
    final previousCalls = _tools(before).map((tool) => tool['callId']).toSet();
    await session.tap(find.byValueKey('composer-input'));
    await session.enterText(prompt);
    await session.waitForNoPendingFrame(timeout: const Duration(seconds: 15));
    await session.tap(find.byValueKey('composer-submit'));
    final deadline = DateTime.now().add(const Duration(minutes: 2));
    while (DateTime.now().isBefore(deadline)) {
      final snapshot = await session.readSnapshot();
      await File('$output.jsonl')
          .writeAsString('${jsonEncode(snapshot)}\n', mode: FileMode.append);
      final workspace = snapshot['workspace'] as Map?;
      if (workspace?['threadId'] != threadId) {
        throw StateError('Recovery changed the selected Thread identity');
      }
      final turn = workspace?['lastTurn'] as Map?;
      if (turn != null && turn['id'] != previousTurn) {
        if (['failed', 'cancelled', 'budgetLimited'].contains(turn['status'])) {
          throw StateError('Recovered Turn did not succeed: $turn');
        }
        if (turn['status'] == 'completed' && workspace?['isBusy'] == false) {
          final executedTool = _tools(snapshot).any(
            (tool) =>
                !previousCalls.contains(tool['callId']) &&
                tool['name'] == expectedTool &&
                tool['status'] == 'succeeded',
          );
          if (!executedTool) {
            throw StateError(
              'New Turn did not execute $expectedTool successfully',
            );
          }
          await File('$output.png').writeAsBytes(await session.screenshot());
          await File('$output.tree.txt')
              .writeAsString(await session.renderTree());
          stdout.writeln(
            jsonEncode({
              'result': 'continued',
              'threadId': threadId,
              'previousTurn': previousTurn,
              'turn': turn,
              'expectedTool': expectedTool,
            }),
          );
          return;
        }
      }
      await Future<void>.delayed(const Duration(milliseconds: 250));
    }
    throw StateError('No new completed Turn in restored Thread $threadId');
  } catch (error) {
    try {
      await File('$output.failure.json')
          .writeAsString(jsonEncode(await session.readSnapshot()));
      await File('$output.failure.png')
          .writeAsBytes(await session.screenshot());
    } on Object catch (captureError) {
      stderr.writeln('Failure evidence unavailable: $captureError');
    }
    rethrow;
  } finally {
    try {
      final shutdown = await session.requestData(
        'shutdown-await',
        timeout: const Duration(minutes: 2),
      );
      if ((jsonDecode(shutdown) as Map)['shutdown'] != 'completed') {
        throw StateError('Studio shutdown did not complete: $shutdown');
      }
    } finally {
      await session.close();
    }
  }
}

Iterable<Map> _tools(Map<String, dynamic> snapshot) sync* {
  final timeline = (snapshot['workspace'] as Map?)?['timeline'] as List? ?? [];
  for (final row in timeline.whereType<Map>()) {
    yield* (row['tools'] as List? ?? []).whereType<Map>();
  }
}
