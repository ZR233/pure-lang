//! Flutter Driver acceptance for real directory and worktree child Agents.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

Future<void> main(List<String> arguments) async {
  final options = _Options.parse(arguments);
  FlutterDriverSession? session;
  try {
    session = await FlutterDriverSession.connect(
      vmServiceUrl: options.vmServiceUrl,
    );
    await session.waitFor(
      find.byValueKey('studio-shell'),
      timeout: const Duration(minutes: 2),
    );
    await _configureAgents(session, options);
    await File(options.settingsScreenshot)
        .writeAsBytes(await session.screenshot(), flush: true);
    await File('${options.settingsScreenshot}.render-tree.txt')
        .writeAsString(await session.renderTree(), flush: true);
    await session.tap(find.byValueKey('settings-back'));
    await _openProjectAndSubmit(session, options);
    final snapshot = await _waitForCompletion(session, options);
    _validateSnapshot(snapshot);
    await File(options.finalScreenshot)
        .writeAsBytes(await session.screenshot(), flush: true);
    await File('${options.finalScreenshot}.render-tree.txt')
        .writeAsString(await session.renderTree(), flush: true);
    stdout.writeln(
      jsonEncode({
        'result': 'completed',
        'workspace': snapshot['workspace'],
        'workflow': snapshot['workflow'],
      }),
    );
    final response = await session.requestData(
      'shutdown-await',
      timeout: const Duration(minutes: 2),
    );
    final decoded = jsonDecode(response);
    if (decoded is! Map<String, dynamic> ||
        decoded['shutdown'] != 'completed') {
      throw StateError('unexpected Studio shutdown response: $response');
    }
    stdout.writeln(jsonEncode({'event': 'studioShutdownCompleted'}));
  } catch (error, stackTrace) {
    stderr.writeln('Subagents Driver failed: $error');
    stderr.writeln(stackTrace);
    if (session != null) {
      try {
        await File('${options.finalScreenshot}.failure.png')
            .writeAsBytes(await session.screenshot(), flush: true);
      } on Object {
        // Preserve the original acceptance failure.
      }
    }
    rethrow;
  } finally {
    await session?.close();
  }
}

Future<void> _configureAgents(
  FlutterDriverSession session,
  _Options options,
) async {
  await session.tap(find.byValueKey('settings-open'));
  await session.waitFor(find.byValueKey('settings-page'));
  await session.tap(find.byValueKey('settings-tab-agents'));
  for (final route in [
    options.explorer,
    options.executor,
    options.worktreeExecutor,
    options.reviewer,
  ]) {
    final model = find.byValueKey('settings-role-${route.role}-model');
    await session.scrollUntilVisible(
      find.byValueKey('settings-pane-scroll'),
      model,
      dyScroll: -280,
      timeout: const Duration(minutes: 1),
    );
    await session.tap(model);
    final option = find.byValueKey(
      'settings-role-${route.role}-model-${route.provider}-${route.model}',
    );
    await session.waitFor(option, timeout: const Duration(seconds: 30));
    await session.tap(option);
    final enabled = find.byValueKey('system-agent-enabled-${route.role}');
    await session.scrollUntilVisible(
      find.byValueKey('settings-pane-scroll'),
      enabled,
      dyScroll: -200,
      timeout: const Duration(minutes: 1),
    );
    // The isolated live config starts all four Profiles disabled. This tap is
    // the product-level enable action and its canonical Settings revision must
    // be observed by the later spawn tool catalog.
    final before = await session.readSnapshot();
    final beforeRevision = (before['persistence'] as Map?)?['revision'];
    await session.tap(enabled);
    await session.waitForNoPendingFrame(timeout: const Duration(seconds: 20));
    final after = await session.readSnapshot();
    final afterRevision = (after['persistence'] as Map?)?['revision'];
    if (beforeRevision is! int ||
        afterRevision is! int ||
        afterRevision <= beforeRevision) {
      throw StateError(
        'agent setting did not produce a canonical revision: '
        '$beforeRevision -> $afterRevision',
      );
    }
  }
}

Future<void> _openProjectAndSubmit(
  FlutterDriverSession session,
  _Options options,
) async {
  await session.waitFor(find.byValueKey('sidebar-open-project'));
  await session.tap(find.byValueKey('sidebar-open-project'));
  await session.waitFor(find.byValueKey('project-path-dialog'));
  await session.tap(find.byValueKey('project-path-input'));
  await session.enterText(options.workspace);
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
  await session.waitFor(find.byValueKey('session-mode-mode.task'));
  await session.tap(find.byValueKey('session-mode-mode.task'));
  await session.waitForNoPendingFrame(timeout: const Duration(seconds: 10));
  final modeSnapshot = await session.readSnapshot();
  final mode = (modeSnapshot['workspace'] as Map?)?['threadMode'];
  if (mode != 'mode.task') {
    throw StateError('Task mode was not applied by the GUI: $mode');
  }
  await session.tap(find.byValueKey('composer-input'));
  await session.enterText(await File(options.promptFile).readAsString());
  await session.waitForNoPendingFrame(timeout: const Duration(seconds: 20));
  await session.tap(find.byValueKey('composer-submit'));
}

Future<Map<String, dynamic>> _waitForCompletion(
  FlutterDriverSession session,
  _Options options,
) async {
  final deadline = DateTime.now().add(options.timeout);
  var changedAt = DateTime.now();
  String? fingerprint;
  var confirmationHandled = false;
  Map<String, dynamic>? last;
  while (DateTime.now().isBefore(deadline)) {
    last = await session.readSnapshot();
    final workspace = last['workspace'] as Map?;
    final current = jsonEncode({
      'turn': workspace?['turn'],
      'lastTurn': workspace?['lastTurn'],
      'timeline': workspace?['timeline'],
      'agents': workspace?['agents'],
      'interaction': workspace?['activeInteraction'],
    });
    if (current != fingerprint) {
      fingerprint = current;
      changedAt = DateTime.now();
    } else if (DateTime.now().difference(changedAt) >= options.stallTimeout) {
      throw StateError('subagents acceptance made no observable progress');
    }
    final timeline = _timelineText(last);
    final busy = workspace?['isBusy'] == true;
    if (!busy && timeline.contains('PURE_SUBAGENTS_LIVE_OK')) return last;
    final interaction = workspace?['activeInteraction'];
    if (interaction is Map && interaction['kind'] == 'toolApproval') {
      await session.tap(find.byValueKey('tool-approve'));
    } else if (interaction is Map && interaction['kind'] == 'userInput') {
      final workflow = last['workflow'];
      final run = workflow is Map ? workflow['currentRun'] : null;
      final stage = run is Map ? run['currentStageId'] : null;
      if (stage == 'awaiting_confirmation' && !confirmationHandled) {
        await _tapFirstUserInputOption(session);
        confirmationHandled = true;
      } else {
        throw StateError(
          'root requested unexpected user input at $stage: $interaction',
        );
      }
    }
    await Future<void>.delayed(const Duration(milliseconds: 300));
  }
  throw StateError('subagents acceptance timed out; last=$last');
}

void _validateSnapshot(Map<String, dynamic> snapshot) {
  final workspace = snapshot['workspace'];
  if (workspace is! Map) throw StateError('terminal snapshot has no workspace');
  final agents = (workspace['agents'] as List? ?? const [])
      .whereType<Map>()
      .toList();
  final roles = agents
      .map((agent) => agent['role'])
      .whereType<String>()
      .toSet();
  for (final role in [
    'explorer',
    'executor',
    'worktree_executor',
    'reviewer',
  ]) {
    if (!roles.contains(role)) {
      throw StateError('terminal snapshot lacks $role Profile: $roles');
    }
  }
  final tools = (workspace['timeline'] as List? ?? const [])
      .whereType<Map>()
      .expand((row) => (row['tools'] as List? ?? const []).whereType<Map>())
      .toList();
  final names = tools.map((tool) => tool['name']).whereType<String>().toList();
  if (names.where((name) => name == 'spawn_agent').length < 2 ||
      !names.contains('close_agent')) {
    throw StateError(
      'terminal timeline lacks canonical spawn/close receipts: $names',
    );
  }
  final joined = tools
      .expand(
        (tool) => [tool['arguments'], tool['result'], tool['denialReason']],
      )
      .whereType<String>()
      .join('\n');
  for (final marker in [
    '"profileId":"executor"',
    '"profileId":"worktree_executor"',
    '"writablePaths":["allowed"]',
    '"workspaceDisposition":"cleanup"',
  ]) {
    if (!joined.replaceAll(' ', '').contains(marker)) {
      throw StateError('terminal receipt lacks $marker');
    }
  }
  final workflow = snapshot['workflow'];
  final run = workflow is Map ? workflow['currentRun'] : null;
  final history = run is Map ? run['history'] : null;
  final stages = <String>{
    if (history is List)
      for (final entry in history.whereType<Map>()) ...[
        if (entry['fromStageId'] is String) entry['fromStageId'] as String,
        if (entry['toStageId'] is String) entry['toStageId'] as String,
      ],
  };
  if (!stages.contains('integrating')) {
    throw StateError('terminal workflow history lacks integrating: $stages');
  }
  if (!_timelineText(snapshot).contains('REVIEWER_READ_ONLY_APPROVED')) {
    throw StateError('terminal timeline lacks reviewer approval marker');
  }
}

Future<void> _tapFirstUserInputOption(FlutterDriverSession session) async {
  if (await _isVisible(session, 'user-input-first-option')) {
    await session.tap(find.byValueKey('user-input-first-option'));
    await session.tap(find.byValueKey('user-input-submit'));
  } else if (await _isVisible(session, 'user-input-first-text')) {
    await session.tap(find.byValueKey('user-input-first-text'));
    await session.enterText('确认');
    await session.tap(find.byValueKey('user-input-submit'));
  } else {
    await session.tap(find.byValueKey('fallback-user-input'));
    await session.enterText('确认');
    await session.tap(find.byValueKey('fallback-user-input-submit'));
  }
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

String _timelineText(Map<String, dynamic> snapshot) =>
    (((snapshot['workspace'] as Map?)?['timeline'] as List?) ?? const [])
        .whereType<Map>()
        .map((row) => row['text'])
        .whereType<String>()
        .join('\n');

class _Route {
  const _Route(this.role, this.provider, this.model);

  final String role;
  final String provider;
  final String model;
}

class _Options {
  const _Options({
    required this.vmServiceUrl,
    required this.workspace,
    required this.promptFile,
    required this.settingsScreenshot,
    required this.finalScreenshot,
    required this.executor,
    required this.worktreeExecutor,
    required this.explorer,
    required this.reviewer,
    required this.timeout,
    required this.stallTimeout,
  });

  final String vmServiceUrl;
  final String workspace;
  final String promptFile;
  final String settingsScreenshot;
  final String finalScreenshot;
  final _Route executor;
  final _Route worktreeExecutor;
  final _Route explorer;
  final _Route reviewer;
  final Duration timeout;
  final Duration stallTimeout;

  static _Options parse(List<String> args) {
    String required(String name) {
      final index = args.indexOf(name);
      if (index < 0 || index + 1 >= args.length) {
        throw ArgumentError('$name is required');
      }
      return args[index + 1];
    }

    final executorProvider = required('--executor-provider');
    final executorModel = required('--executor-model');
    final worktreeProvider = required('--worktree-provider');
    final worktreeModel = required('--worktree-model');
    final explorerProvider = required('--explorer-provider');
    final explorerModel = required('--explorer-model');
    final reviewerProvider = required('--reviewer-provider');
    final reviewerModel = required('--reviewer-model');
    return _Options(
      vmServiceUrl: required('--vm-service-url'),
      workspace: required('--workspace'),
      promptFile: required('--prompt-file'),
      settingsScreenshot: required('--settings-screenshot'),
      finalScreenshot: required('--final-screenshot'),
      executor: _Route('executor', executorProvider, executorModel),
      worktreeExecutor: _Route(
        'worktree_executor',
        worktreeProvider,
        worktreeModel,
      ),
      explorer: _Route('explorer', explorerProvider, explorerModel),
      reviewer: _Route('reviewer', reviewerProvider, reviewerModel),
      timeout: Duration(seconds: int.parse(required('--timeout-seconds'))),
      stallTimeout: Duration(
        seconds: int.parse(required('--stall-timeout-seconds')),
      ),
    );
  }
}
