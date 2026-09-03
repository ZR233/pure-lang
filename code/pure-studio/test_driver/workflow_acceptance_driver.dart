//! Driver for the real, unified workflow acceptance flow.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';
import 'workflow_acceptance_evidence.dart';

Future<void> main(List<String> arguments) async {
  final options = _Options.parse(arguments);
  final snapshots = File(options.snapshotOutput);
  final evidence = WorkflowAcceptanceEvidence();
  await snapshots.parent.create(recursive: true);
  FlutterDriverSession? session;
  try {
    session = await FlutterDriverSession.connect(
      vmServiceUrl: options.vmServiceUrl,
      onReconnect: (event) => _append(snapshots, event.toJson()),
    );
    if (options.mode == 'new') {
      await _startNewWorkflow(session, options, snapshots, evidence);
    }
    var finalSnapshot = options.studioMode == 'mode.simple'
        ? await _waitForSimpleCompletion(session, options, snapshots, evidence)
        : await _waitForTerminal(
            session,
            options,
            snapshots,
            evidence,
            'completed',
          );
    if (options.mode == 'new') {
      finalSnapshot = await _verifyAndRenameThreadTitle(
        session,
        options,
        snapshots,
        evidence,
        finalSnapshot,
      );
    }
    final workflow = workflowFromSnapshot(finalSnapshot);
    if (options.studioMode == 'mode.simple') {
      if (workflow != null) {
        throw StateError(
          'mode.simple unexpectedly exposed a workflow: $workflow',
        );
      }
    } else {
      final run = workflow?['currentRun'];
      if (run is! Map<String, dynamic> ||
          run['currentStateId'] != 'completed' ||
          run['terminal'] != true) {
        throw StateError(
          'workflow did not reach completed terminal: $workflow',
        );
      }
      evidence.validateTaskFlow();
    }
    final timeline =
        ((finalSnapshot['workspace'] as Map?)?['timeline'] as List? ?? const [])
            .whereType<Map>()
            .map((row) => row['text'])
            .whereType<String>()
            .join('\n');
    if (!timeline.contains('PURE_WORKFLOW_GUI_VERIFY_OK') ||
        !timeline.contains('cargo test')) {
      throw StateError(
        'final response does not report the verifier marker and cargo test',
      );
    }
    await File('${options.snapshotOutput}.png')
        .writeAsBytes(await session.screenshot(), flush: true);
    await File('${options.snapshotOutput}.render-tree.txt')
        .writeAsString(await session.renderTree(), flush: true);
    final renderTree = await session.renderTree();
    for (final legacyKey in [
      'task-recovery',
      'work-unit',
      'delivery-review',
      'merge-record',
      'workflow-history-',
    ]) {
      if (renderTree.contains(legacyKey)) {
        throw StateError('legacy Task UI is still rendered: $legacyKey');
      }
    }
    stdout.writeln(
      jsonEncode({
        'result': 'completed',
        'mode': options.mode,
        'attempt': options.attempt,
        'workflow': workflow,
        'workspace': finalSnapshot['workspace'],
      }),
    );
    if (options.shutdownAfterCompletion) {
      final response = await session.requestData(
        'shutdown-await',
        timeout: const Duration(minutes: 2),
      );
      final decoded = jsonDecode(response);
      if (decoded is! Map<String, dynamic> ||
          decoded['shutdown'] != 'completed') {
        throw StateError('unexpected Studio shutdown response: $response');
      }
      stdout.writeln(
        jsonEncode({
          'event': 'studioShutdownCompleted',
          'attempt': options.attempt,
        }),
      );
    }
  } catch (error, stackTrace) {
    stderr.writeln('Workflow Driver failed: $error');
    stderr.writeln(stackTrace);
    if (session != null) {
      try {
        await _appendSnapshot(session, snapshots, 'failure');
        await File('${options.snapshotOutput}.failure.png')
            .writeAsBytes(await session.screenshot(), flush: true);
      } on Object {
        // Preserve the original driver failure.
      }
    }
    rethrow;
  } finally {
    await session?.close();
  }
}

Future<void> _startNewWorkflow(
  FlutterDriverSession session,
  _Options options,
  File snapshots,
  WorkflowAcceptanceEvidence evidence,
) async {
  final protectedFiles = await _protectedFileSnapshot(options.workspace!);
  await session.waitFor(
    find.byValueKey('studio-shell'),
    timeout: const Duration(minutes: 2),
  );
  await session.tap(find.byValueKey('sidebar-open-project'));
  await session.waitFor(find.byValueKey('project-path-dialog'));
  await session.tap(find.byValueKey('project-path-input'));
  await session.enterText(options.workspace!);
  await session.sendTextInputAction(
    TextInputAction.done,
    timeout: const Duration(seconds: 10),
  );
  await session.waitForAbsent(
    find.byValueKey('project-path-dialog'),
    timeout: const Duration(seconds: 30),
  );
  await session.waitFor(find.byValueKey('sidebar-new-session'));
  await session.tap(find.byValueKey('sidebar-new-session'));
  await session.tap(find.byValueKey('session-mode-selector'));
  await session.waitFor(find.byValueKey('session-mode-${options.studioMode}'));
  await session.tap(find.byValueKey('session-mode-${options.studioMode}'));
  await session.waitForNoPendingFrame(timeout: const Duration(seconds: 10));
  await _waitForSnapshot(
    session,
    snapshots,
    'mode-selected',
    (snapshot) =>
        (snapshot['workspace'] as Map?)?['threadMode'] == options.studioMode ||
        (snapshot['navigation'] as Map?)?['newThreadMode'] ==
            options.studioMode,
    timeout: const Duration(seconds: 30),
    evidence: evidence,
  );
  await session.tap(find.byValueKey('composer-input'));
  await session.enterText(await File(options.promptFile!).readAsString());
  await session.waitForNoPendingFrame(timeout: const Duration(seconds: 20));
  await session.tap(find.byValueKey('composer-submit'));
  final provisional = _provisionalTitle(
    await File(options.promptFile!).readAsString(),
  );
  await _waitForSnapshot(session, snapshots, 'provisional-title', (snapshot) {
    final workspace = snapshot['workspace'];
    final threadId = workspace is Map ? workspace['threadId'] : null;
    final directory = snapshot['sidebarDirectory'];
    final titles = directory is Map ? directory['titles'] : null;
    return threadId is String &&
        workspace is Map &&
        workspace['title'] == provisional &&
        provisional != 'New Session' &&
        titles is Map &&
        titles[threadId] == provisional;
  }, timeout: const Duration(seconds: 30));
  if (options.studioMode == 'mode.task') {
    await _waitForSnapshot(
      session,
      snapshots,
      'submitted',
      (snapshot) => workflowFromSnapshot(snapshot) != null,
      timeout: const Duration(minutes: 10),
      evidence: evidence,
    );
    await _driveClarificationAndPlanRevision(
      session,
      snapshots,
      options,
      protectedFiles,
      evidence,
    );
  } else {
    await _waitForSnapshot(
      session,
      snapshots,
      'submitted',
      (snapshot) =>
          workflowFromSnapshot(snapshot) == null &&
          (snapshot['workspace'] as Map?)?['turn'] != null,
      timeout: const Duration(minutes: 2),
      evidence: evidence,
    );
  }
}

Future<Map<String, dynamic>> _verifyAndRenameThreadTitle(
  FlutterDriverSession session,
  _Options options,
  File snapshots,
  WorkflowAcceptanceEvidence evidence,
  Map<String, dynamic> completed,
) async {
  final workspace = completed['workspace'];
  if (workspace is! Map<String, dynamic>) {
    throw StateError(
      'completed snapshot has no workspace for title acceptance',
    );
  }
  final threadId = workspace['threadId'];
  final title = workspace['title'];
  if (threadId is! String || title is! String) {
    throw StateError(
      'completed snapshot has no canonical Thread title: $workspace',
    );
  }
  final prompt = await File(options.promptFile!).readAsString();
  final provisional = _provisionalTitle(prompt);
  final generated = await _waitForSnapshot(
    session,
    snapshots,
    'automatic-title',
    (snapshot) {
      final nextWorkspace = snapshot['workspace'];
      final nextTitle = nextWorkspace is Map ? nextWorkspace['title'] : null;
      final normalizedTitle = nextTitle is String
          ? nextTitle.toLowerCase()
          : '';
      final describesFixture = const [
        'canonical',
        'key',
        'normaliz',
        'validat',
      ].any(normalizedTitle.contains);
      return nextWorkspace is Map &&
          nextWorkspace['threadId'] == threadId &&
          nextTitle is String &&
          nextTitle != 'New Session' &&
          nextTitle != provisional &&
          nextTitle.runes.length <= 36 &&
          describesFixture;
    },
    timeout: const Duration(seconds: 45),
    evidence: evidence,
  );
  final generatedWorkspace = generated['workspace'] as Map<String, dynamic>;
  final generatedTitle = generatedWorkspace['title'] as String;
  final generatedDirectory = generated['sidebarDirectory'];
  final generatedTitles = generatedDirectory is Map
      ? generatedDirectory['titles']
      : null;
  if (generatedTitles is! Map || generatedTitles[threadId] != generatedTitle) {
    throw StateError(
      'sidebar and workspace titles diverged: '
      '$generatedTitles vs $generatedTitle',
    );
  }
  await session.tap(find.byValueKey('thread-rename-$threadId'));
  await session.waitFor(find.byValueKey('thread-rename-dialog-$threadId'));
  await session.tap(find.byValueKey('thread-rename-input-$threadId'));
  await session.enterText('Driver manual title');
  await session.tap(find.byValueKey('thread-rename-save-$threadId'));
  final renamed = await _waitForSnapshot(
    session,
    snapshots,
    'manual-title',
    (snapshot) {
      final nextWorkspace = snapshot['workspace'];
      return nextWorkspace is Map &&
          nextWorkspace['threadId'] == threadId &&
          nextWorkspace['title'] == 'Driver manual title';
    },
    timeout: const Duration(seconds: 30),
    evidence: evidence,
  );
  final renamedDirectory = renamed['sidebarDirectory'];
  final renamedTitles = renamedDirectory is Map
      ? renamedDirectory['titles']
      : null;
  if (renamedTitles is! Map ||
      renamedTitles[threadId] != 'Driver manual title') {
    throw StateError('manual title did not reach the sidebar: $renamedTitles');
  }
  return renamed;
}

String _provisionalTitle(String prompt) {
  final normalized = prompt.trim().split(RegExp(r'\s+')).join(' ');
  if (normalized.isEmpty) return 'New Session';
  return String.fromCharCodes(normalized.runes.take(80));
}

Future<Map<String, dynamic>> _waitForSimpleCompletion(
  FlutterDriverSession session,
  _Options options,
  File snapshots,
  WorkflowAcceptanceEvidence evidence,
) async {
  final deadline = DateTime.now().add(options.workflowTimeout);
  final progress = _ProgressWatch();
  Map<String, dynamic>? last;
  while (DateTime.now().isBefore(deadline)) {
    last = await _appendSnapshot(
      session,
      snapshots,
      'simple-completed',
      evidence: evidence,
    );
    progress.observe(last, options.stallTimeout, 'simple completion');
    final workspace = last['workspace'] as Map?;
    final lastTurn = workspace?['lastTurn'] as Map?;
    if (workflowFromSnapshot(last) == null &&
        lastTurn?['status'] == 'completed' &&
        _hasSuccessfulComplete(last)) {
      return last;
    }
    final interaction = workspace?['activeInteraction'];
    if (interaction is Map && interaction['kind'] == 'toolApproval') {
      await session.tap(find.byValueKey('tool-approve'));
    } else if (interaction is Map && interaction['kind'] == 'userInput') {
      throw StateError('mode.simple requested unexpected user input');
    }
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw StateError('simple completion timed out; last=$last');
}

Future<void> _driveClarificationAndPlanRevision(
  FlutterDriverSession session,
  File snapshots,
  _Options options,
  Map<String, String?> protectedFiles,
  WorkflowAcceptanceEvidence evidence,
) async {
  final deadline = DateTime.now().add(options.workflowTimeout);
  final progress = _ProgressWatch();
  var clarificationAnswered = false;
  var revisionRequested = false;
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await _appendSnapshot(
      session,
      snapshots,
      'interaction',
      evidence: evidence,
    );
    progress.observe(snapshot, options.stallTimeout, 'clarification and plan');
    final workflow = workflowFromSnapshot(snapshot);
    final current = (workflow?['currentRun'] as Map?)?['currentStateId'];
    final interaction = snapshot['workspace'] is Map
        ? (snapshot['workspace'] as Map)['activeInteraction']
        : null;
    if (interaction is Map && interaction['kind'] == 'userInput') {
      await _assertProtectedFilesUnchanged(options.workspace!, protectedFiles);
      if (current == 'planning' && !clarificationAnswered) {
        await _answerClarification(session);
        clarificationAnswered = true;
        evidence.recordClarificationAnswer();
        await _waitForInteractionChange(
          session,
          snapshots,
          interaction['id'],
          evidence,
        );
        continue;
      }
      if (current == 'planning' && !revisionRequested) {
        _assertPlanInteraction(interaction);
        await _requestPlanRevision(session);
        revisionRequested = true;
        evidence.recordPlanRevisionRequest();
        await _waitForInteractionChange(
          session,
          snapshots,
          interaction['id'],
          evidence,
        );
        continue;
      }
      if (current == 'planning' && revisionRequested) {
        _assertRevisedPlanInteraction(interaction);
        await _approvePlan(session);
        evidence.recordRevisedPlanApproval();
        await _waitForSnapshot(
          session,
          snapshots,
          'revised-plan-approved',
          (next) =>
              (workflowFromSnapshot(next)?['currentRun']
                  as Map?)?['currentStateId'] ==
              'editing_documents',
          timeout: const Duration(minutes: 3),
          evidence: evidence,
        );
        return;
      }
      throw StateError(
        'unexpected user interaction at state $current: $interaction',
      );
    }
    if ((current == 'editing_documents' || current == 'working') &&
        !evidence.hasRevisedPlanApproval) {
      throw StateError('workflow skipped revised plan confirmation');
    }
    if (interaction is Map && interaction['kind'] == 'toolApproval') {
      await session.tap(find.byValueKey('tool-approve'));
    }
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw StateError(
    'workflow never completed clarification and revised-plan approval',
  );
}

Future<Map<String, dynamic>> _waitForTerminal(
  FlutterDriverSession session,
  _Options options,
  File snapshots,
  WorkflowAcceptanceEvidence evidence,
  String label,
) async {
  final deadline = DateTime.now().add(options.workflowTimeout);
  final progress = _ProgressWatch();
  Map<String, dynamic>? last;
  while (DateTime.now().isBefore(deadline)) {
    last = await _appendSnapshot(session, snapshots, label, evidence: evidence);
    progress.observe(last, options.stallTimeout, label);
    final workflow = workflowFromSnapshot(last);
    final run = workflow?['currentRun'];
    if (run is Map<String, dynamic> &&
        run['terminal'] == true &&
        run['currentStateId'] == 'completed' &&
        _hasSuccessfulComplete(last)) {
      return last;
    }
    final interaction = (last['workspace'] as Map?)?['activeInteraction'];
    if (interaction is Map && interaction['kind'] == 'toolApproval') {
      await session.tap(find.byValueKey('tool-approve'));
    } else if (interaction is Map && interaction['kind'] == 'userInput') {
      throw StateError(
        'workflow requested unexpected input after plan confirmation',
      );
    }
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw StateError('$label timed out; last=$last');
}

bool _hasSuccessfulComplete(Map<String, dynamic> snapshot) {
  final timeline =
      ((snapshot['workspace'] as Map?)?['timeline'] as List? ?? const [])
          .whereType<Map>();
  for (final row in timeline) {
    final tools = row['tools'];
    if (tools is! List) continue;
    for (final tool in tools.whereType<Map>()) {
      if (tool['name'] == 'complete' && tool['status'] == 'succeeded') {
        return true;
      }
    }
  }
  return false;
}

Future<Map<String, String?>> _protectedFileSnapshot(String workspace) async {
  final result = <String, String?>{};
  for (final relative in [
    'design/task-workflows.md',
    'src/normalize.rs',
    'src/validate.rs',
    'tests/normalize.rs',
    'tests/validate.rs',
  ]) {
    final file = File('$workspace/$relative');
    result[relative] = await file.exists()
        ? base64Encode(await file.readAsBytes())
        : null;
  }
  return result;
}

Future<void> _assertProtectedFilesUnchanged(
  String workspace,
  Map<String, String?> expected,
) async {
  final actual = await _protectedFileSnapshot(workspace);
  if (!_mapEquals(expected, actual)) {
    throw StateError('implementation files changed before plan confirmation');
  }
}

bool _mapEquals(Map<String, String?> left, Map<String, String?> right) {
  if (left.length != right.length) return false;
  return left.entries.every((entry) => right[entry.key] == entry.value);
}

void _assertPlanInteraction(Map interaction) {
  final body = interaction['body'];
  if (body is! String || !body.trimLeft().startsWith('# ')) {
    throw StateError('Plan confirmation has no complete Markdown plan');
  }
}

void _assertRevisedPlanInteraction(Map interaction) {
  _assertPlanInteraction(interaction);
  final body = interaction['body'] as String;
  for (final marker in ['并行', '目录', '隔离', '验证']) {
    if (!body.toLowerCase().contains(marker.toLowerCase())) {
      throw StateError(
        'revised plan omitted requested marker `$marker`: $body',
      );
    }
  }
}

Future<void> _answerClarification(FlutterDriverSession session) async {
  const answer =
      '按第一个非法 Unicode scalar 在原始 UTF-8 输入中的起始 byte index 报告。'
      '例如 "aα" 中 α 报 index=1、byte=0xCE；'
      '"éα" 必须先报 é：index=0、byte=0xC3。';
  if (await _isVisible(session, 'user-input-first-option')) {
    await session.tap(find.byValueKey('user-input-first-option'));
  }
  if (await _isVisible(session, 'user-input-first-text')) {
    await session.tap(find.byValueKey('user-input-first-text'));
    await session.enterText(answer);
  } else if (!await _isVisible(session, 'user-input-first-option')) {
    final input = find.byValueKey('fallback-user-input');
    await session.tap(input);
    await session.enterText(answer);
    await session.tap(find.byValueKey('fallback-user-input-submit'));
    return;
  }
  await session.tap(find.byValueKey('user-input-submit'));
}

Future<void> _requestPlanRevision(FlutterDriverSession session) async {
  const revisionOption = 'user-input-option-plan_confirmation-1';
  await session.waitFor(
    find.byValueKey(revisionOption),
    timeout: const Duration(seconds: 20),
  );
  await session.tap(find.byValueKey(revisionOption));
  await session.waitFor(find.byValueKey('user-input-first-text'));
  await session.tap(find.byValueKey('user-input-first-text'));
  await session.enterText('请补充多目录隔离并行执行、结果整合和验证顺序的具体步骤。');
  await session.tap(find.byValueKey('user-input-submit'));
}

Future<void> _approvePlan(FlutterDriverSession session) async {
  await session.waitFor(
    find.byValueKey('user-input-first-option'),
    timeout: const Duration(seconds: 20),
  );
  await session.tap(find.byValueKey('user-input-first-option'));
  await session.tap(find.byValueKey('user-input-submit'));
}

Future<void> _waitForInteractionChange(
  FlutterDriverSession session,
  File snapshots,
  Object? interactionId,
  WorkflowAcceptanceEvidence evidence,
) async {
  await _waitForSnapshot(
    session,
    snapshots,
    'interaction-resolved',
    (snapshot) {
      final active = (snapshot['workspace'] as Map?)?['activeInteraction'];
      return active is! Map || active['id'] != interactionId;
    },
    timeout: const Duration(minutes: 2),
    evidence: evidence,
  );
}

Future<bool> _isVisible(FlutterDriverSession session, String key) async {
  try {
    await session.waitFor(
      find.byValueKey(key),
      timeout: const Duration(seconds: 5),
    );
    return true;
  } on Object {
    return false;
  }
}

Future<Map<String, dynamic>> _waitForSnapshot(
  FlutterDriverSession session,
  File output,
  String label,
  bool Function(Map<String, dynamic>) predicate, {
  Duration timeout = const Duration(seconds: 30),
  WorkflowAcceptanceEvidence? evidence,
}) async {
  final deadline = DateTime.now().add(timeout);
  Map<String, dynamic>? last;
  while (DateTime.now().isBefore(deadline)) {
    last = await _appendSnapshot(session, output, label, evidence: evidence);
    if (predicate(last)) return last;
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw StateError('$label timed out; last=$last');
}

Future<Map<String, dynamic>> _appendSnapshot(
  FlutterDriverSession session,
  File output,
  String label, {
  WorkflowAcceptanceEvidence? evidence,
}) async {
  final snapshot = await session.readSnapshot();
  evidence?.observe(snapshot);
  await _append(output, {'stage': label, 'snapshot': snapshot});
  return snapshot;
}

Future<void> _append(File output, Object value) => output.writeAsString(
  '${jsonEncode(value)}\n',
  mode: FileMode.append,
  flush: true,
);

class _Options {
  _Options({
    required this.vmServiceUrl,
    required this.mode,
    required this.studioMode,
    required this.workspace,
    required this.promptFile,
    required this.snapshotOutput,
    required this.attempt,
    required this.shutdownAfterCompletion,
    required this.workflowTimeout,
    required this.stallTimeout,
  });

  final String vmServiceUrl;
  final String mode;
  final String studioMode;
  final String? workspace;
  final String? promptFile;
  final String snapshotOutput;
  final int attempt;
  final bool shutdownAfterCompletion;
  final Duration workflowTimeout;
  final Duration stallTimeout;

  static _Options parse(List<String> args) {
    String? value(String name) {
      final index = args.indexOf(name);
      return index < 0 || index + 1 >= args.length ? null : args[index + 1];
    }

    final vm = value('--vm-service-url');
    final mode = value('--mode') ?? 'new';
    final studioMode = value('--studio-mode') ?? 'mode.task';
    if (studioMode != 'mode.simple' && studioMode != 'mode.task') {
      throw ArgumentError('--studio-mode must be mode.simple or mode.task');
    }
    final output = value('--snapshot-output');
    if (vm == null || output == null) {
      throw ArgumentError(
        '--vm-service-url and --snapshot-output are required',
      );
    }
    final workspace = value('--workspace');
    final prompt = value('--prompt-file');
    if (mode == 'new' && (workspace == null || prompt == null)) {
      throw ArgumentError(
        '--workspace and --prompt-file are required for new mode',
      );
    }
    return _Options(
      vmServiceUrl: vm,
      mode: mode,
      studioMode: studioMode,
      workspace: workspace,
      promptFile: prompt,
      snapshotOutput: output,
      attempt: int.tryParse(value('--attempt') ?? '1') ?? 1,
      shutdownAfterCompletion: value('--shutdown-after-completion') != 'false',
      workflowTimeout: Duration(
        seconds:
            int.tryParse(value('--workflow-timeout-seconds') ?? '1800') ?? 1800,
      ),
      stallTimeout: Duration(
        seconds: int.tryParse(value('--stall-timeout-seconds') ?? '600') ?? 600,
      ),
    );
  }
}

class _ProgressWatch {
  String? _fingerprint;
  DateTime _changedAt = DateTime.now();

  void observe(
    Map<String, dynamic> snapshot,
    Duration stallTimeout,
    String label,
  ) {
    final workspace = snapshot['workspace'] as Map?;
    final timeline = workspace?['timeline'] as List? ?? const [];
    final fingerprint = jsonEncode({
      'workflow': snapshot['workflow'],
      'interaction': workspace?['activeInteraction'],
      'timeline': timeline,
    });
    if (_fingerprint != fingerprint) {
      _fingerprint = fingerprint;
      _changedAt = DateTime.now();
      return;
    }
    if (DateTime.now().difference(_changedAt) >= stallTimeout) {
      throw StateError(
        '$label made no model, tool, state, or GUI progress for '
        '${stallTimeout.inMinutes} minutes',
      );
    }
  }
}
