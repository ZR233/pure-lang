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
  Future<Map<String, dynamic>> until(
    bool Function(Map<String, dynamic>) ready,
  ) async {
    final deadline = DateTime.now().add(const Duration(seconds: 30));
    Map<String, dynamic> state = {};
    while (DateTime.now().isBefore(deadline)) {
      state = await snapshot();
      if (ready(state)) return state;
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    throw StateError('Timed out waiting for canonical state: $state');
  }

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
    final lifecycle = jsonDecode(
      await driver.requestData('prepare-session-lifecycle-demo'),
    ) as Map<String, dynamic>;
    if (lifecycle['prepared'] != true) throw StateError('$lifecycle');
    await driver.waitFor(find.byValueKey('studio-shell'));
    await tap('settings-open');
    await tap('settings-tab-agents');
    await driver.waitFor(find.byValueKey('agent-profile-add'));
    await driver.waitForAbsent(find.byValueKey('main-agent-card'));
    await driver.waitForAbsent(find.byValueKey('system-agent-enabled-planner'));
    await driver.waitForAbsent(find.byValueKey('agent-profile-card-planner'));
    final agentSettings =
        (await snapshot())['settings'] as Map<String, dynamic>;
    if ((agentSettings['roles'] as List).any(
      (role) => role['key'] == 'planner',
    )) {
      throw StateError('Agents settings must not expose a planner route');
    }
    await capture('00-child-agent-settings');
    await tap('settings-back');

    await tap('thread-row-thread-main');
    final mainBefore = await until(
      (state) => state['navigation']['selectedThreadId'] == 'thread-main',
    );
    final mainBeforeRoute =
        mainBefore['workspace']['modelRoute'] as Map<String, dynamic>;
    if (mainBeforeRoute['effort'] != 'balanced') {
      throw StateError('Demo root must start with the Simple default route');
    }
    await driver.waitFor(find.byValueKey('composer-input'));
    await capture('01-thread-main-before-route-update');
    await tap('reasoning-effort-selector');
    await tap('reasoning-effort-max');
    final mainUpdated = await until((state) {
      final workspace = state['workspace'] as Map<String, dynamic>?;
      final settings = state['settings'] as Map<String, dynamic>;
      return workspace?['modelRoute']?['effort'] == 'max' &&
          (settings['modeModelRoutes'] as List).any(
            (route) =>
                route['modeId'] == 'mode.simple' && route['effort'] == 'max',
          );
    });
    await capture('02-thread-main-route-updated');

    await tap('thread-row-thread-alt');
    final alternate = await until(
      (state) => state['navigation']['selectedThreadId'] == 'thread-alt',
    );
    if (alternate['workspace']['modelRoute']['effort'] == 'max') {
      throw StateError('Updating one root Thread rewrote another root Thread');
    }
    await capture('03-thread-alt-route-isolated');

    await tap('thread-row-thread-main');
    await until(
      (state) => state['navigation']['selectedThreadId'] == 'thread-main',
    );
    await tap('session-mode-selector');
    await tap('session-mode-mode.task');
    final taskMode = await until((state) {
      final workspace = state['workspace'] as Map<String, dynamic>?;
      return workspace?['threadMode'] == 'mode.task' &&
          workspace?['modelRoute']?['effort'] == 'balanced';
    });
    await capture('04-task-mode-default-restored');

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
    await capture('05-retired-planner-history');
    await tap('agent-switcher');
    await tap('agent-thread-thread-main');
    await driver.waitForAbsent(find.byValueKey('retired-agent-notice'));
    await driver.waitFor(find.byValueKey('composer-input'));
    await File('${output.path}/complete.json').writeAsString(
      jsonEncode({
        'mainThreadRoute': mainUpdated['workspace']['modelRoute'],
        'alternateThreadRoute': alternate['workspace']['modelRoute'],
        'taskModeRoute': taskMode['workspace']['modelRoute'],
        'agentsPageHasMainAgentCard': false,
        'retiredChildHistoryReadable': true,
        'rootComposerAvailable': true,
      }),
    );
  } finally {
    await driver.close();
  }
}
