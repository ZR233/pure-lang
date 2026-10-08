// Opt-in companion to manual-gui --scenario stress, after its initial history
// observation settles. Exercises real wheel input after history and layout changes.
// On X11, run this client with the GUI's DISPLAY and XAUTHORITY as well.
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'pointer_scroll.dart';
import 'raw_tap.dart';
import 'window_control.dart';

Future<void> main(List<String> args) async {
  if (args.length != 2) {
    throw ArgumentError('expected <vm-url> <evidence-directory>');
  }
  final driver = await FlutterDriver.connect(dartVmServiceUrl: args[0]);
  final output = Directory(args[1])..createSync(recursive: true);
  final timeline = find.byValueKey('timeline-scrollable');
  final observations = <String, Object?>{};
  final pid =
      (jsonDecode(await driver.requestData('pid')) as Map)['pid'] as int;

  Future<Map<String, dynamic>> snapshot() async =>
      (jsonDecode(await driver.requestData('snapshot')) as Map)
          .cast<String, dynamic>();
  Future<Map<String, dynamic>> settle() async {
    Map<String, dynamic>? previous;
    final deadline = DateTime.now().add(const Duration(seconds: 15));
    while (DateTime.now().isBefore(deadline)) {
      final state = await snapshot();
      if (state['timelineWindow']['loading'] == false &&
          state['timelineScroll']['restorePending'] == false &&
          previous != null &&
          jsonEncode(state['timelineWindow']['itemIds']) ==
              jsonEncode(previous['timelineWindow']['itemIds']) &&
          jsonEncode(state['timelineScroll']) ==
              jsonEncode(previous['timelineScroll'])) {
        return state;
      }
      previous = state;
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    throw StateError('History window did not settle');
  }

  Map<String, Object?> summary(Map state) => {
    'scroll': state['timelineScroll'],
    'window': state['timelineWindow'],
  };

  try {
    await driver.sendCommand(SetFrameSync(false));
    for (final height in [2400, 5000]) {
      final evidence = <String, Object?>{};
      observations['height-$height'] = evidence;
      if ((await snapshot())['timelineScroll']['showJumpToLatest'] == true) {
        await driver.sendCommand(
          RawTap(find.byValueKey('timeline-jump-latest')),
        );
      }
      final initial = await settle();
      final latestItem = (initial['timelineWindow']['itemIds'] as List).last;
      evidence['initial'] = summary(initial);
      final older = <Object?>[];
      for (var step = 0; step < 6; step++) {
        await driver.sendCommand(PointerScroll(timeline, -1000000));
        older.add(summary(await settle()));
      }
      evidence['older'] = older;
      if ((await snapshot())['timelineWindow']['hasNewer'] != true) {
        throw StateError('Fixture did not leave the latest history window');
      }
      final newer = <Object?>[];
      for (var step = 0; step < 24; step++) {
        await driver.sendCommand(PointerScroll(timeline, 1000000));
        final state = await settle();
        newer.add(summary(state));
        if (state['timelineWindow']['hasNewer'] == false) break;
      }
      evidence['newer'] = newer;
      await driver.waitUntilNoTransientCallbacks();
      await driver.screenshot();
      final before = await settle();
      if (before['timelineWindow']['hasNewer'] != false ||
          before['timelineScroll']['followingBottom'] != false) {
        throw StateError('Expected manual reading in the final history page');
      }
      final resized = resizeOwnedWindow(
        guiPid: pid,
        width: 1280,
        height: height,
      );
      if (!resized.resized) {
        throw StateError('${resized.reason}: ${resized.diagnostics}');
      }
      final original = resized.original!;
      try {
        await driver.waitUntilNoTransientCallbacks();
        await driver.screenshot();
        final endpoint = await settle();
        evidence['endpoint'] = summary(endpoint);
        if (endpoint['timelineScroll']['extentAfter'] != 0.0 ||
            endpoint['timelineScroll']['followingBottom'] != false) {
          throw StateError('Resize did not produce a detached endpoint');
        }
        // The same gesture must work both with and without any scroll extent.
        await driver.sendCommand(PointerScroll(timeline, 60));
        final returned = await settle();
        evidence['returned'] = summary(returned);
        final scroll = returned['timelineScroll'] as Map;
        final window = returned['timelineWindow'] as Map;
        evidence['pass'] =
            scroll['followingBottom'] == true &&
            scroll['detachedByUser'] == false &&
            (scroll['extentAfter'] as num).abs() < 0.5 &&
            window['hasNewer'] == false &&
            (window['itemIds'] as List).last == latestItem;
        await File('${output.path}/edge-$height.png')
            .writeAsBytes(await driver.screenshot());
      } finally {
        final restored = resizeOwnedWindow(
          guiPid: pid,
          width: original.width,
          height: original.height,
        );
        if (!restored.resized) throw StateError('Window restoration failed');
        await settle();
      }
    }
    // Repeated small wheel steps must traverse paged history without idle
    // streaming flushes restoring a stale, pre-layout reading position.
    if ((await snapshot())['timelineScroll']['showJumpToLatest'] == true) {
      await driver.sendCommand(RawTap(find.byValueKey('timeline-jump-latest')));
    }
    final initial = await settle();
    final latestItem = (initial['timelineWindow']['itemIds'] as List).last;
    final rapid = <String, Object?>{'initial': summary(initial)};
    observations['rapid-history'] = rapid;
    final olderSteps = <Object?>[];
    for (var step = 0; step < 100; step++) {
      await driver.sendCommand(PointerScroll(timeline, -60));
      olderSteps.add(summary(await snapshot()));
    }
    rapid['older'] = olderSteps;
    if ((await snapshot())['timelineWindow']['hasNewer'] != true) {
      throw StateError('Small wheel steps did not leave latest history');
    }
    final newerSteps = <Object?>[];
    for (var step = 0; step < 200; step++) {
      await driver.sendCommand(PointerScroll(timeline, 60));
      final state = await snapshot();
      newerSteps.add(summary(state));
      if (state['timelineScroll']['followingBottom'] == true &&
          state['timelineWindow']['hasNewer'] == false) {
        break;
      }
    }
    rapid['newer'] = newerSteps;
    final returned = await settle();
    rapid['returned'] = summary(returned);
    rapid['pass'] =
        returned['timelineScroll']['followingBottom'] == true &&
        returned['timelineScroll']['detachedByUser'] == false &&
        (returned['timelineScroll']['extentAfter'] as num).abs() < 0.5 &&
        returned['timelineWindow']['hasNewer'] == false &&
        (returned['timelineWindow']['itemIds'] as List).last == latestItem;
    await File('${output.path}/rapid-history.png')
        .writeAsBytes(await driver.screenshot());
    if (rapid['pass'] != true) {
      throw StateError('Repeated wheel input could not return to latest');
    }
    // Regression: returning to Latest must clear the persisted history intent.
    // The first subsequent wheel step must enter browseHistory from the tail,
    // rather than reusing the stale anchor and reopening the oldest window.
    await driver.sendCommand(RawTap(find.byValueKey('timeline-jump-latest')));
    final afterJump = await settle();
    await driver.sendCommand(PointerScroll(timeline, -60));
    final afterOneOlderWheel = await settle();
    final jumpThenBrowse = <String, Object?>{
      'afterJump': summary(afterJump),
      'afterOneOlderWheel': summary(afterOneOlderWheel),
      'pass':
          afterJump['timelineScroll']['followingBottom'] == true &&
          afterJump['timelineScroll']['detachedByUser'] == false &&
          afterOneOlderWheel['timelineScroll']['followingBottom'] == false &&
          afterOneOlderWheel['timelineScroll']['readingIntent'] ==
              'browseHistory' &&
          (afterOneOlderWheel['timelineScroll']['pixels'] as num) <
              (afterJump['timelineScroll']['pixels'] as num) &&
          (afterOneOlderWheel['timelineWindow']['itemIds'] as List).last ==
              latestItem,
    };
    observations['jump-then-browse'] = jumpThenBrowse;
    if (jumpThenBrowse['pass'] != true) {
      throw StateError(
        'A wheel step after returning to Latest reused a stale history anchor',
      );
    }
    if (observations.values.any((value) => (value as Map)['pass'] != true)) {
      throw StateError('Wheel at the history endpoint did not restore Latest');
    }
  } catch (error) {
    observations['error'] = '$error';
    rethrow;
  } finally {
    await File('${output.path}/timeline-edge.json').writeAsString(
      '${const JsonEncoder.withIndent('  ').convert(observations)}\n',
    );
    await driver.close();
  }
}
