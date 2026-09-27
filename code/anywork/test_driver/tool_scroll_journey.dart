import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'raw_tap.dart';
import 'tool_probe.dart';
import 'stress_start.dart' as start;

Future<void> main(List<String> args) async {
  if (args.length == 3 && args[2] != '--observe-only') {
    final fixtures = Directory('${args[2]}/scroll-fixture');
    await fixtures.create(recursive: true);
    for (var i = 0; i < 240; i++) {
      await File('${fixtures.path}/entry-${i.toString().padLeft(3, '0')}.txt')
          .writeAsString('Tool scrolling fixture\n');
    }
    final git = await Process.run('git', ['init', '-q', args[2]]);
    if (git.exitCode != 0) throw StateError('Fixture project: ${git.stderr}');
    await start.main([
      args[0],
      args[2],
      'Tool scroll short',
      '${args[1]}/start-stage',
    ]);
  }
  final driver = await FlutterDriver.connect(dartVmServiceUrl: args[0]);
  final output = Directory(args[1]);
  await output.create(recursive: true);
  final evidence = <String, Object?>{};
  final timeline = find.byValueKey('timeline-scrollable');
  Future<Map<String, dynamic>> snapshot() async =>
      (jsonDecode(await driver.requestData('snapshot')) as Map)
          .cast<String, dynamic>();
  Future<Map<String, dynamic>> waitAnswer(String marker) async {
    final end = DateTime.now().add(const Duration(seconds: 60));
    while (DateTime.now().isBefore(end)) {
      final s = await snapshot();
      final w = s['workspace'] as Map?;
      final rows = w?['timeline'] as List? ?? [];
      if (w?['isBusy'] == false &&
          rows.any(
            (r) => (r['text'] as String? ?? '').contains(
              '$marker following paragraph 44.',
            ),
          )) {
        return s;
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    throw StateError('Tool fixture did not complete: $marker');
  }

  Future<Map<String, dynamic>> probe(
    SerializableFinder f,
    String action, {
    double dy = 0,
  }) => driver.sendCommand(ToolProbe(f, action, dy: dy));
  Future<void> place(SerializableFinder target, double y) async {
    final view = (await probe(timeline, 'observe'))['samples'].last as Map;
    final viewTop = view['top'] as num;
    final viewHeight = view['height'] as num;
    for (var i = 0; i < 80; i++) {
      final geometry = await probe(target, 'observe');
      final top = ((geometry['samples'] as List).last['top'] as num).toDouble();
      if ((top - y).abs() < 1) return;
      // A fully clipped sliver has a paint extent of zero; its transform is
      // not a distance-to-item estimate. Traverse by viewport steps until the
      // header is visible, then use its actual on-screen position.
      final delta = top < viewTop
          ? -viewHeight * 0.8
          : top > viewTop + viewHeight - 25
          ? viewHeight * 0.8
          : top - y;
      await probe(timeline, 'wheelOutside', dy: delta.toDouble());
    }
    throw StateError('Unable to place tool at $y');
  }

  try {
    await driver.sendCommand(SetFrameSync(false));
    await waitAnswer('Tool scroll short');
    for (final kind
        in args.contains('--observe-only')
            ? <String>[]
            : ['medium', 'long', 'arguments', 'multiple', 'failed']) {
      await driver.sendCommand(RawTap(find.byValueKey('composer-input')));
      await driver.enterText('Tool scroll $kind');
      await driver.sendCommand(RawTap(find.byValueKey('composer-submit')));
      await waitAnswer('Tool scroll $kind');
    }
    var s = await snapshot();
    if ((s['timelineWindow'] as Map)['hasOlder'] == true) {
      await driver.requestData('load-older');
      final deadline = DateTime.now().add(const Duration(seconds: 10));
      do {
        await Future<void>.delayed(const Duration(milliseconds: 100));
        s = await snapshot();
      } while ((s['timelineWindow'] as Map)['hasOlder'] == true &&
          DateTime.now().isBefore(deadline));
    }
    await File('${output.path}/fixture-snapshot.json')
        .writeAsString(jsonEncode(s));
    final groups = ((s['workspace'] as Map)['timeline'] as List)
        .where((r) => r['type'] == 'toolGroup')
        .toList();
    if (groups.length != 6) {
      throw StateError('Expected 6 real tool groups, got ${groups.length}');
    }
    final cases = <Object?>[];
    evidence['cases'] = cases;
    for (var index = 0; index < groups.length; index++) {
      stdout.writeln('tool_scroll_case=$index');
      var row = groups[index] as Map;
      final summary = find.byValueKey(
        'timeline-tool-group-summary-${row['id']}',
      );
      await place(summary, 120);
      for (final item in row['tools'] as List) {
        if (((await snapshot())['timelineWindow']['previewedItemIds'] as List)
            .contains(item['itemId'])) {
          await driver.sendCommand(
            RawTap(
              find.byValueKey('timeline-item-body-load-${item['itemId']}'),
            ),
          );
          final deadline = DateTime.now().add(const Duration(seconds: 10));
          while (((await snapshot())['timelineWindow']['previewedItemIds']
                  as List)
              .contains(item['itemId'])) {
            if (DateTime.now().isAfter(deadline)) {
              throw StateError('Tool body did not load');
            }
            await Future<void>.delayed(const Duration(milliseconds: 100));
          }
        }
      }
      row =
          (((await snapshot())['workspace'] as Map)['timeline'] as List)
                  .firstWhere((r) => r['id'] == row['id'])
              as Map;
      await place(summary, 120);
      final opened = await probe(summary, 'tap');
      final record = <String, Object?>{'index': index, 'groupOpen': opened};
      cases.add(record);
      final items = row['tools'] as List;
      if (items.any(
        (item) => item['status'] != (index == 5 ? 'failed' : 'succeeded'),
      )) {
        throw StateError('Unexpected tool outcome in case $index');
      }
      if (index == 2 &&
          jsonDecode(items.single['result'] as String)['count'] != 200) {
        throw StateError('Long result was not delivered whole');
      }
      if (index == 3 && (items.single['arguments'] as String).length < 12000) {
        throw StateError('Long arguments were truncated');
      }
      final tiles = <Object?>[];
      record['tiles'] = tiles;
      for (var j = 0; j < items.length; j++) {
        final item = items[j] as Map;
        final tile = find.byValueKey('timeline-tool-details:${item['itemId']}');
        await place(tile, 125);
        final expanded = await probe(tile, 'tap');
        await File('${output.path}/case-$index-tool-$j-expanded.png')
            .writeAsBytes(await driver.screenshot());
        final nested = (expanded['samples'] as List).last['nested'] as List;
        Map<String, dynamic>? inside;
        if (nested.any(
          (s) => s['axis'] == 'vertical' && (s['max'] as num) > 0,
        )) {
          // Move the output into the viewport; the tool title may now be above it.
          await place(tile, 80);
          inside = await probe(tile, 'wheelInner', dy: 60);
          await place(tile, 125);
        }
        final collapsed = await probe(tile, 'tap');
        tiles.add({
          'item': item['itemId'],
          'argumentLength': (item['arguments'] as String? ?? '').length,
          'outputLength': (item['result'] as String? ?? '').length,
          'expanded': expanded,
          'collapsed': collapsed,
          'inside': inside,
        });
      }
      await place(summary, 95);
      record['groupClose'] = await probe(summary, 'tap');
      await File('${output.path}/case-$index-collapsed.png')
          .writeAsBytes(await driver.screenshot());
    }
    // Leave one group open, establish a later center by opening another group,
    // then read back into the earlier group and expand its nested tool.
    stdout.writeln('tool_scroll_reverse');
    final earlier = groups[1] as Map;
    final later = groups[3] as Map;
    final earlierSummary = find.byValueKey(
      'timeline-tool-group-summary-${earlier['id']}',
    );
    final laterSummary = find.byValueKey(
      'timeline-tool-group-summary-${later['id']}',
    );
    await place(earlierSummary, 120);
    await probe(earlierSummary, 'tap');
    await place(laterSummary, 120);
    await probe(laterSummary, 'tap');
    final earlierTile = find.byValueKey(
      'timeline-tool-details:${(earlier['tools'] as List).first['itemId']}',
    );
    await place(earlierTile, 125);
    evidence['reverseExpansion'] = await probe(earlierTile, 'tap');
    await File('${output.path}/reverse-expanded.png')
        .writeAsBytes(await driver.screenshot());
    await place(earlierTile, 125);
    evidence['reverseCollapse'] = await probe(earlierTile, 'tap');
    bool headerStable(Object? probe) {
      final samples = (probe as Map)['samples'] as List;
      final initial = samples.first['top'] as num;
      return samples.every((s) => ((s['top'] as num) - initial).abs() < 1);
    }

    final checks = <bool>[];
    checks.add(headerStable(evidence['reverseExpansion']));
    checks.add(headerStable(evidence['reverseCollapse']));
    for (final c in cases.cast<Map>()) {
      checks.add(headerStable(c['groupOpen']));
      checks.add(headerStable(c['groupClose']));
      for (final t in (c['tiles'] as List).cast<Map>()) {
        checks.add(headerStable(t['expanded']));
        checks.add(headerStable(t['collapsed']));
        final expanded = (t['expanded'] as Map)['samples'] as List;
        final collapsed = (t['collapsed'] as Map)['samples'] as List;
        checks.add(
          (expanded.last['height'] as num) >
              (expanded.first['height'] as num) + 20,
        );
        checks.add(
          (collapsed.last['height'] as num) <
              (collapsed.first['height'] as num) - 20,
        );
        final inside = t['inside'] as Map?;
        if (inside != null) {
          final samples = inside['samples'] as List;
          final before = samples.first as Map;
          final after = samples.last as Map;
          checks.add(
            ((before['scroll']['pixels'] as num) -
                        (after['scroll']['pixels'] as num))
                    .abs() <
                0.5,
          );
          final nestedBefore = (before['nested'] as List)
              .where((n) => n['axis'] == 'vertical' && (n['max'] as num) > 0)
              .last;
          final nestedAfter = (after['nested'] as List)
              .where((n) => n['axis'] == 'vertical' && (n['max'] as num) > 0)
              .last;
          checks.add(
            (nestedAfter['pixels'] as num) > (nestedBefore['pixels'] as num),
          );
        }
      }
    }
    evidence['checks'] = checks;
    evidence['checksPassed'] = checks.every((v) => v);
    if (evidence['checksPassed'] != true) exitCode = 1;
  } finally {
    await File('${output.path}/tool-scroll.json')
        .writeAsString(const JsonEncoder.withIndent('  ').convert(evidence));
    await driver.close();
  }
}
