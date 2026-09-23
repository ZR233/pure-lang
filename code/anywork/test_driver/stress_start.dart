import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

Future<void> main(List<String> args) async {
  if (args.length != 4) {
    stderr.writeln(
      'usage: stress_start.dart VM_URL PROJECT_DIR PROMPT STAGE_FILE',
    );
    exitCode = 64;
    return;
  }
  Future<void> stage(String value) => File(args[3]).writeAsString(value);
  final driver = await FlutterDriverSession.connect(vmServiceUrl: args[0]);
  try {
    await stage('connected');
    await driver
        .waitFor(
          find.byValueKey('sidebar-open-project'),
          timeout: const Duration(seconds: 45),
        )
        .timeout(const Duration(seconds: 45));
    await stage('sidebar_ready');
    await driver.tap(find.byValueKey('sidebar-open-project'));
    await stage('project_dialog');
    await driver.tap(find.byValueKey('add-project-local'));
    await driver
        .waitFor(
          find.byValueKey('add-project-continue-ready'),
          timeout: const Duration(seconds: 15),
        )
        .timeout(const Duration(seconds: 15));
    await driver.tap(find.byValueKey('add-project-continue-ready'));
    await stage('path_dialog');
    try {
      await File('${args[3]}.png').writeAsBytes(
        await driver.screenshot().timeout(const Duration(seconds: 10)),
      );
      await File('${args[3]}.tree').writeAsString(
        await driver.renderTree().timeout(const Duration(seconds: 10)),
      );
    } catch (_) {
      // Diagnostic capture cannot decide whether the form itself can continue.
    }
    await driver
        .waitFor(find.byValueKey('project-path-input'))
        .timeout(const Duration(seconds: 30));
    await driver.tap(find.byValueKey('project-path-input'));
    await driver.enterText(args[1]);
    await driver
        .waitFor(find.byValueKey('project-path-submit'))
        .timeout(const Duration(seconds: 15));
    await driver.tap(find.byValueKey('project-path-submit'));
    await stage('project_submitted');
    await driver
        .waitFor(
          find.byValueKey('composer-input'),
          timeout: const Duration(seconds: 60),
        )
        .timeout(const Duration(seconds: 60));
    await stage('composer_ready');
    await driver.tap(find.byValueKey('composer-input'));
    await driver.enterText(args[2]);
    await driver
        .waitFor(find.byValueKey('composer-submit'))
        .timeout(const Duration(seconds: 15));
    await driver.tap(find.byValueKey('composer-submit'));
    await stage('prompt_submitted');
    stdout.writeln('stress_prompt_submitted');
  } finally {
    try {
      await driver.close().timeout(const Duration(seconds: 5));
    } catch (_) {
      // The command already has a result; a broken VM connection must not hang cleanup.
    }
  }
}
