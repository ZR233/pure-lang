// 会话分页与关机阶段界面的 Flutter Driver 验收。
//
// 用法（由 tool/ 下 harness 启动 `cargo xtask run-gui --demo --driver` 后连接）：
//   dart run test_driver/session_paging_acceptance_driver.dart \
//     --vm-service-url <url> [--seed-threads 40]
//
// 验收内容：
//   1. 侧栏目录触底翻页直到加载完毕（确定性数量断言）；
//   2. shutdown-begin 后抓取关机阶段 overlay 截图，snapshot 中的阶段序列完整
//      且 FlushingPersistence 以 pending=0 结束；
//   3. shutdown-await 等待数据库落库完成后返回。

import 'dart:convert';
import 'dart:io';

import 'flutter_driver_session.dart';

const expectedShutdownPhaseOrder = <String>[
  'stoppingSubscriptions',
  'cancellingTurns',
  'flushingPersistence',
  'stoppingAgents',
  'stoppingMcp',
  'stoppingLsp',
  'stopped',
];

Future<void> main(List<String> args) async {
  final arguments = _parseArguments(args);
  final vmServiceUrl = arguments['vm-service-url'];
  if (vmServiceUrl == null) {
    stderr.writeln(
      'usage: session_paging_acceptance_driver.dart --vm-service-url <url>',
    );
    exitCode = 2;
    return;
  }
  final client = await connectFlutterDriverClient(vmServiceUrl);
  await client.checkHealth();

  final seedCount = int.tryParse(arguments['seed-threads'] ?? '');
  if (seedCount != null) {
    final seeded = await client.requestData('seed-threads:$seedCount');
    stdout.writeln('seeded: $seeded');
  }

  await _acceptSidebarPaging(client);
  await _acceptShutdownPhases(client);
  await client.close();
  stdout.writeln('session paging acceptance: ok');
}

Future<void> _acceptSidebarPaging(FlutterDriverClient client) async {
  var loaded = 0;
  for (var round = 0; round < 12; round += 1) {
    // 等价于触底：app 内直接调用 loadMoreThreads 并回报窗口状态。
    await client.requestData('sidebar-load-more');
    await Future<void>.delayed(const Duration(milliseconds: 250));
    final snapshot = await _snapshot(client);
    loaded = (_sidebarDirectory(snapshot)['count'] as num?)?.toInt() ?? 0;
    final hasMore = _sidebarDirectory(snapshot)['hasMore'] == true;
    stdout.writeln(
      'paging round ${round + 1}: loaded=$loaded hasMore=$hasMore',
    );
    if (!hasMore) break;
  }
  if (loaded < 40) {
    throw StateError('expected >= 40 loaded directory threads, got $loaded');
  }
}

Future<void> _acceptShutdownPhases(FlutterDriverClient client) async {
  await client.requestData('shutdown-begin');
  var sawOverlayWindow = false;
  for (var attempt = 0; attempt < 40; attempt += 1) {
    await Future<void>.delayed(const Duration(milliseconds: 50));
    final phases = _shutdownPhases(await _snapshot(client));
    if (phases.contains('flushingPersistence')) {
      sawOverlayWindow = true;
      final image = await client.screenshot();
      final output = File('build/session-paging-shutdown-overlay.png');
      await output.parent.create(recursive: true);
      await output.writeAsBytes(image);
      break;
    }
  }
  final completed = await client.requestData('shutdown-await');
  stdout.writeln('shutdown: $completed');
  final phases = _shutdownPhases(await _snapshot(client));
  final phaseEntries = _shutdownPhaseEntries(await _snapshot(client));
  stdout.writeln('shutdown phases: $phases');
  if (!sawOverlayWindow) {
    throw StateError('missed shutdown overlay window; phases=$phases');
  }
  if (!_phaseSequenceComplete(phases)) {
    throw StateError('shutdown phase sequence incomplete: $phases');
  }
  if (!phaseEntries.contains('flushingPersistence:0')) {
    throw StateError(
      'shutdown did not confirm persistence pending=0: $phaseEntries',
    );
  }
}

bool _phaseSequenceComplete(List<String> phases) {
  final deduped = <String>[];
  for (final phase in phases) {
    if (deduped.isEmpty || deduped.last != phase) {
      deduped.add(phase);
    }
  }
  if (deduped.length != expectedShutdownPhaseOrder.length) {
    return false;
  }
  for (var index = 0; index < deduped.length; index += 1) {
    if (deduped[index] != expectedShutdownPhaseOrder[index]) {
      return false;
    }
  }
  return true;
}

Future<Map<String, Object?>> _snapshot(FlutterDriverClient client) async {
  final raw = await client.requestData('snapshot');
  return jsonDecode(raw) as Map<String, Object?>;
}

Map<String, Object?> _sidebarDirectory(Map<String, Object?> snapshot) {
  return snapshot['sidebarDirectory'] as Map<String, Object?>? ?? const {};
}

List<String> _shutdownPhases(Map<String, Object?> snapshot) {
  return [
    for (final entry in _shutdownPhaseEntries(snapshot)) entry.split(':').first,
  ];
}

List<String> _shutdownPhaseEntries(Map<String, Object?> snapshot) {
  final phases = snapshot['shutdownPhases'];
  return phases is List
      ? [for (final entry in phases) entry.toString()]
      : const [];
}

Map<String, String> _parseArguments(List<String> args) {
  final parsed = <String, String>{};
  for (var index = 0; index < args.length; index += 1) {
    if (!args[index].startsWith('--') || index + 1 >= args.length) {
      continue;
    }
    parsed[args[index].substring(2)] = args[index + 1];
    index += 1;
  }
  return parsed;
}
