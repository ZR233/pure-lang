// 长历史（1k / 1M）原生 GUI 规模验收驱动。
//
// 只由 `tool/project_session_scale_harness.py` 启动：harness 先把一份真实 Studio home
// 复制到独立目录并注入 1000 / 1000000 条自洽历史，再用
// `cargo xtask run-gui --driver` 打开真实 Linux 原生 Studio，然后运行本驱动：
//
//   cargo dart run test_driver/project_session_scale_driver.dart \
//     --vm-service-url <url> --output <dir> --fixture 1k --thread-id <threadId> \
//     [--expected-last-turn-id <turnId>] [--expected-latest-item-id <itemId>] \
//     [--expected-large-item-id <itemId>] [--window-timeout-ms <ms>]
//
// 本驱动只回答存储规模问题，不派生任何性能“通过”结论：
//   1. 启动时所选 Thread 自动打开，SQL 首窗有界，且确实还有更旧的历史；
//   3. 首窗最新条目仍是真实基线的最后 Turn 与超长正文条目；
//   4. 打开不得执行模型：wire-index.jsonl 的 conversation 计数不得增加；
//   5. 记录启动/打开阶段的时间戳与窗口身份，供外部 `/proc` 采样对齐；
//   6. `shutdown-await` 之后 Studio 必须干净关闭。
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'raw_tap.dart';

/// SQL 历史窗口上限：打开后只允许持有有限的一页；运行中 overlay 不设上限。
const _sqlHistoryWindowLimit = 500;

int _nowMs() => DateTime.now().millisecondsSinceEpoch;

Future<void> main(List<String> arguments) async {
  final options = _Options.parse(arguments);
  final output = Directory(options.output);
  await output.create(recursive: true);
  final driver = await FlutterDriver.connect(
    dartVmServiceUrl: options.vmServiceUrl,
    printCommunication: false,
    logCommunicationToFile: false,
  );
  await driver.sendCommand(const SetFrameSync(false));
  final context = _Context(driver: driver, options: options, output: output);
  Object? failure;
  StackTrace? failureStack;
  try {
    await context.run();
  } catch (error, stack) {
    failure = error;
    failureStack = stack;
    try {
      await context.capture('failure');
      await File('${output.path}/failure.json').writeAsString(
        jsonEncode({
          'error': '$error',
          'snapshot': await context.readSnapshot(),
        }),
      );
    } on Object catch (captureError) {
      stderr.writeln('Failure evidence unavailable: $captureError');
    }
  } finally {
    try {
      final shutdown = await driver.requestData(
        'shutdown-await',
        timeout: const Duration(minutes: 5),
      );
      final decoded = jsonDecode(shutdown) as Map;
      await File('${output.path}/shutdown.json')
          .writeAsString(jsonEncode({'atMs': _nowMs(), 'reply': decoded}));
      if (decoded['shutdown'] != 'completed' && failure == null) {
        failure = StateError('Studio shutdown did not complete: $shutdown');
      }
    } on Object catch (error) {
      if (failure == null) {
        failure = error;
        failureStack = StackTrace.current;
      } else {
        stderr.writeln('Shutdown after failure also failed: $error');
      }
    } finally {
      await driver.close();
    }
  }
  if (failure != null) {
    Error.throwWithStackTrace(failure, failureStack ?? StackTrace.current);
  }
}

class _Context {
  _Context({required this.driver, required this.options, required this.output});

  final FlutterDriver driver;
  final _Options options;
  final Directory output;

  Future<void> run() async {
    await driver.waitFor(
      find.byValueKey('studio-shell'),
      timeout: const Duration(minutes: 3),
    );
    // 侧栏只有在控制器发布权威状态后才渲染。
    await driver.waitFor(
      find.byValueKey('sidebar-open-project'),
      timeout: const Duration(minutes: 3),
    );
    final startup = await readSnapshot();
    final startupAtMs = _nowMs();
    await _writeJson('startup.json', startup);
    _require(
      (startup['navigation'] as Map)['selectedThreadId'] == options.threadId,
      'startup did not restore the selected Thread ${options.threadId}',
    );
    await capture('first-screen');

    // wire-index.jsonl is created empty before the GUI is launched.
    final requestsBefore = <String, Object?>{
      'available': true,
      'total': 0,
      'conversation': 0,
    };
    final opened = await _waitFirstWindow();
    final firstWindowAtMs = _nowMs();
    await _writeJson('opened.json', opened);
    await capture('opened');

    final window = opened['timelineWindow'] as Map;
    final itemIds = _itemIds(opened);
    final visibleItemCount = itemIds.length;
    final historyCount = _windowHistoryCount(window);
    _require(historyCount > 0, 'the opened Thread published no SQL history');
    // 打开后只能持有有限的 SQL 一页：百万级历史不得整段驻留。
    _require(
      historyCount <= _sqlHistoryWindowLimit,
      'the first SQL history window holds $historyCount items, above the '
      '$_sqlHistoryWindowLimit-item limit',
    );
    // 历史总量远大于一页，因此打开后的首窗必须还有更旧的一页可翻。
    _require(
      window['hasOlder'] == true,
      'the opened window claims no older history, but the fixture holds far '
      'more than one window (historyCount=$historyCount)',
    );
    // 打开不执行工作。
    final workspace = opened['workspace'] as Map?;
    _require(
      workspace != null && workspace['isBusy'] == false,
      'opening the saved Thread did not leave an idle workspace '
      '(workspace=${workspace == null ? 'absent' : workspace['isBusy']})',
    );
    final expectedLastTurn = options.expectedLastTurnId;
    if (expectedLastTurn != null) {
      _require(
        _lastTurnId(opened) == expectedLastTurn,
        'opening the scaled history did not preserve the real last Turn '
        '(${_lastTurnId(opened)} != $expectedLastTurn)',
      );
    }
    // 真实基线的最后 Turn 与超长正文必须仍是最新条目，而不是被注入的历史顶替。
    final expectedLatest = options.expectedLatestItemId;
    if (expectedLatest != null) {
      _require(
        itemIds.last == expectedLatest,
        'the newest item is ${itemIds.last}, not the real latest item '
        '$expectedLatest',
      );
    }
    final expectedLarge = options.expectedLargeItemId;
    final largeItemResident =
        expectedLarge != null && itemIds.contains(expectedLarge);
    final largeItemPreviewed =
        expectedLarge != null &&
        _previewedItemIds(opened).contains(expectedLarge);
    if (expectedLarge != null) {
      _require(
        largeItemResident || largeItemPreviewed,
        'the real oversized body $expectedLarge is not in the first window',
      );
    }
    final modelRequestGuard = _requireNoConversationRequests(
      requestsBefore,
      'scale open',
    );

    await _writeJson('timeline.json', {
      'fixture': options.fixture,
      'threadId': options.threadId,
      'startupAtMs': startupAtMs,
      'firstWindowAtMs': firstWindowAtMs,
      'openedObservedEndAtMs': _nowMs(),
      'startupToFirstWindowMs': firstWindowAtMs - startupAtMs,
    });
    await _writeJson('scale.json', {
      'fixture': options.fixture,
      'threadId': options.threadId,
      'sqlHistoryWindowLimit': _sqlHistoryWindowLimit,
      'startup': {
        'selectedThreadRestored': true,
        'navigation': startup['navigation'],
        'persistence': startup['persistence'],
      },
      'window': {
        'visibleItemCount': visibleItemCount,
        'historyCount': historyCount,
        'overlayCount': _windowOverlayCount(window),
        'firstItemId': itemIds.first,
        'lastItemId': itemIds.last,
        'itemIds': itemIds,
        'previewedItemIds': _previewedItemIds(opened),
        'loadedItemIds': _loadedItemIds(opened),
        'hasOlder': window['hasOlder'],
        'hasNewer': window['hasNewer'],
        'loading': window['loading'],
        'epoch': window['epoch'],
      },
      'open': {
        'lastTurnId': _lastTurnId(opened),
        'expectedLastTurnId': expectedLastTurn,
        'expectedLatestItemId': expectedLatest,
        'latestItemMatched':
            expectedLatest == null || itemIds.last == expectedLatest,
        'largeItemId': expectedLarge,
        'largeItemResident': largeItemResident,
        'largeItemPreviewed': largeItemPreviewed,
        'workspaceBusy': workspace?['isBusy'],
        'syncState': workspace?['syncState'],
      },
      'modelRequestGuard': modelRequestGuard,
      'persistence': opened['persistence'],
      // 缩略的观测，供操作者判断历史规模是否影响了渲染；不作为性能判据。
      'observations': {'navigation': opened['navigation']},
      'findings': <String>[],
    });
  }

  Future<Map<String, dynamic>> readSnapshot() async {
    final raw = await driver.requestData(
      'snapshot',
      timeout: const Duration(seconds: 120),
    );
    return jsonDecode(raw) as Map<String, dynamic>;
  }

  /// 等到 SQL 首窗真正落地：窗口非空且不再处于加载态。
  Future<Map<String, dynamic>> _waitFirstWindow() async {
    final deadline = DateTime.now().add(options.windowTimeout);
    var sawLoading = false;
    Map<String, dynamic> state = const {};
    while (DateTime.now().isBefore(deadline)) {
      state = await readSnapshot();
      final window = state['timelineWindow'] as Map;
      if (window['loading'] == true) sawLoading = true;
      if ((state['workspace'] as Map?)?['threadId'] == options.threadId &&
          _itemIds(state).isNotEmpty &&
          window['loading'] != true) {
        return state;
      }
      await Future<void>.delayed(const Duration(milliseconds: 50));
    }
    throw StateError(
      'timed out waiting for the first history window '
      '(sawLoading=$sawLoading, observed ${jsonEncode(_brief(state))})',
    );
  }

  Future<void> capture(String name) async {
    await driver.waitForCondition(
      const NoPendingFrame(),
      timeout: const Duration(seconds: 20),
    );
    await File('${output.path}/$name.png')
        .writeAsBytes(await driver.screenshot());
    final tree = (await driver.getRenderTree()).tree ?? '';
    await File('${output.path}/$name.txt').writeAsString(tree);
  }

  Future<void> _writeJson(String name, Map<String, Object?> value) async {
    await File('${output.path}/$name')
        .writeAsString(jsonEncode(value), flush: true);
  }

  /// harness 的脚本化 provider 请求计数：只有带会话消息的请求才算一次模型对话。
  Map<String, Object?> _wireRequestStats() {
    final file = File('${output.path}/wire-index.jsonl');
    if (!file.existsSync()) {
      return {'available': false};
    }
    var total = 0;
    var conversation = 0;
    for (final line in file.readAsLinesSync()) {
      final trimmed = line.trim();
      if (trimmed.isEmpty) continue;
      total += 1;
      try {
        final entry = jsonDecode(trimmed) as Map;
        if (((entry['userMessages'] as num?) ?? 0) > 0) conversation += 1;
      } on Object {
        // 仍在刷新的最后一行不构成证据。
        continue;
      }
    }
    return {'available': true, 'total': total, 'conversation': conversation};
  }

  /// 打开一个已保存的会话不得重跑任何历史模型对话。
  Map<String, Object?> _requireNoConversationRequests(
    Map<String, Object?> before,
    String phase,
  ) {
    final after = _wireRequestStats();
    final available = before['available'] == true && after['available'] == true;
    final delta = available
        ? ((after['conversation'] as num?)?.toInt() ?? 0) -
              ((before['conversation'] as num?)?.toInt() ?? 0)
        : null;
    final guard = <String, Object?>{
      'available': available,
      'before': before,
      'after': after,
      'conversationDelta': delta,
    };
    _require(
      available,
      '$phase requires the harness wire index (wire-index.jsonl)',
    );
    _require(
      delta == 0,
      '$phase executed a model conversation request: ${jsonEncode(guard)}',
    );
    return guard;
  }
}

// ------------------------------------------------------------------- helpers

List<String> _itemIds(Map<String, dynamic> state) =>
    ((state['timelineWindow'] as Map)['itemIds'] as List).cast<String>();

List<String> _previewedItemIds(Map<String, dynamic> state) =>
    (((state['timelineWindow'] as Map)['previewedItemIds'] as List?) ??
            const [])
        .whereType<String>()
        .toList();

List<String> _loadedItemIds(Map<String, dynamic> state) =>
    (((state['timelineWindow'] as Map)['loadedItemIds'] as List?) ?? const [])
        .whereType<String>()
        .toList();

int _windowHistoryCount(Map window) =>
    ((window['historyCount'] as num?) ?? 0).toInt();

int _windowOverlayCount(Map window) =>
    ((window['overlayCount'] as num?) ?? 0).toInt();

String? _lastTurnId(Map<String, dynamic> state) =>
    ((state['workspace'] as Map?)?['lastTurn'] as Map?)?['id'] as String?;

Map<String, Object?> _brief(Map<String, dynamic> state) => {
  'selectedThreadId': (state['navigation'] as Map?)?['selectedThreadId'],
  'workspaceThreadId': (state['workspace'] as Map?)?['threadId'],
  'lastTurnId': _lastTurnId(state),
  'visibleItemCount': _itemIds(state).length,
  'persistence': state['persistence'],
  'window': state['timelineWindow'],
};

void _require(bool condition, String message) {
  if (!condition) throw StateError(message);
}

class _Options {
  const _Options({
    required this.vmServiceUrl,
    required this.output,
    required this.fixture,
    required this.threadId,
    required this.expectedLastTurnId,
    required this.expectedLatestItemId,
    required this.expectedLargeItemId,
    required this.windowTimeout,
  });

  final String vmServiceUrl;
  final String output;
  final String fixture;
  final String threadId;
  final String? expectedLastTurnId;
  final String? expectedLatestItemId;
  final String? expectedLargeItemId;
  final Duration windowTimeout;

  static _Options parse(List<String> arguments) {
    final values = <String, String>{};
    for (var index = 0; index < arguments.length; index += 2) {
      if (index + 1 >= arguments.length || !arguments[index].startsWith('--')) {
        throw const FormatException('Expected --name value arguments');
      }
      values[arguments[index].substring(2)] = arguments[index + 1];
    }
    String required(String name) {
      final value = values[name];
      if (value == null || value.isEmpty) {
        throw FormatException('Missing --$name');
      }
      return value;
    }

    final timeoutMs = int.tryParse(values['window-timeout-ms'] ?? '') ?? 600000;
    return _Options(
      vmServiceUrl: required('vm-service-url'),
      output: required('output'),
      fixture: values['fixture'] ?? 'unknown',
      threadId: required('thread-id'),
      expectedLastTurnId: values['expected-last-turn-id'],
      expectedLatestItemId: values['expected-latest-item-id'],
      expectedLargeItemId: values['expected-large-item-id'],
      windowTimeout: Duration(milliseconds: timeoutMs),
    );
  }
}
