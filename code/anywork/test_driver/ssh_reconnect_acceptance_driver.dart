// Native SSH settings acceptance. Use an isolated ANYWORK_HOME for the GUI.
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

Future<void> main(List<String> args) async {
  if (args.length != 4) {
    throw ArgumentError(
      'Expected VM service URL, SSH host, username, screenshot path',
    );
  }
  final session = await FlutterDriverSession.connect(vmServiceUrl: args[0]);
  try {
    await session.tap(find.byValueKey('settings-open'));
    await session.tap(find.byValueKey('settings-tab-ssh'));
    await session.tap(find.byValueKey('ssh-add-server'));
    for (final field in [
      ('ssh-server-name-input', 'SSH environment acceptance'),
      ('ssh-server-host-input', args[1]),
      ('ssh-server-username-input', args[2]),
    ]) {
      await session.tap(find.byValueKey(field.$1));
      await session.enterText(field.$2);
    }
    await session.tap(find.byValueKey('ssh-server-save'));
    await session.waitForAbsent(find.byValueKey('ssh-server-dialog'));
    final reply = jsonDecode(
      await session.requestData('ssh-server-id:SSH environment acceptance'),
    ) as Map<String, dynamic>;
    final id = reply['serverId'] as String;
    // No connection test is needed before the reconnect action can be used.
    await session.tap(find.byValueKey('ssh-reconnect-$id'));
    await session.waitFor(
      find.byValueKey('ssh-ready-$id'),
      timeout: const Duration(seconds: 60),
    );
    await session.tap(find.byValueKey('ssh-reconnect-$id'));
    await session.waitFor(
      find.byValueKey('ssh-ready-$id'),
      timeout: const Duration(seconds: 60),
    );
    await File(args[3]).writeAsBytes(await session.screenshot());
    await File('${args[3]}.tree.txt').writeAsString(await session.renderTree());
    stdout.writeln('SSH settings reconnect acceptance passed');
    await session.requestData(
      'shutdown-await',
      timeout: const Duration(seconds: 30),
    );
  } finally {
    await session.close();
  }
}
