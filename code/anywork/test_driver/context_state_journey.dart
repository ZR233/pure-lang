import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';
import 'realtime_journey.dart';

/// 真实供应商的原生界面验收。只通过界面实施，快照和文件仅用于观察结果。
Future<void> main(List<String> args) async {
  if (args.length != 5) {
    throw ArgumentError('first|restart VM_URL PROJECT OUTPUT MARKER required');
  }
  final journey = ContextStateJourney(
    await FlutterDriverSession.connect(vmServiceUrl: args[1]),
    args,
  );
  Object? failure;
  StackTrace? failureStack;
  try {
    await journey.run();
  } catch (error, stack) {
    failure = error;
    failureStack = stack;
    try {
      await journey.capture('failure');
    } catch (_) {
      // 保留原始失败，不用取证失败替代。
    }
  }
  var shutdown = 'failed';
  try {
    final reply = jsonDecode(
      await journey.driver.requestData(
        'shutdown',
        timeout: const Duration(seconds: 60),
      ),
    );
    if (reply is! Map || reply['shutdown'] != 'completed') {
      throw StateError('原生程序未完成关闭');
    }
    shutdown = 'completed';
    await File('${args[3]}/${args[0]}-shutdown.json')
        .writeAsString(jsonEncode(reply));
    final exit = jsonDecode(
      await journey.driver.requestData('request-app-exit'),
    );
    if (exit is! Map || exit['exit'] != 'exit_requested') {
      throw StateError('原生程序未受理退出请求');
    }
  } catch (error, stack) {
    failure ??= error;
    failureStack ??= stack;
  } finally {
    try {
      await journey.driver.close().timeout(const Duration(seconds: 5));
    } catch (_) {
      // 原生关闭后连接终止不覆盖已收集的验收结果。
    }
  }
  await File('${args[3]}/${args[0]}-summary.json').writeAsString(
    const JsonEncoder.withIndent('  ').convert({
      'phase': args[0],
      'status': failure == null ? 'complete' : 'failed',
      'shutdown': shutdown,
      'error': failure?.toString(),
      'humanVerdict': 'pending',
      'checks': journey.checks,
    }),
  );
  if (failure != null) Error.throwWithStackTrace(failure, failureStack!);
}

class ContextStateJourney {
  ContextStateJourney(this.driver, List<String> args)
    : phase = args[0],
      project = args[2],
      output = args[3],
      marker = args[4];

  final FlutterDriverSession driver;
  final String phase;
  final String project;
  final String output;
  final String marker;
  final Map<String, bool> checks = {};
  Set<String> previousAnswerIds = {};
  String? submittedInputId;

  File get artifact => File('$project/acceptance-state.txt');
  File get observed => File('$output/observed.json');

  Future<void> tap(String key) async {
    final finder = find.byValueKey(key);
    await driver.waitFor(finder, timeout: const Duration(seconds: 45));
    await driver.rawTap(finder);
  }

  Future<void> submit(String prompt) async {
    final submittedPrompt =
        '$prompt\n本次观察编号：$phase-${DateTime.now().microsecondsSinceEpoch}';
    previousAnswerIds = timelineRows(await driver.readSnapshot())
        .map((row) => '${row['id']}')
        .toSet();
    await tap('composer-input');
    await driver.enterText(submittedPrompt);
    await tap('composer-submit');
    final accepted = await waitFor(
      (snapshot) =>
          (workspaceOf(snapshot)?['composer'] as Map?)?['submissionPending'] ==
              false &&
          timelineRows(snapshot).any(
            (row) =>
                row['type'] == 'userMessage' &&
                !previousAnswerIds.contains('${row['id']}') &&
                row['text'] == submittedPrompt,
          ),
      '本轮输入已受理',
    );
    submittedInputId =
        timelineRows(accepted).lastWhere(
              (row) =>
                  row['type'] == 'userMessage' &&
                  !previousAnswerIds.contains('${row['id']}') &&
                  row['text'] == submittedPrompt,
            )['id']
            as String;
  }

  bool settled(Map<String, dynamic> snapshot) {
    final workspace = workspaceOf(snapshot);
    final persistence = snapshot['persistence'];
    return workspace != null &&
        workspace['isBusy'] == false &&
        (workspace['composer'] as Map?)?['submissionPending'] == false &&
        workspace['syncState'] == 'ready' &&
        persistence is Map &&
        persistence['kind'] == 'ready' &&
        persistence['pendingCommits'] == 0;
  }

  Future<Map<String, dynamic>> waitFor(
    FutureOr<bool> Function(Map<String, dynamic>) predicate,
    String label,
  ) async {
    final deadline = DateTime.now().add(const Duration(minutes: 12));
    Map<String, dynamic>? last;
    while (DateTime.now().isBefore(deadline)) {
      last = await driver.readSnapshot();
      if (await predicate(last)) return last;
      await Future<void>.delayed(const Duration(milliseconds: 350));
    }
    throw StateError('$label 超时：${jsonEncode(summarize(last ?? {}))}');
  }

  Map<String, dynamic>? runOf(Map<String, dynamic> snapshot) {
    final run = (snapshot['workflow'] as Map?)?['currentRun'];
    return run is Map<String, dynamic> ? run : null;
  }

  Map<String, dynamic>? activeInteractionOf(Map<String, dynamic> snapshot) {
    final interaction = workspaceOf(snapshot)?['activeInteraction'];
    return interaction is Map<String, dynamic> ? interaction : null;
  }

  Future<bool> hasArtifact(String expected) async =>
      await artifact.exists() &&
      (await artifact.readAsString()).contains(expected);

  Future<void> capture(String label) async {
    await File('$output/$phase-$label-snapshot.json')
        .writeAsString(jsonEncode(await driver.readSnapshot()));
    await File('$output/$phase-$label-tree.txt')
        .writeAsString(await driver.renderTree());
    await File('$output/$phase-$label.png')
        .writeAsBytes(await driver.screenshot());
  }

  void require(String label, bool condition) {
    checks[label] = condition;
    if (!condition) throw StateError(label);
  }

  Future<void> openProject() async {
    await tap('sidebar-open-project');
    await tap('add-project-local');
    await tap('add-project-continue-ready');
    await tap('project-path-input');
    await driver.enterText(project);
    await tap('project-path-submit');
  }

  Future<Map<String, dynamic>> answerWith(String token) => waitFor(
    (snapshot) =>
        settled(snapshot) &&
        currentInputCompleted(snapshot) &&
        timelineRows(snapshot).any(
          (row) =>
              row['type'] == 'finalAnswer' &&
              !previousAnswerIds.contains('${row['id']}') &&
              '${row['text']}'.contains(token),
        ),
    '完成隔离观察 $token',
  );

  bool currentInputCompleted(Map<String, dynamic> snapshot) {
    final lastTurn = workspaceOf(snapshot)?['lastTurn'] as Map?;
    return submittedInputId != null &&
        lastTurn?['inputId'] == submittedInputId &&
        lastTurn?['status'] == 'completed';
  }

  Future<Map<String, dynamic>> reopen(String threadId) async {
    await tap('thread-row-$threadId');
    return waitFor(
      (snapshot) =>
          workspaceOf(snapshot)?['threadId'] == threadId && settled(snapshot),
      '恢复会话 $threadId',
    );
  }

  Future<void> isolation() async {
    final savedFile = File('$output/isolation-observed.json');
    if (phase == 'isolation-first') {
      await openProject();
      final roots = <String, String>{};
      for (final label in ['ROOT_A', 'ROOT_B']) {
        if (roots.isNotEmpty) await tap('sidebar-new-session');
        await submit(
          '仅进行只读会话隔离观察，不需要实施计划。'
          '本会话唯一私有标记为 $marker-$label。只在 final 中原样回复该标记，'
          '不查询其他会话，不调用工具，不派发子代理。',
        );
        final first = await answerWith('$marker-$label');
        roots[label] = workspaceOf(first)!['threadId'] as String;
        await submit('请依据当前本会话历史原样回复唯一私有标记，不执行工具。');
        await answerWith('$marker-$label');
        await capture(label);
      }
      await tap('sidebar-new-session');
      await submit(
        '进行只读代理上下文隔离观察，不需要实施计划，不修改文件。'
        '父会话私有标记为 $marker-PARENT_PRIVATE，禁止将此标记传给子代理。'
        '请选择只读 explorer 配置派发两个独立子代理：第一个 message 仅为'
        '“只读合成任务：唯一标记 $marker-CHILD_A。只在 final 原样回复标记，不调用工具。”；'
        '第二个 message 仅为“只读合成任务：唯一标记 $marker-CHILD_B。只在 final 原样回复标记，不调用工具。”。'
        '不要传递父会话历史或其他标记。接收两份显式报告后，在 final 汇总两个子标记并结束本轮。',
      );
      final parent = await answerWith('$marker-CHILD_B');
      final root = workspaceOf(parent)!['threadId'] as String;
      final agents = workspaceOf(parent)!['agents'] as List;
      final children = agents
          .whereType<Map>()
          .where((agent) => agent['id'] != root)
          .map((agent) => agent['id'] as String)
          .toList();
      require(
        '父与两个子代理具有独立身份',
        children.length == 2 && children.toSet().length == 2,
      );
      require(
        '两个根会话身份独立',
        roots.values.toSet().length == 2 && !roots.values.contains(root),
      );
      await capture('parent-delivery');
      await savedFile.writeAsString(
        jsonEncode({'roots': roots, 'parent': root, 'children': children}),
      );
    } else if (phase.startsWith('isolation-restart')) {
      final saved = jsonDecode(await savedFile.readAsString()) as Map;
      for (final entry in (saved['roots'] as Map).entries) {
        await reopen(entry.value as String);
        await submit('只观察恢复后的本会话上下文：原样回复本会话唯一私有标记，不查询其他会话，不调用工具。');
        await answerWith('$marker-${entry.key}');
        await capture('restored-${entry.key}');
      }
      await reopen(saved['parent'] as String);
      await submit(
        '只读恢复观察：向原子代理 ${jsonEncode(saved['children'])} 分别发送'
        '“只在 final 回复原派发任务中的唯一标记，不调用工具。”。'
        '不附加父会话历史或其他标记，接收显式报告后汇总原两个子标记并结束本轮。',
      );
      await answerWith('$marker-CHILD_B');
      await capture('restored-children-delivery');
      require('恢复后的独立根与子代理均可继续', true);
    } else {
      throw ArgumentError('未知隔离阶段 $phase');
    }
  }

  Future<void> run() async {
    if (phase == 'auxiliary-trace') {
      await openProject();
      await submit('只读上下文取证：仅在 final 回复 $marker，不调用工具，不需要实施计划。');
      await answerWith(marker);
      await capture('answered');
      return;
    }
    if (phase.startsWith('isolation')) {
      await isolation();
    } else if (phase == 'first' || phase.startsWith('approve-restored')) {
      if (phase.startsWith('approve-restored')) {
        final saved = jsonDecode(
          await File('$output/first-failure-snapshot.json').readAsString(),
        ) as Map<String, dynamic>;
        await tap('thread-row-${workspaceOf(saved)?['threadId']}');
      }
      final initial = await driver.readSnapshot();
      if (workspaceOf(initial)?['threadId'] == null) {
        await tap('sidebar-open-project');
        await tap('add-project-local');
        await tap('add-project-continue-ready');
        await tap('project-path-input');
        await driver.enterText(project);
        await tap('project-path-submit');
        await tap('session-mode-selector');
        await tap('session-mode-mode.task');
        await submit(
          '执行合成上下文验收，唯一标记为 $marker。这是两步小任务，不派发子代理，'
          '不查询会话之外的材料，也不修改已有文件。先用 plan_submit 提交完整计划：'
          '第一步在用户界面批准后从 planning 经 editing_documents 进入 working，'
          '直接创建 acceptance-state.txt，写入唯一一行 $marker-FIRST；'
          '第二步等待我发送 NEXT 后在同一文件追加唯一一行 $marker-SECOND。'
          '计划正文必须包含两个完整标记和等待条件。本任务不改变架构，无设计文档需要更新。'
          '批准后先完成第一步，保持 working 阶段，用 final 或 finish_turn 结束本轮等待 NEXT，不调用 wait 等待用户。'
          '不要提前完成第二步或进入 integrating，不因上下文压缩重复请求批准。',
        );
      }
      final plan = await waitFor(
        (snapshot) => activeInteractionOf(snapshot)?['kind'] == 'userInput',
        '完整计划确认',
      );
      final body = activeInteractionOf(plan)?['body'] as String? ?? '';
      require(
        '计划包含两个完整标记',
        body.contains('$marker-FIRST') && body.contains('$marker-SECOND'),
      );
      try {
        await driver.waitFor(
          find.byValueKey('plan-details'),
          timeout: const Duration(seconds: 2),
        );
      } on DriverError {
        await tap('plan-summary');
      }
      await capture('approved-plan-before-answer');
      await tap('plan-details-close');
      await tap('plan-approve');
      final first = await waitFor(
        (snapshot) async =>
            settled(snapshot) && await hasArtifact('$marker-FIRST'),
        '批准后实施第一步',
      );
      require('批准后没有新确认', activeInteractionOf(first) == null);
      require('工作流处于实施阶段', runOf(first)?['currentStateId'] == 'working');
      await capture('first-implemented');
      await submit(
        'NEXT：在已批准范围内继续第二步，追加 $marker-SECOND 恰好一次。'
        '本次验收仍保留 working 阶段，结束本轮等待重启后的观察；不要重复批准。',
      );
      final second = await waitFor(
        (snapshot) async =>
            settled(snapshot) && await hasArtifact('$marker-SECOND'),
        '压缩后实施第二步',
      );
      require('第二步没有新确认', activeInteractionOf(second) == null);
      require('第二步保持实施阶段', runOf(second)?['currentStateId'] == 'working');
      final lines = await artifact.readAsLines();
      require(
        '两步产物各执行一次',
        lines.where((line) => line.trim() == '$marker-FIRST').length == 1 &&
            lines.where((line) => line.trim() == '$marker-SECOND').length == 1,
      );
      await capture('second-implemented');
      await observed.writeAsString(
        jsonEncode({
          'threadId': workspaceOf(second)?['threadId'],
          'run': runOf(second),
          'plan': body,
          'artifact': await artifact.readAsString(),
        }),
      );
    } else if (phase.startsWith('restart')) {
      final saved = jsonDecode(await observed.readAsString()) as Map;
      final reopened = await reopen(saved['threadId'] as String);
      require(
        '重启保持同一工作流',
        runOf(reopened)?['runId'] == saved['run']['runId'] &&
            runOf(reopened)?['currentStateId'] == 'working',
      );
      require('重启没有新确认', activeInteractionOf(reopened) == null);
      await capture('reopened');
      await submit(
        '仅观察恢复状态：依据最新宿主计划和工作流投影，回复当前批准状态、'
        '阶段及计划中的两个完整标记，不执行工具、不重新批准、不转换阶段。',
      );
      final continued = await waitFor(
        (snapshot) =>
            settled(snapshot) &&
            currentInputCompleted(snapshot) &&
            timelineRows(snapshot).any(
              (row) =>
                  row['type'] == 'finalAnswer' &&
                  !previousAnswerIds.contains('${row['id']}') &&
                  '${row['text']}'.contains('$marker-SECOND'),
            ),
        '重启后继续观察',
      );
      require('恢复后阶段未回退', runOf(continued)?['currentStateId'] == 'working');
      require('恢复后没有新确认', activeInteractionOf(continued) == null);
      require('恢复后没有重复执行', await artifact.readAsString() == saved['artifact']);
      await capture('continued');
    } else {
      throw ArgumentError('未知阶段 $phase');
    }
  }
}
