import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

/// 验收顶栏工作区入口的固定图标、三个宿主应用条目与可定位语义。
Future<void> main(List<String> args) async {
  if (args.length != 2) {
    stderr.writeln(
      'usage: dart run test_driver/workspace_open_journey.dart VM_URL OUTPUT_DIR',
    );
    exitCode = 64;
    return;
  }
  final output = Directory(args[1]);
  await output.create(recursive: true);
  final driver = await FlutterDriverSession.connect(vmServiceUrl: args[0]);
  try {
    await driver.waitFor(find.byValueKey('thread-row-thread-main'));
    await driver.rawTap(find.byValueKey('thread-row-thread-main'));
    await driver.waitFor(find.byValueKey('session-open-workspace-menu'));
    await File('${output.path}/workspace-open-button.png')
        .writeAsBytes(await driver.screenshot());

    await driver.rawTap(find.byValueKey('session-open-workspace-menu'));
    for (final key in const [
      'session-open-workspace-vscode',
      'session-open-workspace-zed',
      'session-open-workspace-terminal',
    ]) {
      await driver.waitFor(find.byValueKey(key));
    }
    await File('${output.path}/workspace-open-menu.png')
        .writeAsBytes(await driver.screenshot());
    await File('${output.path}/workspace-open-report.json').writeAsString(
      '${const JsonEncoder.withIndent('  ').convert({
        'button': 'session-open-workspace-menu',
        'entries': ['session-open-workspace-vscode', 'session-open-workspace-zed', 'session-open-workspace-terminal'],
      })}\n',
    );

    final shutdown = jsonDecode(
      await driver.requestData(
        'shutdown',
        timeout: const Duration(seconds: 60),
      ),
    );
    if (shutdown is! Map || shutdown['shutdown'] != 'completed') {
      throw StateError('native GUI shutdown did not complete');
    }
  } finally {
    try {
      await driver.close().timeout(const Duration(seconds: 5));
    } catch (_) {
      // 应用完成关闭后，驱动连接不一定仍可用。
    }
  }
}
