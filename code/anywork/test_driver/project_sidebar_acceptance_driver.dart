import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

/// Exercise the project sidebar and wizard through real Flutter widgets.
/// Arguments: VM service URL, output directory, optional isolated X11 display.
Future<void> main(List<String> arguments) async {
  if (arguments.length < 2) {
    throw ArgumentError('VM URL and output directory required');
  }
  final output = Directory(arguments[1]);
  await output.create(recursive: true);
  final driver = await FlutterDriver.connect(
    dartVmServiceUrl: arguments[0],
    printCommunication: false,
    logCommunicationToFile: false,
  );
  Future<void> tap(String key) =>
      driver.tap(find.byValueKey(key), timeout: const Duration(seconds: 20));
  Future<void> capture(String name) async {
    await File('${output.path}/$name.png')
        .writeAsBytes(await driver.screenshot());
    final tree = (await driver.getRenderTree()).tree ?? '';
    await File('${output.path}/$name.txt').writeAsString(tree);
    if (tree.contains('OVERFLOWING')) throw StateError('Overflow in $name');
  }

  try {
    await driver.waitFor(
      find.byValueKey('studio-shell'),
      timeout: const Duration(seconds: 30),
    );
    await capture('01-project-sidebar');
    await tap('sidebar-open-project');
    await capture('02-project-location');
    await tap('add-project-remote');
    await tap('add-project-continue');
    await driver.waitFor(find.byValueKey('add-project-new-connection'));
    await capture('03-remote-connections');
    await tap('add-project-new-connection');
    await capture('04-new-connection');
    for (final field in {
      'ssh-server-name-input': 'Sidebar dev',
      'ssh-server-host-input': 'dev.example.com',
      'ssh-server-username-input': 'rui',
    }.entries) {
      await tap(field.key);
      await driver.enterText(field.value);
    }
    await tap('ssh-server-save');
    await driver.waitFor(find.byValueKey('ssh-directory-dialog'));
    await capture('05-remote-directory');
    await tap('ssh-open-current-directory');
    await driver.waitForAbsent(find.byValueKey('ssh-directory-dialog'));
    await capture('06-project-added');
    await tap('sidebar-new-session');
    await driver.waitFor(find.byValueKey('studio-start-page'));
    if (arguments.length >= 3) {
      final resized = await Process.run('python3', [
        'tool/sidebar_native_harness.py',
        '--resize',
        arguments[2],
      ]);
      if (resized.exitCode != 0) {
        throw StateError('Cannot resize isolated GUI: ${resized.stderr}');
      }
      await tap('sidebar-toggle');
      await driver.waitFor(find.byValueKey('studio-sidebar'));
      await capture('07-narrow-project-drawer');
    }
    await File('${output.path}/complete.txt').writeAsString(
      'Sidebar, remote creation, directory, project draft, and responsive navigation passed.\n',
    );
  } finally {
    await driver.close();
  }
}
