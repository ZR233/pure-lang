import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'raw_tap.dart';

/// Opt-in native GUI acceptance for SQL history and the live overlay.
///
/// The Python harness launches this driver in a `full` phase and then a `reopen`
/// phase against the same isolated Studio home. It is intentionally manual
/// acceptance evidence, not an automatic quality gate.
Future<void> main(List<String> args) async {
  if (args.length != 5 && args.length != 6) {
    throw ArgumentError(
      'Expected VM URL, output, provider URL, workspace, phase '
      '[full|reopen], and root Thread ID for reopen',
    );
  }
  final phase = args[4];
  if (phase != 'full' && phase != 'reopen') {
    throw ArgumentError('Expected phase full or phase reopen');
  }
  if ((phase == 'full') != (args.length == 5) ||
      (phase == 'reopen') != (args.length == 6)) {
    throw ArgumentError('Expected phase full or phase reopen with its data');
  }

  final driver = await FlutterDriver.connect(dartVmServiceUrl: args[0]);
  final output = Directory(args[1]);
  final snapshots = File('${output.path}/snapshots.jsonl');
  final timings = <String, int>{};

  Future<Map<String, dynamic>> snapshot() async {
    final raw = await driver.requestData(
      'snapshot',
      timeout: const Duration(seconds: 30),
    );
    await snapshots.writeAsString('$raw\n', mode: FileMode.append);
    return jsonDecode(raw) as Map<String, dynamic>;
  }

  Future<Map<String, dynamic>> waitFor(
    bool Function(Map<String, dynamic>) accepts, {
    Duration timeout = const Duration(minutes: 2),
  }) async {
    final deadline = DateTime.now().add(timeout);
    Map<String, dynamic>? last;
    while (DateTime.now().isBefore(deadline)) {
      final state = await snapshot();
      last = state;
      if (accepts(state)) return state;
      await Future<void>.delayed(const Duration(milliseconds: 50));
    }
    throw StateError('native timeline acceptance condition timed out: $last');
  }

  Map<String, dynamic>? workspace(Map<String, dynamic> state) =>
      state['workspace'] as Map<String, dynamic>?;
  List<dynamic> ids(Map<String, dynamic> state) =>
      (state['timelineWindow'] as Map)['itemIds'] as List;
  String text(Map<String, dynamic> state) =>
      (workspace(state)?['timeline'] as List? ?? [])
          .map((row) => (row as Map)['text'] ?? '')
          .join('\n');
  Map<String, dynamic>? composer(Map<String, dynamic> state) =>
      workspace(state)?['composer'] as Map<String, dynamic>?;
  Map<String, dynamic>? lastTurn(Map<String, dynamic> state) =>
      workspace(state)?['lastTurn'] as Map<String, dynamic>?;
  bool followingBottom(Map<String, dynamic> state) {
    final anchor = (state['timelineWindow'] as Map)['anchor'] as Map?;
    return anchor?['followingBottom'] == true;
  }

  void require(bool condition, String message) {
    if (!condition) throw StateError(message);
  }

  Future<void> tap(SerializableFinder finder) async {
    await driver.waitFor(finder, timeout: const Duration(seconds: 40));
    await driver.sendCommand(
      RawTap(finder, timeout: const Duration(seconds: 30)),
    );
  }

  Future<void> tapKey(String key) => tap(find.byValueKey(key));

  Future<void> capture(String name) async {
    await snapshot();
    await File('${output.path}/$name.png')
        .writeAsBytes(await driver.screenshot());
    final tree = await driver.getRenderTree();
    await File('${output.path}/$name.tree.txt').writeAsString(tree.tree ?? '');
  }

  Future<void> select(String id) async {
    await tapKey('agent-switcher');
    await tapKey('agent-thread-$id');
    await waitFor(
      (state) =>
          (state['navigation'] as Map)['selectedThreadId'] == id &&
          workspace(state)?['threadId'] == id &&
          (state['timelineWindow'] as Map)['loading'] == false &&
          ids(state).isNotEmpty,
    );
  }

  Future<Map<String, dynamic>> ensureLatest() async {
    var state = await snapshot();
    if (!followingBottom(state)) {
      await tapKey('timeline-jump-latest');
    }
    return waitFor(
      (state) =>
          followingBottom(state) &&
          (state['timelineWindow'] as Map)['hasNewer'] == false &&
          (state['timelineWindow'] as Map)['loading'] == false,
    );
  }

  Future<(int, double)?> visibleHistoricalSeed() async {
    for (var seed = 70; seed <= 95; seed++) {
      try {
        final position = await driver.getTopLeft(
          find.text('timeline-seed-$seed'),
          timeout: const Duration(milliseconds: 400),
        );
        if (position.dy >= 80 && position.dy < 510) {
          return (seed, position.dy);
        }
      } catch (_) {
        // Most rows in a paged history window are not mounted in the viewport.
      }
    }
    return null;
  }

  Future<void> providerControl(String action) async {
    final client = HttpClient();
    try {
      final response = await (await client.getUrl(
        Uri.parse('${args[2]}/$action'),
      )).close();
      await response.drain<void>();
    } finally {
      client.close();
    }
  }

  Future<void> releaseProvider() => providerControl('release');

  Future<String> fixtureStart(String threadId, [String? ending]) async {
    await providerControl('prepare');
    final request = <String, String>{'action': 'start', 'threadId': threadId};
    if (ending != null) request['ending'] = ending;
    final receipt = jsonDecode(
      await driver.requestData('timeline-native:${jsonEncode(request)}'),
    ) as Map;
    return receipt['inputId'] as String;
  }

  Future<void> submitThroughGui(String prompt) async {
    await tapKey('composer-input');
    await driver.enterText(prompt);
    final drafted = await waitFor(
      (state) => composer(state)?['draft'] == prompt,
    );
    require(
      composer(drafted)?['draft'] == prompt,
      'GUI composer did not expose the running-turn follow-up draft',
    );
    require(
      workspace(drafted)?['isBusy'] == true,
      'GUI follow-up was not drafted while a Turn was running',
    );
    await capture('gui-follow-up-draft-while-running');
    await tapKey('composer-submit');
    await waitFor(
      (state) =>
          composer(state)?['draft'] == '' &&
          composer(state)?['submissionPending'] == false,
    );
    await capture('gui-follow-up-submitted-while-running');
  }

  try {
    await driver.runUnsynchronized(() async {
      await driver.waitFor(
        find.byValueKey('studio-shell'),
        timeout: const Duration(minutes: 2),
      );

      if (phase == 'reopen') {
        final metadata = jsonDecode(
          await File('${output.path}/timeline-thread.json').readAsString(),
        ) as Map;
        final rootId = args[5];
        require(
          metadata['rootId'] == rootId,
          'reopen root Thread ID disagrees with full-phase metadata',
        );
        var state = await waitFor(
          (state) =>
              ((state['sidebarDirectory'] as Map)['ids'] as List).contains(
                rootId,
              ) &&
              (state['navigation'] as Map)['selectedThreadId'] == rootId,
        );
        await capture('reopen-startup');
        if (workspace(state)?['threadId'] != rootId) {
          await tapKey('studio-open-thread-$rootId');
        }
        state = await waitFor(
          (state) =>
              workspace(state)?['threadId'] == rootId &&
              ids(state).isNotEmpty &&
              (state['timelineWindow'] as Map)['loading'] == false &&
              lastTurn(state) != null,
        );
        require(
          lastTurn(state)?['inputId'] == metadata['cancelledInputId'] &&
              lastTurn(state)?['status'] == 'cancelled',
          'reopened last Turn is not the GUI-cancelled Turn',
        );
        require(
          text(state).contains('第一段\n第二段') &&
              text(state).contains('GUI_FOLLOW_UP_COMPLETE') &&
              text(state).contains('失败前片段') &&
              text(state).contains('取消前片段'),
          'reopened SQL history lost stream, GUI follow-up, failure, or '
          'cancel evidence',
        );
        await capture('reopen-latest-history');

        for (var round = 0; round < 24; round++) {
          await driver.scroll(
            find.byValueKey('timeline-scrollable'),
            0,
            6500,
            const Duration(milliseconds: 200),
          );
          state = await waitFor(
            (state) =>
                (state['timelineWindow'] as Map)['loading'] == false &&
                followingBottom(state) != true,
          );
          require(
            ids(state).length <= 500 &&
                ((state['timelineWindow'] as Map)['historyCount'] as int) <=
                    500,
            'reopened timeline window or history exceeds budget',
          );
          if ((state['timelineWindow'] as Map)['hasOlder'] == false) break;
        }
        require(
          (state['timelineWindow'] as Map)['hasOlder'] == false &&
              text(state).contains('timeline-seed-0'),
          'reopened older pagination did not reach durable seed 0',
        );
        await capture('reopen-oldest-history');
        await tapKey('timeline-jump-latest');
        state = await waitFor(
          (state) =>
              followingBottom(state) &&
              (state['timelineWindow'] as Map)['hasNewer'] == false &&
              (state['timelineWindow'] as Map)['loading'] == false,
        );
        require(
          text(state).contains('GUI_FOLLOW_UP_COMPLETE') &&
              text(state).contains('取消前片段'),
          'jump-to-latest did not restore reopened terminal history',
        );
        await capture('reopen-jump-latest');

        final shutdown = jsonDecode(
          await driver.requestData(
            'shutdown-await',
            timeout: const Duration(seconds: 30),
          ),
        ) as Map;
        require(shutdown['shutdown'] == 'completed', 'native shutdown failed');
        await capture('reopen-shutdown-completed');
        await File('${output.path}/reopen-result.json').writeAsString(
          jsonEncode({
            'result': 'passed',
            'sameStudioHome': true,
            'rootThreadId': rootId,
            'lastTerminalStatus': 'cancelled',
            'guiFollowUpInputId': metadata['guiInputId'],
            'timings': timings,
          }),
        );
        return;
      }

      final watch = Stopwatch()..start();
      final seeded = jsonDecode(
        await driver.requestData(
          'timeline-native:${jsonEncode({'action': 'seed', 'path': args[3]})}',
          timeout: const Duration(minutes: 5),
        ),
      ) as Map;
      final rootId = seeded['rootId'] as String;
      var state = await waitFor(
        (state) => ids(state).isNotEmpty && workspace(state)?['agents'] is List,
      );
      timings['seedAndFirstOpenMs'] = watch.elapsedMilliseconds;
      final agents = (workspace(state)!['agents'] as List).cast<Map>();
      final children = agents.where((agent) => agent['threadId'] != rootId);
      require(
        children.isNotEmpty,
        'native fixture did not create a child Thread',
      );
      final childId = children.first['threadId'] as String;
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
            ids(state).length <= 500 && (window['historyCount'] as int) <= 500,
            'timeline window or history exceeds budget',
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
        for (var round = 0; round < 16; round++) {
          if ((state['timelineWindow'] as Map)['anchor']?['itemId'] ==
              ids(state).first) {
            break;
          }
          await driver.scroll(
            find.byValueKey('timeline-scrollable'),
            0,
            6500,
            const Duration(milliseconds: 200),
          );
          state = await snapshot();
        }
        require(
          (state['timelineWindow'] as Map)['anchor']?['itemId'] ==
              ids(state).first,
          'oldest loaded item was not actually brought into view',
        );
        await capture('oldest');
        final anchor = (state['timelineWindow'] as Map)['anchor'];
        await select(childId);
        require(
          text(await snapshot()).contains('CHILD 独立正文'),
          'child text missing or wrong owner',
        );
        await capture('child-history');
        watch.reset();
        await select(rootId);
        timings['cachedSwitchMs'] = watch.elapsedMilliseconds;
        state = await snapshot();
        require(
          anchor == null || ids(state).contains((anchor as Map)['itemId']),
          'restored reading anchor is not in the SQL window',
        );
        require(
          (state['timelineWindow'] as Map)['anchor'] == null ||
              anchor == null ||
              ((state['timelineWindow'] as Map)['anchor'] as Map)['itemId'] ==
                  (anchor as Map)['itemId'],
          'agent switch changed reading anchor',
        );
        for (var round = 0; round < 24; round++) {
          if ((state['timelineWindow'] as Map)['hasOlder'] == false) break;
          await driver.scroll(
            find.byValueKey('timeline-scrollable'),
            0,
            6500,
            const Duration(milliseconds: 200),
          );
          state = await waitFor(
            (state) => (state['timelineWindow'] as Map)['loading'] == false,
          );
        }
        require(
          (state['timelineWindow'] as Map)['hasOlder'] == false &&
              text(state).contains('timeline-seed-0'),
          'restored reading position cannot page back to the first message',
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
          text(state).contains('timeline-seed-95'),
          'latest seed missing after reverse pagination',
        );
        await capture('latest-seeds');
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
      await ensureLatest();

      final completedInputId = await fixtureStart(rootId);
      await waitFor((state) => text(state).contains('第一段'));
      await capture('live-completed-first-chunk');
      await select(childId);
      require(
        !text(await snapshot()).contains('第一段'),
        'live root overlay contaminated the child workspace',
      );
      await capture('live-completed-child-isolation');
      await releaseProvider();
      await select(rootId);
      state = await waitFor((state) {
        final turn = lastTurn(state);
        return text(state).contains('第二段') &&
            turn?['inputId'] == completedInputId &&
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
      await capture('stream-completed');

      final runningInputId = await fixtureStart(rootId);
      await waitFor((state) => text(state).contains('第一段'));
      await capture('live-overlay-before-history-review');
      await driver.scroll(
        find.byValueKey('timeline-scrollable'),
        0,
        6500,
        const Duration(milliseconds: 200),
      );
      state = await waitFor(
        (state) =>
            (state['timelineWindow'] as Map)['loading'] == false &&
            followingBottom(state) != true &&
            ((state['timelineWindow'] as Map)['overlayCount'] as int) > 0 &&
            RegExp(r'timeline-seed-\d+').hasMatch(text(state)),
      );
      await capture('history-review-with-live-overlay');
      watch.reset();
      await submitThroughGui('timeline-gui-follow-up');
      timings['guiSubmissionMs'] = watch.elapsedMilliseconds;
      await tapKey('timeline-jump-latest');
      state = await waitFor(
        (state) =>
            followingBottom(state) &&
            (state['timelineWindow'] as Map)['hasNewer'] == false &&
            (state['timelineWindow'] as Map)['loading'] == false,
      );
      await capture('jump-latest-while-follow-up-pending');
      await releaseProvider();
      state = await waitFor((state) {
        final turn = lastTurn(state);
        return turn?['inputId'] != runningInputId &&
            turn?['status'] == 'completed' &&
            text(state).contains('GUI_FOLLOW_UP_COMPLETE');
      });
      final guiInputId = lastTurn(state)!['inputId'] as String;
      require(
        text(state).contains('第一段\n第二段') &&
            text(state).contains('GUI_FOLLOW_UP_COMPLETE'),
        'GUI follow-up did not execute after the controlled stream',
      );
      await capture('gui-follow-up-completed');

      final detachedInputId = await fixtureStart(rootId);
      await waitFor(
        (state) =>
            lastTurn(state)?['inputId'] == detachedInputId &&
            ((state['timelineWindow'] as Map)['overlayCount'] as int) > 0,
      );
      (int, double)? visibleBefore;
      for (var attempt = 0; attempt < 4 && visibleBefore == null; attempt++) {
        await driver.scroll(
          find.byValueKey('timeline-scrollable'),
          0,
          3200,
          const Duration(milliseconds: 300),
        );
        state = await waitFor(
          (state) =>
              !followingBottom(state) &&
              ((state['timelineWindow'] as Map)['overlayCount'] as int) > 0 &&
              RegExp(r'timeline-seed-\d+').hasMatch(text(state)),
        );
        visibleBefore = await visibleHistoricalSeed();
      }
      await capture('detached-before-terminal');
      require(
        visibleBefore != null,
        'detached reading position is not showing historical messages',
      );
      final detachedAnchor = (state['timelineWindow'] as Map)['anchor'];
      await releaseProvider();
      state = await waitFor(
        (state) =>
            lastTurn(state)?['inputId'] == detachedInputId &&
            lastTurn(state)?['status'] == 'completed' &&
            ((state['timelineWindow'] as Map)['overlayCount'] as int) == 0 &&
            !followingBottom(state),
      );
      require(
        (state['timelineWindow'] as Map)['anchor']?['itemId'] ==
            (detachedAnchor as Map)['itemId'],
        'SQL terminal confirmation moved the historical reading anchor',
      );
      await capture('detached-terminal-confirmed');
      final visibleAfter = await driver.getTopLeft(
        find.text('timeline-seed-${visibleBefore!.$1}'),
        timeout: const Duration(seconds: 2),
      );
      require(
        visibleAfter.dy >= 80 &&
            visibleAfter.dy < 510 &&
            (visibleAfter.dy - visibleBefore.$2).abs() < 130,
        'terminal confirmation displaced visible historical messages',
      );
      await ensureLatest();

      String? cancelledInputId;
      for (final ending in ['failed', 'cancelled']) {
        final prefix = ending == 'failed' ? '失败前片段' : '取消前片段';
        final inputId = await fixtureStart(rootId, ending);
        await waitFor((state) => text(state).contains(prefix));
        await capture('$ending-live-prefix');
        await select(childId);
        require(
          !text(await snapshot()).contains(prefix),
          '$ending live overlay leaked into child history',
        );
        await select(rootId);
        if (ending == 'failed') {
          await releaseProvider();
        } else {
          await waitFor((state) => text(state).contains(prefix));
          await capture('cancelled-before-gui-stop');
          await tapKey('composer-stop');
          state = await waitFor((state) {
            final turn = lastTurn(state);
            return turn?['inputId'] == inputId &&
                turn?['status'] == 'cancelled' &&
                text(state).contains(prefix);
          });
          await capture('cancelled-after-gui-stop');
          await releaseProvider();
          cancelledInputId = inputId;
        }
        state = await waitFor((state) {
          final turn = lastTurn(state);
          return turn?['inputId'] == inputId && turn?['status'] == ending;
        });
        require(
          text(state).contains(prefix),
          '$ending removed the observed preview',
        );
        require(
          text(state).contains('第一段\n第二段') &&
              text(state).contains('GUI_FOLLOW_UP_COMPLETE'),
          '$ending removed completed text or the GUI-submitted follow-up',
        );
        await capture('stream-$ending');
      }
      require(cancelledInputId != null, 'GUI cancel receipt was not recorded');

      await File('${output.path}/timeline-thread.json').writeAsString(
        jsonEncode({
          'rootId': rootId,
          'childId': childId,
          'completedInputId': completedInputId,
          'runningInputId': runningInputId,
          'guiInputId': guiInputId,
          'cancelledInputId': cancelledInputId,
        }),
      );
      final shutdown = jsonDecode(
        await driver.requestData(
          'shutdown-await',
          timeout: const Duration(seconds: 30),
        ),
      ) as Map;
      require(
        shutdown['shutdown'] == 'completed',
        'native shutdown failed: $shutdown',
      );
      await capture('shutdown-completed');
      await File('${output.path}/result.json').writeAsString(
        jsonEncode({
          'result': 'passed',
          'phase': 'full',
          'nativeFrb': true,
          'scriptedProvider': true,
          'sameStudioHomeReopenPlanned': true,
          'seenItemCount': seen.length,
          'terminalCases': [
            'completed',
            'gui-follow-up',
            'failed',
            'cancelled',
          ],
          'guiSubmissionWhileRunning': true,
          'guiCancel': true,
          'childSwitchValidated': true,
          'timings': timings,
        }),
      );
    });
  } catch (error, stackTrace) {
    try {
      await capture('failure-$phase');
      await File('${output.path}/failure-$phase.json')
          .writeAsString(jsonEncode(await snapshot()));
    } on Object catch (captureError) {
      stderr.writeln('Failure evidence unavailable: $captureError');
    }
    stderr.writeln('Timeline acceptance failed: $error\n$stackTrace');
    rethrow;
  } finally {
    await driver.close();
  }
}
