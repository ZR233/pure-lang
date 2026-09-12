import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

/// Readable, repeatable screenshots of the real demo app, using Flutter Driver.
/// Run against `cargo xtask run-gui --demo --driver`; pass its VM URL and an
/// output directory. Resize only the isolated GUI window between runs.
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
  final captured = <String>[];
  Future<void> capture(String name) async {
    await File('${output.path}/$name.png')
        .writeAsBytes(await driver.screenshot());
    final tree = (await driver.getRenderTree()).tree;
    if (tree == null) throw StateError('Driver returned no render tree');
    await File('${output.path}/$name.txt').writeAsString(tree);
    if (tree.contains('OVERFLOWING')) {
      throw StateError('Render overflow in $name');
    }
    captured.add(name);
  }

  try {
    await driver.waitFor(
      find.byValueKey('studio-shell'),
      timeout: const Duration(seconds: 30),
    );
    await driver.requestData('prepare-theme-interactions-demo');
    await driver.waitFor(
      find.byValueKey('tool-approve'),
      timeout: const Duration(seconds: 30),
    );
    await capture('workspace');
    await driver.tap(
      find.byValueKey('settings-open'),
      timeout: const Duration(seconds: 30),
    );
    await driver.waitFor(
      find.byValueKey('settings-page'),
      timeout: const Duration(seconds: 30),
    );
    for (final tab in [
      'providers',
      'instructions',
      'skills',
      'agents',
      'mcp',
      'lsp',
      'ssh',
      'statistics',
      'security',
      'general',
    ]) {
      final target = find.byValueKey('settings-tab-$tab');
      await driver.scrollIntoView(target, timeout: const Duration(seconds: 30));
      await driver.tap(target, timeout: const Duration(seconds: 30));
      await capture('settings-$tab');
      if (tab == 'providers') {
        await driver.tap(
          find.byValueKey('provider-add'),
          timeout: const Duration(seconds: 30),
        );
        await driver.waitFor(
          find.byValueKey('provider-cancel'),
          timeout: const Duration(seconds: 30),
        );
        await capture('provider-editor');
        await driver.tap(
          find.byValueKey('provider-cancel'),
          timeout: const Duration(seconds: 30),
        );
      }
      if (tab == 'ssh') {
        await driver.tap(
          find.byValueKey('ssh-add-server'),
          timeout: const Duration(seconds: 30),
        );
        await driver.waitFor(
          find.byValueKey('ssh-server-dialog'),
          timeout: const Duration(seconds: 30),
        );
        await capture('ssh-dialog');
        await driver.tap(
          find.text('Cancel'),
          timeout: const Duration(seconds: 30),
        );
      }
    }
    // The horizontal navigation lazily unmounts the Back item after scrolling.
    // Bring the owning list back to its start before resolving that finder.
    await driver.scroll(
      find.ancestor(
        of: find.byValueKey('settings-tab-general'),
        matching: find.byType('ListView'),
      ),
      2000,
      2000,
      const Duration(milliseconds: 300),
      timeout: const Duration(seconds: 30),
    );
    await driver.scrollIntoView(
      find.byValueKey('settings-back'),
      timeout: const Duration(seconds: 30),
    );
    await driver.tap(
      find.byValueKey('settings-back'),
      timeout: const Duration(seconds: 30),
    );
    await driver.waitFor(
      find.byValueKey('studio-shell'),
      timeout: const Duration(seconds: 30),
    );
    await driver.tap(
      find.byValueKey('tool-approve'),
      timeout: const Duration(seconds: 30),
    );
    await driver.waitFor(
      find.byValueKey('user-input-first-text'),
      timeout: const Duration(seconds: 30),
    );
    await capture('question');
    await driver.tap(
      find.byValueKey('user-input-first-text'),
      timeout: const Duration(seconds: 30),
    );
    await driver.enterText('Continue');
    await driver.tap(
      find.byValueKey('user-input-submit'),
      timeout: const Duration(seconds: 30),
    );
    await driver.waitFor(
      find.byValueKey('composer-input'),
      timeout: const Duration(seconds: 30),
    );
    await capture('composer');
    await driver.tap(
      find.byValueKey('todo-open-button'),
      timeout: const Duration(seconds: 30),
    );
    await driver.waitFor(
      find.byValueKey('todo-close-button'),
      timeout: const Duration(seconds: 30),
    );
    await capture('todo');
    await driver.tap(
      find.byValueKey('todo-close-button'),
      timeout: const Duration(seconds: 30),
    );
    await driver.requestData('prepare-theme-plan-demo');
    await driver.waitFor(
      find.byValueKey('plan-details'),
      timeout: const Duration(seconds: 30),
    );
    await capture('plan');
    await driver.tap(
      find.byValueKey('plan-approve'),
      timeout: const Duration(seconds: 30),
    );
    await driver.waitFor(
      find.byValueKey('composer-input'),
      timeout: const Duration(seconds: 30),
    );
    await driver.requestData('prepare-persistence-failure-demo');
    await driver.waitFor(
      find.byValueKey('persistence-state-banner'),
      timeout: const Duration(seconds: 30),
    );
    await capture('persistence-error');
    await File('${output.path}/result.json').writeAsString(
      jsonEncode({'result': 'completed', 'screenshots': captured}),
    );
    stdout.writeln('Theme screenshots completed: ${captured.length}');
  } finally {
    await driver.close();
  }
}
