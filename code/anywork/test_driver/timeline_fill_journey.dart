import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'pointer_scroll.dart';
import 'raw_tap.dart';

/// Native observation of a compact raw page after the tool-scroll journey's long history.
Future<Map<String, Object?>> observeLatestViewportFill(
  FlutterDriver driver,
  Directory output,
) async {
  final timeline = find.byValueKey('timeline-scrollable');
  Future<Map<String, dynamic>> snapshot() async =>
      (jsonDecode(await driver.requestData('snapshot')) as Map)
          .cast<String, dynamic>();

  Future<Map<String, dynamic>> until(
    String stage,
    bool Function(Map<String, dynamic>) ready,
  ) async {
    final deadline = DateTime.now().add(const Duration(seconds: 60));
    while (true) {
      final state = await snapshot();
      if (ready(state)) return state;
      if (DateTime.now().isAfter(deadline)) {
        await File('${output.path}/latest-fill-$stage.json')
            .writeAsString(jsonEncode(state));
        await File('${output.path}/latest-fill-$stage.png')
            .writeAsBytes(await driver.screenshot());
        throw StateError('Latest viewport did not settle: $stage');
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
  }

  await driver.sendCommand(RawTap(find.byValueKey('timeline-jump-latest')));
  await driver.tap(find.byValueKey('composer-input'));
  await driver.enterText('Tool scroll compact');
  await driver.tap(find.byValueKey('composer-submit'));
  await until('compact-answer', (state) {
    final workspace = state['workspace'] as Map?;
    return workspace?['isBusy'] == false &&
        ((workspace?['timeline'] as List?) ?? []).any(
          (row) => row['text'] == 'compact tail complete',
        );
  });

  final observations = <String, Object?>{};
  // Each return resets to the small canonical latest page. Fill must repeat without an up gesture.
  for (var attempt = 0; attempt < 2; attempt++) {
    await driver.sendCommand(PointerScroll(timeline, -250));
    await until(
      'detached-$attempt',
      (s) => (s['timelineScroll'] as Map?)?['detachedByUser'] == true,
    );
    await driver.sendCommand(RawTap(find.byValueKey('timeline-jump-latest')));
    final filled = await until('filled-$attempt', (state) {
      final scroll = state['timelineScroll'] as Map?;
      final window = state['timelineWindow'] as Map?;
      return scroll?['followingBottom'] == true &&
          scroll?['detachedByUser'] == false &&
          window?['hasNewer'] == false &&
          ((window?['windowItemCount'] as num?) ?? 0) > 32 &&
          ((scroll?['maxScrollExtent'] as num?) ?? 0) > 0 &&
          ((scroll?['bottomSlack'] as num?) ?? 1) < 0.5 &&
          ((scroll?['extentAfter'] as num?) ?? 1) < 0.5;
    });
    observations['return-$attempt'] = {
      'window': filled['timelineWindow'],
      'scroll': filled['timelineScroll'],
    };
    await File('${output.path}/latest-fill-$attempt.png')
        .writeAsBytes(await driver.screenshot());
  }
  return observations;
}
