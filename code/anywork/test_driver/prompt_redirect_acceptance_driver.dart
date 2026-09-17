import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

Future<void> main(List<String> args) async {
  if (args.length != 2) {
    throw ArgumentError('Expected VM service URL and artifact directory');
  }
  final artifacts = Directory(args[1]);
  await artifacts.create(recursive: true);
  final session = await FlutterDriver.connect(
    dartVmServiceUrl: args[0],
    logCommunicationToFile: false,
  );
  await session.sendCommand(SetFrameSync(false));
  Future<Map<String, dynamic>> snapshot() async =>
      jsonDecode(await session.requestData('snapshot')) as Map<String, dynamic>;
  Future<Map<String, dynamic>> until(
    bool Function(Map<String, dynamic>) matches,
  ) async {
    final deadline = DateTime.now().add(const Duration(seconds: 45));
    while (DateTime.now().isBefore(deadline)) {
      final state = await snapshot();
      if (matches(state)) return state;
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    throw StateError('Expected prompt transition did not arrive');
  }

  Future<void> send(String message) async {
    final before = await snapshot();
    if (before['workspace'] != null) {
      await until(
        (state) =>
            ((state['workspace'] as Map?)?['composer']
                as Map?)?['submissionPending'] ==
            false,
      );
    }
    await session.tap(find.byValueKey('composer-input'));
    await session.enterText(message);
    if (await session.getText(find.byValueKey('composer-input')) != message) {
      throw StateError('Composer did not retain the entered prompt');
    }
    final deadline = DateTime.now().add(const Duration(seconds: 15));
    while (true) {
      final button = await session.getWidgetDiagnostics(
        find.byValueKey('composer-submit'),
      );
      final properties = (button['properties'] as List).cast<Map>();
      if (properties.any(
        (property) =>
            property['name'] == 'onPressed' &&
            !(property.containsKey('value') && property['value'] == null) &&
            !(property['description'] as String).contains('disabled'),
      )) {
        break;
      }
      if (DateTime.now().isAfter(deadline)) {
        throw StateError('Submit button did not become enabled');
      }
      await Future<void>.delayed(const Duration(milliseconds: 50));
    }
    if (before['workspace'] != null &&
        ((await snapshot())['workspace'] as Map?)?['isBusy'] != true) {
      throw StateError('Original turn finished before redirect submission');
    }
    await File(
      '${artifacts.path}/before-submit-${before['workspace'] == null ? 'first' : 'redirect'}.json',
    ).writeAsString(jsonEncode(await snapshot()));
    await File(
      '${artifacts.path}/before-submit-${before['workspace'] == null ? 'first' : 'redirect'}.png',
    ).writeAsBytes(await session.screenshot());
    await session.tap(find.byValueKey('composer-submit'));
  }

  try {
    await session.waitFor(find.byValueKey('studio-shell'));
    await session.requestData('prepare-session-lifecycle-demo');
    await session.tap(find.byValueKey('sidebar-new-session'));
    await session.waitFor(find.byValueKey('studio-start-page'));
    await send('first prompt awaiting redirection');
    await until((state) => (state['workspace'] as Map?)?['isBusy'] == true);
    await session.waitFor(find.byValueKey('composer-stop'));
    await send('inserted prompt redirects execution');
    final redirected = await until((state) {
      final workspace = state['workspace'] as Map?;
      final rows = workspace?['timeline'] as List? ?? [];
      return workspace?['isBusy'] == true &&
          rows.any(
            (row) =>
                (row as Map)['text'] == 'inserted prompt redirects execution',
          );
    });
    await File('${artifacts.path}/running.json')
        .writeAsString(jsonEncode(redirected));
    await File('${artifacts.path}/running.png')
        .writeAsBytes(await session.screenshot());
    await session.waitFor(find.byValueKey('composer-stop'));
    await session.tap(find.byValueKey('composer-stop'));
    final stopped = await until(
      (state) => (state['workspace'] as Map?)?['isBusy'] == false,
    );
    final rows = ((stopped['workspace'] as Map)['timeline'] as List)
        .cast<Map>();
    for (final message in [
      'first prompt awaiting redirection',
      'inserted prompt redirects execution',
    ]) {
      if (rows
              .where(
                (row) => row['type'] == 'userMessage' && row['text'] == message,
              )
              .length !=
          1) {
        throw StateError('Prompt was lost or duplicated: $message');
      }
    }
    if (rows.where((row) => row['type'] == 'turnOutcome').length < 2) {
      throw StateError('Both cancelled turns must remain in the timeline');
    }
    await File('${artifacts.path}/stopped.json')
        .writeAsString(jsonEncode(stopped));
    await File('${artifacts.path}/stopped.png')
        .writeAsBytes(await session.screenshot());
    stdout.writeln('PROMPT_REDIRECT_DRIVER_PASSED');
  } finally {
    try {
      await session.requestData(
        'shutdown-await',
        timeout: const Duration(minutes: 1),
      );
    } finally {
      await session.close();
    }
  }
}
