import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

/// Complete settings → canonical save → model selection → task acceptance.
/// Native runs replace only the external HTTP provider with this local fixture.
Future<void> main(List<String> arguments) async {
  if (arguments.length < 2) {
    throw ArgumentError(
      'Usage: <vm-service-url> <artifact-directory> [--demo]',
    );
  }
  final artifacts = Directory(arguments[1]).absolute;
  await artifacts.create(recursive: true);
  final demo = arguments.contains('--demo');
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  final requests = <Map<String, dynamic>>[];
  final serving = server.forEach((request) async {
    final payload = jsonDecode(
      await utf8.decoder.bind(request).join(),
    ) as Map<String, dynamic>;
    requests.add({'path': request.uri.path, 'model': payload['model']});
    request.response.headers.contentType = ContentType('text', 'event-stream');
    final tools = (payload['tools'] as List?) ?? const [];
    final completeTool = tools
        .where((tool) => tool['function']?['name'] == 'complete')
        .firstOrNull;
    final chunks = [
      {
        'id': 'acceptance-${requests.length}',
        'model': 'local-coder',
        'choices': [
          {
            'delta': completeTool == null
                ? {'content': 'Provider acceptance complete.'}
                : {
                    'tool_calls': [
                      {
                        'index': 0,
                        'id': 'complete-${requests.length}',
                        'type': 'function',
                        'function': {
                          'name': 'complete',
                          'arguments': jsonEncode({
                            'summary': 'Provider acceptance complete.',
                            'evidence': [
                              'Canonical compatible route and final usage received.',
                            ],
                          }),
                        },
                      },
                    ],
                  },
            'finish_reason': completeTool == null ? 'stop' : 'tool_calls',
          },
        ],
      },
      {
        'id': 'acceptance-${requests.length}',
        'choices': [],
        'usage': {
          'prompt_tokens': 100,
          'completion_tokens': 10,
          'total_tokens': 110,
          'prompt_tokens_details': {'cached_tokens': 20},
        },
      },
    ];
    for (final chunk in chunks) {
      request.response.write('data: ${jsonEncode(chunk)}\n\n');
    }
    request.response.write('data: [DONE]\n\n');
    await request.response.close();
  });
  final driver = await FlutterDriverSession.connect(vmServiceUrl: arguments[0]);
  try {
    await driver.waitFor(
      find.byValueKey('studio-shell'),
      timeout: const Duration(minutes: 2),
    );
    await driver.tap(find.byValueKey('settings-open'));
    await driver.tap(find.byValueKey('settings-tab-providers'));
    await driver.tap(find.byValueKey('provider-add'));
    await driver.tap(find.byValueKey('provider-preset'));
    await driver.tap(find.text('OpenAI API 兼容'));
    await driver.tap(find.byValueKey('provider-base-url'));
    await driver.enterText('http://127.0.0.1:${server.port}/v1');
    final editor = find.byValueKey('provider-editor-scroll');
    await driver.scrollUntilVisible(
      editor,
      find.byValueKey('provider-model-add'),
      dyScroll: -300,
    );
    await driver.tap(find.byValueKey('provider-model-add'));
    await driver.scrollUntilVisible(
      editor,
      find.byValueKey('provider-model-0-id'),
      dyScroll: -250,
    );
    await driver.tap(find.byValueKey('provider-model-0-id'));
    await driver.enterText('local-coder');
    await driver.scrollUntilVisible(
      editor,
      find.byValueKey('provider-save'),
      dyScroll: 400,
    );
    await driver.tap(find.byValueKey('provider-save'));
    final saved = await _until(driver, (snapshot) {
      final settings = snapshot['settings'] as Map<String, dynamic>;
      return (settings['providers'] as List).any(
        (provider) =>
            provider['preset'] == 'openai-compatible' &&
            provider['defaultModel'] == 'local-coder',
      );
    });
    final provider = (saved['settings']['providers'] as List).singleWhere(
      (provider) => provider['preset'] == 'openai-compatible',
    );
    if (provider['pricingEnabled'] != false ||
        provider['hasBearerToken'] != false ||
        provider['status'] != 'ready') {
      throw StateError(
        'Compatible provider must save with pricing off and no credential',
      );
    }
    await File('${artifacts.path}/settings.json')
        .writeAsString(jsonEncode(saved));
    await File('${artifacts.path}/settings.png')
        .writeAsBytes(await driver.screenshot());
    await driver.tap(find.byValueKey('settings-back'));
    if (!demo) {
      final workspace = Directory('${artifacts.path}/workspace');
      await workspace.create(recursive: true);
      await driver.tap(find.byValueKey('sidebar-open-project'));
      await driver.tap(find.byValueKey('project-path-input'));
      await driver.enterText(workspace.path);
      await driver.waitUntilNoTransientCallbacks();
      await driver.tap(find.byValueKey('project-path-submit'));
      await _until(
        driver,
        (snapshot) => snapshot['project']?['path'] == workspace.path,
      );
    }
    await driver.tap(find.byValueKey('sidebar-new-session'));
    await driver.waitUntilNoTransientCallbacks();
    await driver.tap(find.byValueKey('model-selector'));
    await driver.tap(find.byValueKey('model-${provider['id']}-local-coder'));
    await _until(
      driver,
      (snapshot) => (snapshot['settings']['roles'] as List).any(
        (role) =>
            role['key'] == 'planner' &&
            role['providerId'] == provider['id'] &&
            role['model'] == 'local-coder' &&
            (role['effort'] == null || role['effort'] == ''),
      ),
    );
    await driver.waitUntilNoTransientCallbacks();
    await driver.tap(find.byValueKey('composer-input'));
    await driver.enterText('Reply exactly OK. Do not call tools.');
    await _until(
      driver,
      (snapshot) =>
          snapshot['navigation']['newThreadComposer']['draft'] ==
          'Reply exactly OK. Do not call tools.',
    );
    await driver.waitUntilNoTransientCallbacks();
    await driver.tap(find.byValueKey('composer-submit'));
    final completed = await _until(driver, (snapshot) {
      final workspace = snapshot['workspace'];
      return workspace is Map &&
          workspace['model'] == 'local-coder' &&
          workspace['lastTurn']?['status'] == 'completed';
    });
    if (!demo) {
      if (requests.isEmpty ||
          requests.any((request) => request['model'] != 'local-coder')) {
        throw StateError(
          'The selected compatible provider did not receive the task',
        );
      }
      final usage = completed['workspace']['usage'];
      if (usage['inputTokens'] < 100 || usage['cacheReadTokens'] < 20) {
        throw StateError('Final provider usage was not projected to the GUI');
      }
    }
    await File('${artifacts.path}/completed.json')
        .writeAsString(jsonEncode(completed));
    await File('${artifacts.path}/requests.json')
        .writeAsString(jsonEncode(requests));
    await File('${artifacts.path}/completed.png')
        .writeAsBytes(await driver.screenshot());
    stdout.writeln(
      jsonEncode({
        'result': 'passed',
        'mode': demo ? 'demo' : 'native',
        'providerId': provider['id'],
        'endpoint': 'http://127.0.0.1:${server.port}/v1',
        'requests': requests.length,
      }),
    );
  } finally {
    await driver.close();
    await server.close(force: true);
    await serving;
  }
}

Future<Map<String, dynamic>> _until(
  FlutterDriverSession driver,
  bool Function(Map<String, dynamic>) ready,
) async {
  final deadline = DateTime.now().add(const Duration(seconds: 90));
  Map<String, dynamic> snapshot = {};
  while (DateTime.now().isBefore(deadline)) {
    snapshot = await driver.readSnapshot();
    if (ready(snapshot)) return snapshot;
    await Future<void>.delayed(const Duration(milliseconds: 200));
  }
  throw StateError(
    'Provider acceptance did not reach its expected state: ${jsonEncode(snapshot)}',
  );
}
