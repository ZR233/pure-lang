import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'raw_tap.dart';

/// Deterministic Linux demo acceptance: route editing and retired-child history.
Future<void> main(List<String> arguments) async {
  final output = Directory(arguments[1]);
  await output.create(recursive: true);
  final driver = await FlutterDriver.connect(
    dartVmServiceUrl: arguments[0],
    printCommunication: false,
    logCommunicationToFile: false,
  );
  await driver.sendCommand(const SetFrameSync(false));
  // Reuse the repository's Linux popup hit-testing workaround.
  Future<void> tap(String key) async {
    await driver.waitFor(
      find.byValueKey(key),
      timeout: const Duration(seconds: 20),
    );
    await driver.sendCommand(
      RawTap(find.byValueKey(key), timeout: const Duration(seconds: 20)),
    );
  }

  Future<Map<String, dynamic>> snapshot() async =>
      jsonDecode(await driver.requestData('snapshot')) as Map<String, dynamic>;
  Future<void> capture(String name) async {
    await driver.waitForCondition(
      const NoPendingFrame(),
      timeout: const Duration(seconds: 10),
    );
    await File('${output.path}/$name.png')
        .writeAsBytes(await driver.screenshot());
    final tree = (await driver.getRenderTree()).tree ?? '';
    await File('${output.path}/$name.txt').writeAsString(tree);
    if (tree.contains('OVERFLOWING')) throw StateError('Overflow in $name');
  }

  try {
    await driver.waitFor(find.byValueKey('studio-shell'));
    await tap('settings-open');
    await tap('settings-tab-agents');
    await driver.waitFor(find.byValueKey('main-agent-card'));
    await driver.waitForAbsent(find.byValueKey('system-agent-enabled-planner'));
    await driver.waitForAbsent(find.byValueKey('agent-profile-card-planner'));
    await driver.waitFor(find.byValueKey('settings-role-planner-effort'));
    final before = (await snapshot())['settings'] as Map<String, dynamic>;
    final role = (before['roles'] as List)
        .cast<Map<String, dynamic>>()
        .singleWhere((role) => role['key'] == 'planner');
    await driver.waitFor(find.byValueKey('settings-role-planner-model'));
    // The deterministic demo catalog has one model and three effort choices.
    const effort = 'max';
    if (role['effort'] == effort) {
      throw StateError('Demo must start with balanced effort');
    }
    await driver.scrollIntoView(
      find.byValueKey('settings-role-planner-effort'),
      alignment: 0.5,
    );
    await capture('00-main-agent-before-edit');
    await tap('settings-role-planner-effort');
    await capture('00-main-agent-effort-options');
    await tap('settings-role-planner-effort-$effort');
    await tap('settings-tab-general');
    await tap('settings-tab-agents');
    await driver.waitFor(find.byValueKey('main-agent-card'));
    final after = (await snapshot())['settings'] as Map<String, dynamic>;
    final saved = (after['roles'] as List)
        .cast<Map<String, dynamic>>()
        .singleWhere((role) => role['key'] == 'planner');
    if (saved['model'] != role['model'] ||
        saved['effort'] != effort ||
        (after['revision'] as int) <= (before['revision'] as int)) {
      throw StateError(
        'Main agent route did not persist in canonical settings',
      );
    }
    await capture('01-main-agent-settings');
    await tap('settings-back');
    final prepared = jsonDecode(
      await driver.requestData('prepare-retired-planner-demo'),
    ) as Map<String, dynamic>;
    if (prepared['prepared'] != true) throw StateError('$prepared');
    await tap('agent-switcher');
    await tap('agent-thread-thread-reviewer');
    await driver.waitFor(find.byValueKey('retired-agent-notice'));
    await driver.waitFor(find.text('Disabled'));
    await driver.waitForAbsent(find.text('Main agent'));
    await driver.waitFor(find.text('Driver agent workspace selected.'));
    await driver.waitForAbsent(find.byValueKey('composer-input'));
    await capture('02-retired-planner-history');
    await tap('agent-switcher');
    await tap('agent-thread-thread-main');
    await driver.waitForAbsent(find.byValueKey('retired-agent-notice'));
    await driver.waitFor(find.byValueKey('composer-input'));
    await File('${output.path}/complete.json').writeAsString(
      jsonEncode({
        'mainAgentRoute': saved,
        'retiredChildHistoryReadable': true,
        'rootComposerAvailable': true,
      }),
    );
  } finally {
    await driver.close();
  }
}
