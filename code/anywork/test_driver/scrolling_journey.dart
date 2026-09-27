import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'pointer_scroll.dart';
import 'raw_tap.dart';
import 'scrollbar_drag.dart';

/// Opt-in native observation, shared by the long-body manual journey.
Future<Map<String, Object?>> observeTimelineScrolling(
  String vmUrl,
  Directory output,
) async {
  final driver = await FlutterDriver.connect(dartVmServiceUrl: vmUrl);
  final timeline = find.byValueKey('timeline-scrollable');
  final observations = <String, Object?>{};
  Future<Map<String, dynamic>> scroll() async {
    final snapshot = jsonDecode(await driver.requestData('snapshot')) as Map;
    return (snapshot['timelineScroll'] as Map).cast<String, dynamic>();
  }

  try {
    await driver.sendCommand(SetFrameSync(false));
    final wheel = <Map<String, dynamic>>[await scroll()];
    for (var step = 0; step < 8; step++) {
      await driver.sendCommand(PointerScroll(timeline, -60));
      await Future<void>.delayed(const Duration(milliseconds: 100));
      wheel.add(await scroll());
    }
    observations['wheel'] = wheel;
    observations['wheelViewportStable'] = wheel.every(
      (sample) =>
          sample['viewportDimension'] == wheel.first['viewportDimension'],
    );
    observations['forwardCoordinates'] = wheel.every(
      (sample) => sample['minScrollExtent'] == 0,
    );
    observations['wheelDetached'] = wheel.last['followingBottom'] == false;
    observations['wheelContinuous'] = List.generate(wheel.length - 1, (index) {
      final delta =
          (wheel[index + 1]['pixels'] as num) - (wheel[index]['pixels'] as num);
      return (delta + 60).abs() < 0.5;
    }).every((moved) => moved);
    await File('${output.path}/scroll-wheel.png')
        .writeAsBytes(await driver.screenshot());
    final up = await driver.sendCommand(ScrollbarDrag(timeline, -140));
    observations['thumbUp'] = up;
    await File('${output.path}/scroll-thumb-up.png')
        .writeAsBytes(await driver.screenshot());
    final down = await driver.sendCommand(ScrollbarDrag(timeline, 80));
    observations['thumbDown'] = down;
    bool monotonic(Map<String, dynamic> result, bool increasing) {
      final samples = result['samples'] as List;
      var changes = 0;
      for (var index = 1; index < samples.length; index++) {
        final delta =
            (samples[index]['pixels'] as num) -
            (samples[index - 1]['pixels'] as num);
        if (increasing ? delta < -0.5 : delta > 0.5) return false;
        if (delta.abs() > 0.5) changes++;
      }
      return changes >= 12;
    }

    observations['thumbUpContinuous'] = monotonic(up, false);
    observations['thumbDownContinuous'] = monotonic(down, true);
    observations['pass'] = [
      'wheelViewportStable',
      'forwardCoordinates',
      'wheelDetached',
      'wheelContinuous',
      'thumbUpContinuous',
      'thumbDownContinuous',
    ].every((key) => observations[key] == true);
    await driver.sendCommand(RawTap(find.byValueKey('timeline-jump-latest')));
    await Future<void>.delayed(const Duration(milliseconds: 200));
    observations['returnedToLatest'] = await scroll();
    return observations;
  } finally {
    observations['humanVerdict'] = 'pending';
    await File('${output.path}/scrolling.json').writeAsString(
      '${const JsonEncoder.withIndent('  ').convert(observations)}\n',
    );
    await driver.close();
  }
}
