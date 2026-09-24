import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

Future<void> main(List<String> args) async {
  if (args.length != 6 || !const ['first', 'restart'].contains(args[0])) {
    stderr.writeln(
      'usage: statistics_journey.dart first|restart VM_URL PROJECT_DIR OUTPUT_DIR COORD_DIR EXPECTED_SAMPLES',
    );
    exitCode = 64;
    return;
  }
  final phase = args[0];
  final output = Directory(args[3]);
  final coord = Directory(args[4]);
  final expected = int.parse(args[5]);
  final driver = await FlutterDriverSession.connect(vmServiceUrl: args[1]);
  Future<void> stage(String name) =>
      File('${coord.path}/statistics-stage').writeAsString(name);
  try {
    await stage('${phase}_connected');
    await _openStatistics(driver);
    final initial = await _readStatistics(driver);
    if (phase == 'restart') {
      _verifyPopulated(initial, expected);
      _verifyHealthy(initial);
      await _capture(driver, output, 'statistics-restarted', initial);
      await stage('restart_verified');
    } else {
      if (_sampleCount(initial) != 0 ||
          (initial['history'] as List).isNotEmpty ||
          initial['statisticsPending'] != false ||
          initial['statisticsGap'] != false ||
          initial['readFailed'] != false) {
        throw StateError('isolated statistics page was not empty');
      }
      await _capture(driver, output, 'statistics-empty', initial);
      await stage('empty_verified');
      await driver.tap(find.byValueKey('settings-back'));
      await _openProject(driver, args[2]);
      await stage('ready_lock');
      await _waitForFile(File('${coord.path}/lock-acquired'));
      await _submit(driver, 'Local statistics fast response');
      await File('${coord.path}/fast-submitted').writeAsString('submitted');
      await stage('fast_submitted');
      await _openStatistics(driver);
      final locked = await _waitForPending(driver);
      await _captureState(output, 'statistics-locked', locked);
      await File('${coord.path}/lock-observed').writeAsString('observed');
      await stage('lock_observed');
      await _waitForTurn(driver, 5, output, 'fast');
      final fast = await _waitForStatistics(driver, 1);
      _verifyHealthy(fast);
      await _capture(driver, output, 'statistics-fast', fast);
      await stage('fast_verified');

      await driver.tap(find.byValueKey('settings-back'));
      await driver.waitFor(find.byValueKey('sidebar-new-session'));
      await driver.tap(find.byValueKey('sidebar-new-session'));
      await driver.waitFor(find.byValueKey('composer-input'));
      await _submit(driver, 'Local statistics paced response');
      await stage('paced_submitted');
      await _waitForTurn(driver, 25, output, 'paced');
      final workspace = (await driver.readSnapshot())['workspace'];
      final turn = workspace is Map ? workspace['lastTurn'] : null;
      final turnFields = turn is Map ? turn : const {};
      final diagnostic = {
        'status': turnFields['status'],
        'reason': turnFields['reason'],
        'usage': workspace is Map ? workspace['usage'] : null,
      };
      await File('${output.path}/statistics-paced-turn.json').writeAsString(
        '${const JsonEncoder.withIndent('  ').convert(diagnostic)}\n',
      );
      await _openStatistics(driver);
      final paced = await _waitForStatistics(
        driver,
        (fast['history'] as List).length + 1,
      );
      await _captureState(output, 'statistics-paced-diagnostics', paced);
      _verifyPopulated(paced, (fast['history'] as List).length + 1);
      _verifyHealthy(paced);
      await _capture(driver, output, 'statistics-final', paced);
      await stage('paced_verified');
    }
    final result = jsonDecode(
      await driver
          .requestData('shutdown', timeout: const Duration(seconds: 60))
          .timeout(const Duration(seconds: 65)),
    );
    if (result is! Map || result['shutdown'] != 'completed') {
      throw StateError('native GUI shutdown failed');
    }
    await stage('${phase}_shutdown');
  } finally {
    try {
      await driver.close().timeout(const Duration(seconds: 5));
    } catch (_) {
      // The native shutdown can close the observation connection first.
    }
  }
}

Future<void> _openStatistics(FlutterDriverSession driver) async {
  await driver.waitFor(
    find.byValueKey('settings-open'),
    timeout: const Duration(seconds: 60),
  );
  await driver.tap(find.byValueKey('settings-open'));
  await driver.waitFor(find.byValueKey('settings-page'));
  await driver.tap(find.byValueKey('settings-tab-statistics'));
  await driver.waitFor(find.byValueKey('statistics-summary'));
  await driver.waitFor(find.byValueKey('statistics-history'));
}

Future<void> _openProject(FlutterDriverSession driver, String path) async {
  await driver.waitFor(find.byValueKey('sidebar-open-project'));
  await driver.tap(find.byValueKey('sidebar-open-project'));
  await driver.tap(find.byValueKey('add-project-local'));
  await driver.waitFor(find.byValueKey('add-project-continue-ready'));
  await driver.tap(find.byValueKey('add-project-continue-ready'));
  await driver.waitFor(find.byValueKey('project-path-input'));
  await driver.tap(find.byValueKey('project-path-input'));
  await driver.enterText(path);
  await driver.waitFor(find.byValueKey('project-path-submit'));
  await driver.tap(find.byValueKey('project-path-submit'));
  await driver.waitFor(
    find.byValueKey('composer-input'),
    timeout: const Duration(seconds: 60),
  );
}

Future<void> _submit(FlutterDriverSession driver, String prompt) async {
  await driver.tap(find.byValueKey('composer-input'));
  await driver.enterText(prompt);
  await driver.waitFor(find.byValueKey('composer-submit'));
  await driver.tap(find.byValueKey('composer-submit'));
}

Future<void> _waitForTurn(
  FlutterDriverSession driver,
  int minimumTokens,
  Directory output,
  String label,
) async {
  final deadline = DateTime.now().add(const Duration(seconds: 90));
  Map<String, dynamic>? lastSnapshot;
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await driver.readSnapshot();
    lastSnapshot = snapshot;
    final workspace = snapshot['workspace'];
    final persistence = snapshot['persistence'];
    final turn = workspace is Map ? workspace['lastTurn'] : null;
    final usage = workspace is Map ? workspace['usage'] : null;
    final outputTokens = usage is Map ? usage['outputTokens'] : null;
    if (turn is Map &&
        const [
          'failed',
          'cancelled',
          'budgetLimited',
        ].contains(turn['status'])) {
      throw StateError(
        'model turn ended as ${turn['status']}: ${turn['reason']}',
      );
    }
    if (workspace is Map &&
        turn is Map &&
        workspace['threadStatus'] == 'idle' &&
        workspace['isBusy'] == false &&
        workspace['syncState'] == 'ready' &&
        outputTokens is num &&
        outputTokens >= minimumTokens &&
        persistence is Map &&
        persistence['kind'] == 'ready' &&
        persistence['pendingCommits'] == 0) {
      return;
    }
    await Future<void>.delayed(const Duration(milliseconds: 200));
  }
  final workspace = lastSnapshot?['workspace'];
  final persistence = lastSnapshot?['persistence'];
  final fields = workspace is Map ? workspace : const {};
  final lastTurn = fields['lastTurn'];
  final turnFields = lastTurn is Map ? lastTurn : const {};
  final diagnostic = {
    'threadStatus': fields['threadStatus'],
    'isBusy': fields['isBusy'],
    'syncState': fields['syncState'],
    'lastTurnStatus': turnFields['status'],
    'lastTurnReason': turnFields['reason'],
    'usage': fields['usage'],
    'persistence': persistence,
  };
  await File('${output.path}/statistics-$label-turn-timeout.json')
      .writeAsString(
        '${const JsonEncoder.withIndent('  ').convert(diagnostic)}\n',
      );
  throw StateError('model turn did not complete and persist');
}

Future<Map<String, dynamic>> _readStatistics(
  FlutterDriverSession driver,
) async {
  final data = jsonDecode(await driver.requestData('statistics'));
  if (data is! Map<String, dynamic> ||
      data['revision'] is! int ||
      data['statisticsPending'] is! bool ||
      data['statisticsGap'] is! bool ||
      data['readFailed'] is! bool ||
      data['summaries'] is! List ||
      data['history'] is! List) {
    throw StateError('statistics bridge snapshot unavailable');
  }
  return data;
}

Future<Map<String, dynamic>> _waitForPending(
  FlutterDriverSession driver,
) async {
  final deadline = DateTime.now().add(const Duration(seconds: 18));
  while (DateTime.now().isBefore(deadline)) {
    final data = await _readStatistics(driver);
    if (data['statisticsPending'] == true) {
      if (data['statisticsGap'] != false || data['readFailed'] != false) {
        throw StateError(
          'statistics projection unhealthy while SQLite is locked: pending=${data['statisticsPending']} gap=${data['statisticsGap']} readFailed=${data['readFailed']}',
        );
      }
      return data;
    }
    await Future<void>.delayed(const Duration(milliseconds: 100));
  }
  throw StateError('statisticsPending not observed under BEGIN IMMEDIATE');
}

void _verifyHealthy(Map<String, dynamic> data) {
  if (data['statisticsPending'] != false ||
      data['statisticsGap'] != false ||
      data['readFailed'] != false) {
    throw StateError('statistics projection did not settle after lock release');
  }
}

int _sampleCount(Map<String, dynamic> data) => (data['summaries'] as List)
    .fold<int>(0, (count, summary) => count + (summary['samples'] as int));

Future<Map<String, dynamic>> _waitForStatistics(
  FlutterDriverSession driver,
  int minimum,
) async {
  final deadline = DateTime.now().add(const Duration(seconds: 60));
  while (DateTime.now().isBefore(deadline)) {
    final data = await _readStatistics(driver);
    if ((data['history'] as List).length >= minimum &&
        (minimum == 1 ||
            (_sampleCount(data) > 0 &&
                (data['history'] as List).any(
                  (row) =>
                      row['tokens'] >= 25 && (row['decodeMillis'] ?? 0) > 0,
                )))) {
      return data;
    }
    await Future<void>.delayed(const Duration(milliseconds: 200));
  }
  throw StateError(
    'statistics did not automatically refresh to $minimum samples',
  );
}

void _verifyPopulated(Map<String, dynamic> data, int minimum) {
  if (_sampleCount(data) < 1 ||
      (data['history'] as List).length < minimum ||
      !(data['summaries'] as List).any(
        (item) => item['model'] == 'fixture-model' && item['tokens'] > 0,
      ) ||
      !(data['history'] as List).any(
        (item) =>
            item['model'] == 'fixture-model' &&
            item['tokens'] >= 25 &&
            item['responseMillis'] > 0,
      )) {
    throw StateError('statistics summary, history, or paced sample missing');
  }
}

Future<void> _capture(
  FlutterDriverSession driver,
  Directory output,
  String name,
  Map<String, dynamic> data,
) async {
  // The Driver data endpoint omits prompts, paths, credentials and call bodies.
  await _captureState(output, name, data);
  await File('${output.path}/$name.png')
      .writeAsBytes(await driver.screenshot());
  if ((data['history'] as List).isNotEmpty) {
    final item = (data['history'] as List).first as Map;
    final key =
        'statistics-history-row:${jsonEncode([item['providerInstanceId'], item['model'], item['effort'], 0])}';
    await driver.scrollUntilVisible(
      find.byValueKey('statistics-history'),
      find.byValueKey(key),
      dyScroll: -200,
      timeout: const Duration(seconds: 30),
    );
    await driver.waitFor(find.byValueKey(key));
    await File('${output.path}/$name-history.png')
        .writeAsBytes(await driver.screenshot());
  }
}

Future<void> _captureState(
  Directory output,
  String name,
  Map<String, dynamic> data,
) async {
  await File('${output.path}/$name.json').writeAsString(
    '${const JsonEncoder.withIndent('  ').convert({
      'revision': data['revision'],
      'statisticsPending': data['statisticsPending'],
      'statisticsGap': data['statisticsGap'],
      'readFailed': data['readFailed'],
      'samples': _sampleCount(data),
      'summaries': (data['summaries'] as List).length,
      'history': (data['history'] as List).length,
      'models': [for (final row in data['summaries'] as List) row['model']],
      'metrics': [
        for (final row in data['history'] as List) {'model': row['model'], 'tokens': row['tokens'], 'decodeMillis': row['decodeMillis'], 'responseMillis': row['responseMillis']},
      ],
    })}\n',
  );
}

Future<void> _waitForFile(File file) async {
  final deadline = DateTime.now().add(const Duration(seconds: 30));
  while (DateTime.now().isBefore(deadline)) {
    if (await file.exists() && await file.readAsString() == 'acquired') return;
    await Future<void>.delayed(const Duration(milliseconds: 100));
  }
  throw StateError('calls.sqlite BEGIN IMMEDIATE lock was not acquired');
}
