import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

/// Captures real startup animation frames, then verifies return to the shell.
/// Run against `cargo xtask run-gui --demo --driver` with VM URL and output dir.
Future<void> main(List<String> arguments) async {
  if (arguments.length != 2) {
    throw ArgumentError('Expected VM service URL and screenshot directory');
  }
  final output = Directory(arguments[1]);
  await output.create(recursive: true);
  final driver = await FlutterDriver.connect(
    dartVmServiceUrl: arguments[0],
    printCommunication: false,
    logCommunicationToFile: false,
  );
  try {
    await driver.waitFor(find.byValueKey('studio-shell'));
    await driver.runUnsynchronized(() async {
      final response = jsonDecode(
        await driver.requestData('preview-startup-demo'),
      ) as Map<String, dynamic>;
      if (response['startupPreview'] != true) {
        throw StateError('Startup preview failed: $response');
      }
      await driver.waitFor(find.byValueKey('studio-startup'));
      final animation = find.byValueKey('studio-startup-animation');
      final topLeft = await driver.getTopLeft(animation);
      final bottomRight = await driver.getBottomRight(animation);
      await File('${output.path}/animation-bounds.json').writeAsString(
        jsonEncode({
          'left': topLeft.dx,
          'top': topLeft.dy,
          'right': bottomRight.dx,
          'bottom': bottomRight.dy,
        }),
      );
      // Fixed observation window; no pumpAndSettle on an infinite animation.
      for (var i = 0; i < 12; i++) {
        await Future<void>.delayed(const Duration(milliseconds: 75));
        await File('${output.path}/startup-$i.png')
            .writeAsBytes(await driver.screenshot());
      }
      final tree = (await driver.getRenderTree()).tree;
      await File('${output.path}/startup-tree.txt').writeAsString(tree ?? '');
      if (tree == null || tree.contains('OVERFLOWING')) {
        throw StateError('Startup render tree missing or overflowing');
      }
      await driver.requestData('finish-startup-demo');
    });
    await driver.waitFor(find.byValueKey('studio-shell'));
    await File('${output.path}/ready.png')
        .writeAsBytes(await driver.screenshot());
    stdout.writeln('Startup frames captured; no overflow; shell restored.');
  } finally {
    try {
      await driver.runUnsynchronized(() async {
        await driver.requestData('finish-startup-demo');
        await driver.requestData('shutdown');
      });
    } finally {
      await driver.close();
    }
  }
}
