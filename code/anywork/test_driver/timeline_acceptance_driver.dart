import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

/// Native-only, local scripted-provider acceptance. Run via the Python harness.
Future<void> main(List<String> args) async {
  final driver = await FlutterDriver.connect(dartVmServiceUrl: args[0]);
  final output = Directory(args[1]);
  final snapshots = File('${output.path}/snapshots.jsonl');
  final timings = <String, int>{};
  Future<Map<String, dynamic>> snapshot() async {
    final raw = await driver.requestData('snapshot');
    snapshots.writeAsStringSync('$raw\n', mode: FileMode.append);
    return jsonDecode(raw) as Map<String, dynamic>;
  }

  Future<Map<String, dynamic>> waitFor(
    bool Function(Map<String, dynamic>) accepts,
  ) async {
    final deadline = DateTime.now().add(const Duration(seconds: 30));
    while (DateTime.now().isBefore(deadline)) {
      final state = await snapshot();
      if (accepts(state)) return state;
      await Future<void>.delayed(const Duration(milliseconds: 50));
    }
    throw StateError('native timeline acceptance condition timed out');
  }

  List<dynamic> ids(Map<String, dynamic> state) =>
      (state['timelineWindow'] as Map)['itemIds'] as List;
  String text(Map<String, dynamic> state) =>
      ((state['workspace'] as Map?)?['timeline'] as List? ?? [])
          .map((row) => (row as Map)['text'] ?? '')
          .join('\n');
  void require(bool condition, String message) {
    if (!condition) throw StateError(message);
  }

  Future<void> select(String id) async {
    await driver.tap(find.byValueKey('agent-switcher'));
    await driver.tap(find.byValueKey('agent-thread-$id'));
    await waitFor(
      (state) =>
          (state['navigation'] as Map)['selectedThreadId'] == id &&
          (state['workspace'] as Map?)?['isLoading'] != true,
    );
  }

  try {
    await driver.runUnsynchronized(() async {
      await driver.waitFor(
        find.byValueKey('studio-shell'),
        timeout: const Duration(minutes: 2),
      );
      final watch = Stopwatch()..start();
      final seeded = jsonDecode(
        await driver.requestData(
          'timeline-native:${jsonEncode({'action': 'seed', 'path': args[3]})}',
          timeout: const Duration(minutes: 5),
        ),
      ) as Map;
      final rootId = seeded['rootId'] as String;
      var state = await waitFor(
        (state) =>
            ids(state).isNotEmpty &&
            (state['workspace'] as Map?)?['agents'] is List,
      );
      timings['seedAndFirstOpenMs'] = watch.elapsedMilliseconds;
      final agents = (state['workspace'] as Map)['agents'] as List;
      final childId =
          (agents.cast<Map>().firstWhere(
                (agent) => agent['threadId'] != rootId,
              ))['threadId']
              as String;
      final seen = <String>{...ids(state).cast<String>()};
      final benchmark = await driver.requestData(
        'timeline-native:${jsonEncode({'action': 'benchmark', 'threadId': rootId})}',
      );
      await File('${output.path}/page-query-benchmark.json')
          .writeAsString(benchmark);
      final expectedIds =
          ((jsonDecode(benchmark) as Map)['canonicalItemIds'] as List)
              .cast<String>()
              .toSet();
      final trace = await driver.traceAction(() async {
        for (var round = 0; round < 24; round++) {
          watch.reset();
          await driver.scroll(
            find.byValueKey('timeline-scrollable'),
            0,
            6500,
            const Duration(milliseconds: 200),
          );
          state = await waitFor(
            (state) => (state['timelineWindow'] as Map)['loading'] == false,
          );
          timings['olderPage-$round-ms'] = watch.elapsedMilliseconds;
          seen.addAll(ids(state).cast<String>());
          final window = state['timelineWindow'] as Map;
          require(
            ids(state).length <= 500 &&
                (window['cacheCount'] as int) <= 900 &&
                (window['tailCount'] as int) <= 400,
            'timeline cache exceeds budget',
          );
          if (window['hasOlder'] == false) break;
        }
        require(
          (state['timelineWindow'] as Map)['hasOlder'] == false,
          'older pagination did not reach the beginning',
        );
        require(
          text(state).contains('timeline-seed-0'),
          'oldest seed text disappeared',
        );
        await File('${output.path}/oldest.png')
            .writeAsBytes(await driver.screenshot());
        final anchor = (state['timelineWindow'] as Map)['anchor'];
        await select(childId);
        require(
          text(await snapshot()).contains('CHILD 独立正文'),
          'child text missing or wrong owner',
        );
        watch.reset();
        await select(rootId);
        timings['cachedSwitchMs'] = watch.elapsedMilliseconds;
        state = await snapshot();
        require(
          text(state).contains('timeline-seed-0'),
          'cached history was replaced on agent switch',
        );
        require(
          (state['timelineWindow'] as Map)['anchor'] == null ||
              anchor == null ||
              ((state['timelineWindow'] as Map)['anchor'] as Map)['itemId'] ==
                  (anchor as Map)['itemId'],
          'agent switch changed reading anchor',
        );
        for (var round = 0; round < 24; round++) {
          await driver.scroll(
            find.byValueKey('timeline-scrollable'),
            0,
            -6500,
            const Duration(milliseconds: 200),
          );
          state = await waitFor(
            (state) => (state['timelineWindow'] as Map)['loading'] == false,
          );
          seen.addAll(ids(state).cast<String>());
          if ((state['timelineWindow'] as Map)['hasNewer'] == false) break;
        }
        require(
          (state['timelineWindow'] as Map)['hasNewer'] == false,
          'newer pagination did not reach the tail',
        );
        require(
          text(state).contains('timeline-seed-159'),
          'latest seed missing after reverse pagination',
        );
      });
      require(
        seen.length == expectedIds.length && seen.containsAll(expectedIds),
        'bidirectional reading disagrees with complete canonical history',
      );
      await TimelineSummary.summarize(trace).writeTimelineToFile(
        'scroll',
        destinationDirectory: output.path,
        pretty: true,
      );
      state = await snapshot();
      if (((state['timelineWindow'] as Map)['anchor']
              as Map?)?['followingBottom'] !=
          true) {
        await driver.tap(find.byValueKey('timeline-jump-latest'));
      }
      final receipt = jsonDecode(
        await driver.requestData(
          'timeline-native:${jsonEncode({'action': 'start', 'threadId': rootId})}',
        ),
      ) as Map;
      await waitFor((state) => text(state).contains('第一段'));
      await select(childId);
      final client = HttpClient();
      try {
        final response = await (await client.getUrl(
          Uri.parse('${args[2]}/release'),
        )).close();
        await response.drain<void>();
      } finally {
        client.close();
      }
      await select(rootId);
      state = await waitFor((state) {
        final turn = (state['workspace'] as Map?)?['lastTurn'] as Map?;
        return text(state).contains('第二段') &&
            turn?['inputId'] == receipt['inputId'] &&
            turn?['status'] == 'completed';
      });
      require(
        text(state).contains('第一段\n第二段\n```text\nC:\\fixture\\n\n```\n'),
        'stream chunks were lost or rewritten across switch',
      );
      require(
        !text(state).contains('CHILD 独立正文'),
        'child text contaminated root',
      );
      await File('${output.path}/stream-completed.png')
          .writeAsBytes(await driver.screenshot());
      for (final ending in ['failed', 'cancelled']) {
        final prefix = ending == 'failed' ? '失败前片段' : '取消前片段';
        final receipt = jsonDecode(
          await driver.requestData(
            'timeline-native:${jsonEncode({'action': 'start', 'threadId': rootId, 'ending': ending})}',
          ),
        ) as Map;
        await waitFor((state) => text(state).contains(prefix));
        await select(childId);
        if (ending == 'cancelled') {
          await driver.requestData(
            'timeline-native:${jsonEncode({'action': 'interrupt', 'threadId': rootId, 'inputId': receipt['inputId']})}',
          );
        }
        final client = HttpClient();
        try {
          final response = await (await client.getUrl(
            Uri.parse('${args[2]}/release'),
          )).close();
          await response.drain<void>();
        } finally {
          client.close();
        }
        await select(rootId);
        state = await waitFor((state) {
          final turn = (state['workspace'] as Map?)?['lastTurn'] as Map?;
          return turn?['inputId'] == receipt['inputId'] &&
              turn?['status'] == ending;
        });
        require(
          text(state).contains(prefix),
          '$ending removed the observed preview',
        );
        require(
          text(state).contains('第一段\n第二段'),
          '$ending removed completed text',
        );
        await File('${output.path}/stream-$ending.png')
            .writeAsBytes(await driver.screenshot());
      }
      final shutdown = jsonDecode(
        await driver.requestData(
          'shutdown-await',
          timeout: const Duration(seconds: 30),
        ),
      ) as Map;
      require(shutdown['shutdown'] == 'completed', 'native shutdown failed');
      await File('${output.path}/result.json').writeAsString(
        jsonEncode({
          'result': 'passed',
          'nativeFrb': true,
          'scriptedProvider': true,
          'seenItemCount': seen.length,
          'terminalCases': ['completed', 'failed', 'cancelled'],
          'timings': timings,
        }),
      );
    });
  } catch (_) {
    await File('${output.path}/failure.png')
        .writeAsBytes(await driver.screenshot());
    rethrow;
  } finally {
    await driver.close();
  }
}
