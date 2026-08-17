// Timeline 顺序回归验收（真实 runtime）：
//   1. 打开临时项目 → 新会话（Simple 模式）；
//   2. 连发 3 条真实消息，等待每条 Turn 完成；
//   3. 断言 snapshot 中用户消息按提交顺序追加在 timeline 末尾
//      （修复前：用户输入全部落到最开头）；
//   4. 新建第二个会话并与其往复切换，断言选中内容与顺序稳定。
//
// 用法：dart run test_driver/order_verify_driver.dart --vm-service-url <url> --workspace <path>

import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

Future<void> main(List<String> args) async {
  final arguments = _parseArguments(args);
  final vmServiceUrl = arguments['vm-service-url'];
  final workspace = arguments['workspace'];
  if (vmServiceUrl == null || workspace == null) {
    stderr.writeln(
      'usage: order_verify_driver.dart --vm-service-url <url> --workspace <path>',
    );
    exitCode = 2;
    return;
  }
  final client = await connectFlutterDriverClient(vmServiceUrl);
  await client.checkHealth();

  await _openProject(client, workspace);
  final messages = ['第一条：hello', '第二条：world', '第三条：pure studio'];
  final first = await _newSession(client, messages.first);
  for (final message in messages.skip(1)) {
    await _submitAndWaitTurn(client, message);
  }
  final snapshot1 = await _snapshot(client);
  _assertUserMessagesOrdered(snapshot1, messages, first);

  final second = await _newSession(client, '第二个会话的消息');
  // 往复切换：second → first → second。
  await _switchSession(client, first);
  await _switchSession(client, second);
  await _switchSession(client, first);
  final snapshot2 = await _snapshot(client);
  _assertUserMessagesOrdered(snapshot2, messages, first);

  await client.close();
  stdout.writeln('ORDER VERIFY DRIVER: ok');
}

Future<void> _openProject(FlutterDriverClient client, String workspace) async {
  await client.waitFor(
    find.byValueKey('studio-shell'),
    timeout: const Duration(minutes: 2),
  );
  await client.tap(find.byValueKey('sidebar-open-project'));
  await client.waitFor(find.byValueKey('project-path-dialog'));
  await client.tap(find.byValueKey('project-path-input'));
  await client.enterText(workspace);
  await client.sendTextInputAction(TextInputAction.done);
  await client.waitForAbsent(
    find.byValueKey('project-path-dialog'),
    timeout: const Duration(seconds: 30),
  );
}

Future<String> _newSession(
  FlutterDriverClient client,
  String firstMessage,
) async {
  await client.waitFor(find.byValueKey('sidebar-new-session'));
  await client.tap(find.byValueKey('sidebar-new-session'));
  await client.waitFor(find.byValueKey('studio-start-page'));
  await client.waitFor(find.byValueKey('composer-input'));
  final transient = await _snapshot(client);
  if (transient['workspace'] != null) {
    throw StateError('new session was persisted before its first message');
  }
  await _submitAndWaitTurn(client, firstMessage);
  final snapshot = await _snapshot(client);
  final workspaceView = snapshot['workspace'];
  if (workspaceView is! Map<String, dynamic>) {
    throw StateError('no workspace after first message');
  }
  return workspaceView['threadId'] as String;
}

Future<void> _switchSession(FlutterDriverClient client, String threadId) async {
  await client.waitFor(find.byValueKey('thread-row-$threadId'));
  await client.tap(find.byValueKey('thread-row-$threadId'));
  final deadline = DateTime.now().add(const Duration(seconds: 30));
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await _snapshot(client);
    final workspaceView = snapshot['workspace'];
    if (workspaceView is Map<String, dynamic> &&
        workspaceView['threadId'] == threadId) {
      return;
    }
    await Future<void>.delayed(const Duration(milliseconds: 200));
  }
  throw StateError('switch to $threadId did not take effect');
}

Future<void> _submitAndWaitTurn(
  FlutterDriverClient client,
  String message,
) async {
  await client.waitFor(find.byValueKey('composer-input'));
  await client.tap(find.byValueKey('composer-input'));
  await client.enterText(message);
  await client.waitForNoPendingFrame(timeout: const Duration(seconds: 15));
  await client.tap(find.byValueKey('composer-submit'));
  // 等待 turn 终态（composer 恢复空闲）。
  final deadline = DateTime.now().add(const Duration(minutes: 6));
  while (DateTime.now().isBefore(deadline)) {
    await Future<void>.delayed(const Duration(milliseconds: 500));
    final snapshot = await _snapshot(client);
    final workspaceView = snapshot['workspace'];
    if (workspaceView is! Map<String, dynamic>) continue;
    final turn = workspaceView['turn'];
    if (turn == null) return;
  }
  throw StateError('turn for "$message" did not settle within 6 minutes');
}

void _assertUserMessagesOrdered(
  Map<String, dynamic> snapshot,
  List<String> messages,
  String threadId,
) {
  final workspaceView = snapshot['workspace'];
  if (workspaceView is! Map<String, dynamic>) {
    throw StateError('no workspace in snapshot');
  }
  if (workspaceView['threadId'] != threadId) {
    throw StateError(
      'selected ${workspaceView['threadId']} but expected $threadId',
    );
  }
  final progress = workspaceView['timelineProgress'];
  if (progress is! Map<String, dynamic>) {
    throw StateError('workspace has no timelineProgress');
  }
  final rows = progress['rows'];
  if (rows is! List) {
    throw StateError('timelineProgress has no rows');
  }
  final userTexts = [
    for (final row in rows)
      if (row is Map<String, dynamic> && row['type'] == 'userMessage')
        row['text'] as String? ?? '',
  ];
  final trailing = userTexts.take(messages.length).toList();
  for (var index = 0; index < messages.length; index += 1) {
    final expected = messages[index];
    if (index >= trailing.length || trailing[index] != expected) {
      throw StateError(
        'user messages out of order: expected $expected at tail position '
        '$index, got ${trailing.length > index ? trailing[index] : '<none>'} '
        '(all=$userTexts rows=$rows)',
      );
    }
  }
  stdout.writeln('ordered user messages at tail: $trailing');
}

Future<Map<String, dynamic>> _snapshot(FlutterDriverClient client) async {
  final raw = await client.requestData('snapshot');
  final decoded = jsonDecode(raw) as Map<String, dynamic>;
  if (decoded['workspace'] is! Map<String, dynamic>) {
    // Driver 快照尚未发布 workspace：等待后重试一次。
    await Future<void>.delayed(const Duration(milliseconds: 300));
    final retry = await client.requestData('snapshot');
    return jsonDecode(retry) as Map<String, dynamic>;
  }
  return decoded;
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
