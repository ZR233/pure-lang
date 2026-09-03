//! Flutter Driver acceptance for real directory and worktree child Agents.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

Future<void> main(List<String> arguments) async {
  final options = _Options.parse(arguments);
  final evidence = _SubagentFlowEvidence();
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
    if (options.ssh case final ssh?) {
      await _configureSshProject(session, ssh);
    }
    await File(options.settingsScreenshot)
        .writeAsBytes(await session.screenshot(), flush: true);
    await File('${options.settingsScreenshot}.render-tree.txt')
        .writeAsString(await session.renderTree(), flush: true);
    await session.tap(find.byValueKey('settings-back'));
    await _openProjectAndSubmit(session, options);
    final snapshot = await _waitForCompletion(session, options, evidence);
    _validateSnapshot(snapshot, evidence);
    await File(options.finalScreenshot)
        .writeAsBytes(await session.screenshot(), flush: true);
    await File('${options.finalScreenshot}.render-tree.txt')
        .writeAsString(await session.renderTree(), flush: true);
    stdout.writeln(
      jsonEncode({
        'result': 'completed',
        'project': snapshot['project'],
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

Future<void> _configureSshProject(
  FlutterDriverSession session,
  _SshOptions ssh,
) async {
  await session.tap(find.byValueKey('settings-tab-ssh'));
  await session.waitFor(find.byValueKey('ssh-add-server'));
  await session.tap(find.byValueKey('ssh-add-server'));
  await session.waitFor(find.byValueKey('ssh-server-dialog'));
  for (final field in [
    ('ssh-server-name-input', _SshOptions.name),
    ('ssh-server-host-input', ssh.host),
    ('ssh-server-username-input', ssh.username),
    ('ssh-server-port-input', '${ssh.port}'),
  ]) {
    await session.tap(find.byValueKey(field.$1));
    await session.enterText(field.$2);
  }
  await session.tap(find.byValueKey('ssh-server-auth-input'));
  await session.waitFor(find.text('Password'));
  await session.tap(find.text('Password'));
  await session.waitFor(find.byValueKey('ssh-server-password-input'));
  await session.tap(find.byValueKey('ssh-server-password-input'));
  await session.enterText(ssh.password);
  await session.tap(find.byValueKey('ssh-server-save'));
  await session.waitForAbsent(
    find.byValueKey('ssh-server-dialog'),
    timeout: const Duration(seconds: 30),
  );

  final serverId = await _waitForSshServerId(session, _SshOptions.name);
  await session.waitFor(find.byValueKey('ssh-test-$serverId'));
  await session.tap(find.byValueKey('ssh-test-$serverId'));
  await session.waitFor(
    find.byValueKey('ssh-reconnect-$serverId'),
    timeout: const Duration(minutes: 3),
  );
  await session.tap(find.byValueKey('ssh-open-$serverId'));
  await session.waitFor(
    find.byValueKey('ssh-directory-dialog'),
    timeout: const Duration(minutes: 2),
  );
  final fixtureEntry = find.byValueKey('ssh-directory-entry-${ssh.workspace}');
  await session.scrollUntilVisible(
    find.byValueKey('ssh-directory-list'),
    fixtureEntry,
    dyScroll: -240,
    timeout: const Duration(minutes: 2),
  );
  await session.tap(fixtureEntry);
  await session.waitFor(
    find.byValueKey('ssh-directory-current-${ssh.workspace}'),
    timeout: const Duration(minutes: 2),
  );
  await session.tap(find.byValueKey('ssh-open-current-directory'));
  await session.waitForAbsent(
    find.byValueKey('ssh-directory-dialog'),
    timeout: const Duration(minutes: 2),
  );
  final deadline = DateTime.now().add(const Duration(minutes: 2));
  Object? lastPath;
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await session.readSnapshot();
    lastPath = (snapshot['project'] as Map?)?['path'];
    if (lastPath == ssh.workspace) return;
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw StateError(
    'SSH project did not become canonical before timeout: '
    'expected=${ssh.workspace}, last=$lastPath',
  );
}

Future<String> _waitForSshServerId(
  FlutterDriverSession session,
  String name,
) async {
  final deadline = DateTime.now().add(const Duration(seconds: 30));
  Object? last;
  while (DateTime.now().isBefore(deadline)) {
    final response = await session.requestData('ssh-server-id:$name');
    final decoded = jsonDecode(response);
    last = decoded;
    if (decoded is Map && decoded['serverId'] is String) {
      return decoded['serverId'] as String;
    }
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw StateError('SSH server did not become canonical: $last');
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
    await _configureAgentSetting(
      route,
      'model ${route.provider}/${route.model}',
      () async {
        final beforeSnapshot = await session.readSnapshot();
        final beforeModelRevision = _readSettingsRevision(beforeSnapshot);
        final beforeRoute = _readCanonicalRoute(beforeSnapshot, route.role);
        final model = find.byValueKey('settings-role-${route.role}-model');
        await session.scrollUntilVisible(
          find.byValueKey('settings-pane-scroll'),
          model,
          dyScroll: -280,
          timeout: const Duration(minutes: 1),
        );
        await session.tap(model);
        final option = find.byValueKey(
          'settings-role-${route.role}-model-'
          '${route.provider}-${route.model}',
        );
        await session.waitFor(option, timeout: const Duration(seconds: 30));
        await session.tap(option);
        await _waitForCanonicalRoute(
          session,
          route,
          beforeRoute: beforeRoute,
          beforeRevision: beforeModelRevision,
          timeout: const Duration(seconds: 20),
        );
        await _waitForStableFrame(session);
      },
    );

    await _configureAgentSetting(route, 'enabled', () async {
      final beforeEnableRevision = _readSettingsRevision(
        await session.readSnapshot(),
      );
      final enabled = find.byValueKey('system-agent-enabled-${route.role}');
      await session.scrollUntilVisible(
        find.byValueKey('settings-pane-scroll'),
        enabled,
        dyScroll: -200,
        timeout: const Duration(minutes: 1),
      );
      // The isolated live config starts all four Profiles disabled. This tap
      // is the product-level enable action and its canonical Settings revision
      // must be observed by the later spawn tool catalog.
      await session.tap(enabled);
      await _waitForSettingsRevision(
        session,
        beforeEnableRevision,
        timeout: const Duration(seconds: 20),
      );
      await _waitForStableFrame(session);
    });
  }
}

Future<void> _configureAgentSetting(
  _Route route,
  String stage,
  Future<void> Function() configure,
) async {
  try {
    await configure();
  } catch (error, stackTrace) {
    Error.throwWithStackTrace(
      StateError('failed to configure ${route.role} $stage: $error'),
      stackTrace,
    );
  }
}

Future<void> _waitForStableFrame(FlutterDriverSession session) {
  return session.waitForNoPendingFrame(timeout: const Duration(seconds: 20));
}

int _readSettingsRevision(Map<String, dynamic> snapshot) {
  final revision = (snapshot['settings'] as Map?)?['revision'];
  if (revision is int) return revision;
  throw StateError(
    'Studio snapshot has no canonical Settings revision: $revision',
  );
}

Future<int> _waitForSettingsRevision(
  FlutterDriverSession session,
  int beforeRevision, {
  required Duration timeout,
}) async {
  final deadline = DateTime.now().add(timeout);
  Object? lastSeen;
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await session.readSnapshot();
    lastSeen = (snapshot['settings'] as Map?)?['revision'];
    if (lastSeen is int && lastSeen > beforeRevision) return lastSeen;
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw StateError(
    'agent setting did not produce a canonical Settings revision before '
    'timeout: before=$beforeRevision, lastSeen=$lastSeen',
  );
}

_CanonicalRoute _readCanonicalRoute(
  Map<String, dynamic> snapshot,
  String roleKey,
) {
  final roles = (snapshot['settings'] as Map?)?['roles'];
  if (roles is List) {
    for (final value in roles) {
      if (value is Map && value['key'] == roleKey) {
        final provider = value['providerId'];
        final model = value['model'];
        if (provider is String && model is String) {
          return _CanonicalRoute(provider, model);
        }
      }
    }
  }
  throw StateError('Studio snapshot has no canonical route for role $roleKey');
}

Future<void> _waitForCanonicalRoute(
  FlutterDriverSession session,
  _Route target, {
  required _CanonicalRoute beforeRoute,
  required int beforeRevision,
  required Duration timeout,
}) async {
  final deadline = DateTime.now().add(timeout);
  _CanonicalRoute lastRoute = beforeRoute;
  var lastRevision = beforeRevision;
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await session.readSnapshot();
    lastRoute = _readCanonicalRoute(snapshot, target.role);
    lastRevision = _readSettingsRevision(snapshot);
    if (canonicalRouteReached(
      beforeProvider: beforeRoute.provider,
      beforeModel: beforeRoute.model,
      beforeRevision: beforeRevision,
      currentProvider: lastRoute.provider,
      currentModel: lastRoute.model,
      currentRevision: lastRevision,
      targetProvider: target.provider,
      targetModel: target.model,
    )) {
      return;
    }
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw StateError(
    'agent model did not reach canonical route before timeout: '
    'role=${target.role}, target=${target.provider}/${target.model}, '
    'beforeRoute=$beforeRoute, lastRoute=$lastRoute, '
    'beforeRevision=$beforeRevision, lastRevision=$lastRevision',
  );
}

/// Returns whether the observed canonical route satisfies a route transition.
///
/// A route change must advance the canonical Settings revision. Re-selecting
/// the already-canonical route is a controller no-op and may keep the revision
/// unchanged.
bool canonicalRouteReached({
  required String beforeProvider,
  required String beforeModel,
  required int beforeRevision,
  required String currentProvider,
  required String currentModel,
  required int currentRevision,
  required String targetProvider,
  required String targetModel,
}) {
  final routeMatches =
      currentProvider == targetProvider && currentModel == targetModel;
  if (!routeMatches) return false;

  final beforeAlreadyMatches =
      beforeProvider == targetProvider && beforeModel == targetModel;
  return beforeAlreadyMatches || currentRevision > beforeRevision;
}

Future<void> _openProjectAndSubmit(
  FlutterDriverSession session,
  _Options options,
) async {
  if (options.ssh == null) {
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
  }
  await session.waitFor(find.byValueKey('sidebar-new-session'));
  await session.tap(find.byValueKey('sidebar-new-session'));
  await session.tap(find.byValueKey('session-mode-selector'));
  await session.waitFor(find.byValueKey('session-mode-mode.task'));
  await session.tap(find.byValueKey('session-mode-mode.task'));
  await session.waitForNoPendingFrame(timeout: const Duration(seconds: 10));
  final navigationSnapshot = await session.readSnapshot();
  final newThreadMode =
      (navigationSnapshot['navigation'] as Map?)?['newThreadMode'];
  if (newThreadMode != 'mode.task') {
    throw StateError('new Thread navigation mode was not Task: $newThreadMode');
  }
  await session.tap(find.byValueKey('composer-input'));
  await session.enterText(await File(options.promptFile).readAsString());
  await session.waitForNoPendingFrame(timeout: const Duration(seconds: 20));
  final submittedAt = DateTime.now().toUtc();
  final submitElapsed = Stopwatch()..start();
  stdout.writeln(
    jsonEncode({
      'event': 'promptSubmitStarted',
      'capturedAt': submittedAt.toIso8601String(),
    }),
  );
  try {
    await session.tap(
      find.byValueKey('composer-submit'),
      timeout: options.stallTimeout,
    );
    stdout.writeln(
      jsonEncode({
        'event': 'promptSubmitReturned',
        'elapsedMs': submitElapsed.elapsedMilliseconds,
        'capturedAt': DateTime.now().toUtc().toIso8601String(),
      }),
    );
  } on TimeoutException catch (error) {
    throw StateError(
      'root provider produced no response during prompt submission: $error',
    );
  }
}

Future<Map<String, dynamic>> _waitForCompletion(
  FlutterDriverSession session,
  _Options options,
  _SubagentFlowEvidence evidence,
) async {
  final deadline = DateTime.now().add(options.timeout);
  var changedAt = DateTime.now();
  String? fingerprint;
  var clarificationHandled = false;
  var revisionRequested = false;
  var revisedPlanApproved = false;
  final handledInteractionIds = <Object?>{};
  Map<String, dynamic>? last;
  while (DateTime.now().isBefore(deadline)) {
    last = await session.readSnapshot();
    evidence.observe(last);
    final workspace = last['workspace'] as Map?;
    final current = jsonEncode({
      'turn': workspace?['turn'],
      'lastTurn': workspace?['lastTurn'],
      'timeline': workspace?['timeline'],
      'agents': workspace?['agents'],
      'interaction': workspace?['activeInteraction'],
      'workflow': last['workflow'],
    });
    if (current != fingerprint) {
      fingerprint = current;
      changedAt = DateTime.now();
    } else if (DateTime.now().difference(changedAt) >= options.stallTimeout) {
      throw StateError('subagents acceptance made no observable progress');
    }
    final interaction = workspace?['activeInteraction'];
    final workflow = last['workflow'];
    final run = workflow is Map ? workflow['currentRun'] : null;
    final state = run is Map ? run['currentStateId'] as String? : null;
    if (interaction is Map &&
        handledInteractionIds.contains(interaction['id'])) {
      await Future<void>.delayed(const Duration(milliseconds: 300));
      continue;
    }
    if (interaction is Map && interaction['kind'] == 'toolApproval') {
      await _resolveInteraction(
        session: session,
        artifactPrefix: options.finalScreenshot,
        kind: 'toolApproval',
        stage: state,
        action: () => _tapInteractionAction(session, 'tool-approve'),
      );
      handledInteractionIds.add(interaction['id']);
    } else if (interaction is Map && interaction['kind'] == 'userInput') {
      if (state == 'planning' && !clarificationHandled) {
        await _resolveInteraction(
          session: session,
          artifactPrefix: options.finalScreenshot,
          kind: 'clarification',
          stage: state,
          action: () => _tapFirstUserInputOption(session),
        );
        handledInteractionIds.add(interaction['id']);
        clarificationHandled = true;
        evidence.sawClarification = true;
      } else if (state == 'planning' && !revisionRequested) {
        _assertPlanInteraction(interaction);
        await _resolveInteraction(
          session: session,
          artifactPrefix: options.finalScreenshot,
          kind: 'plan-revision',
          stage: state,
          action: () => _requestPlanRevision(session),
        );
        handledInteractionIds.add(interaction['id']);
        revisionRequested = true;
        evidence.sawPlanRevisionRequest = true;
      } else if (state == 'planning' &&
          revisionRequested &&
          !revisedPlanApproved) {
        _assertRevisedPlanInteraction(interaction);
        await _resolveInteraction(
          session: session,
          artifactPrefix: options.finalScreenshot,
          kind: 'plan-approval',
          stage: state,
          action: () => _approvePlan(session),
        );
        handledInteractionIds.add(interaction['id']);
        revisedPlanApproved = true;
        evidence.sawRevisedPlanWhilePlanning = true;
        evidence.sawRevisedPlanApproval = true;
      } else {
        throw StateError(
          'root requested unexpected user input at $state: $interaction',
        );
      }
    }
    if (subagentsAcceptanceCompleted(last)) return last;
    await Future<void>.delayed(const Duration(milliseconds: 300));
  }
  throw StateError('subagents acceptance timed out; last=$last');
}

Future<void> _resolveInteraction({
  required FlutterDriverSession session,
  required String artifactPrefix,
  required String kind,
  required String? stage,
  required Future<void> Function() action,
}) async {
  final elapsed = Stopwatch()..start();
  stdout.writeln(
    jsonEncode({
      'event': 'interactionResolutionStarted',
      'kind': kind,
      'stage': stage,
      'capturedAt': DateTime.now().toUtc().toIso8601String(),
    }),
  );
  await File('$artifactPrefix.$kind-before.png')
      .writeAsBytes(await session.screenshot(), flush: true);
  await File('$artifactPrefix.$kind-before.render-tree.txt')
      .writeAsString(await session.renderTree(), flush: true);
  await action();
  stdout.writeln(
    jsonEncode({
      'event': 'interactionResolutionReturned',
      'kind': kind,
      'stage': stage,
      'elapsedMs': elapsed.elapsedMilliseconds,
      'capturedAt': DateTime.now().toUtc().toIso8601String(),
    }),
  );
}

/// Returns true only when the complete subagent workflow has reached its
/// terminal state and the live acceptance marker is a root final answer.
bool subagentsAcceptanceCompleted(Map<String, dynamic> snapshot) {
  final workspace = snapshot['workspace'];
  if (workspace is! Map || workspace['isBusy'] == true) return false;
  if (workspace['activeInteraction'] != null) return false;

  final workflow = snapshot['workflow'];
  final run = workflow is Map ? workflow['currentRun'] : null;
  if (run is! Map ||
      run['currentStateId'] != 'completed' ||
      run['terminal'] != true) {
    return false;
  }

  final roles = (workspace['agents'] as List? ?? const [])
      .whereType<Map>()
      .map((agent) => agent['role'])
      .whereType<String>()
      .toSet();
  if (!{
    'explorer',
    'executor',
    'worktree_executor',
    'reviewer',
  }.every(roles.contains)) {
    return false;
  }

  return (workspace['timeline'] as List? ?? const []).whereType<Map>().any(
    (row) =>
        row['type'] == 'finalAnswer' &&
        row['text'] is String &&
        (row['text'] as String).contains('PURE_SUBAGENTS_LIVE_OK'),
  );
}

/// Applies the live subagent snapshot projection checks without requiring a
/// Driver connection. This keeps the acceptance-only evidence rules directly
/// unit-testable.
void validateSubagentSnapshotProjection(Map<String, dynamic> snapshot) {
  _SubagentFlowEvidence().observe(snapshot);
}

void _validateSnapshot(
  Map<String, dynamic> snapshot,
  _SubagentFlowEvidence evidence,
) {
  final workspace = snapshot['workspace'];
  if (workspace is! Map) throw StateError('terminal snapshot has no workspace');
  if (workspace['threadMode'] != 'mode.task') {
    throw StateError(
      'terminal workspace mode is not Task: ${workspace['threadMode']}',
    );
  }
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
  for (final role in ['executor', 'worktree_executor']) {
    final count = agents.where((agent) => agent['role'] == role).length;
    if (count < 2) {
      throw StateError('terminal snapshot has fewer than two $role Agents');
    }
  }
  final tools = (workspace['timeline'] as List? ?? const [])
      .whereType<Map>()
      .expand((row) => (row['tools'] as List? ?? const []).whereType<Map>())
      .toList();
  final names = tools.map((tool) => tool['name']).whereType<String>().toList();
  if (names.where((name) => name == 'spawn_agent').length < 7 ||
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
    '"writablePaths":["allowed/normalize"]',
    '"writablePaths":["allowed/validate"]',
    '"workspaceDisposition":"cleanup"',
  ]) {
    if (!joined.replaceAll(' ', '').contains(marker)) {
      throw StateError('terminal receipt lacks $marker');
    }
  }
  evidence.validate();
  if (!_timelineText(snapshot).contains('REVIEWER_READ_ONLY_APPROVED')) {
    throw StateError('terminal timeline lacks reviewer approval marker');
  }
}

void _assertPlanInteraction(Map interaction) {
  final body = interaction['body'];
  if (body is! String || !body.trimLeft().startsWith('# ')) {
    throw StateError('Plan confirmation has no complete Markdown plan');
  }
}

void _assertRevisedPlanInteraction(Map interaction) {
  _assertPlanInteraction(interaction);
  final plan = (interaction['body'] as String).toLowerCase();
  for (final marker in [
    'allowed/normalize',
    'allowed/validate',
    'worktree_alpha',
    'worktree_beta',
    'cleanup',
  ]) {
    if (!plan.contains(marker)) {
      throw StateError('revised plan omitted `$marker`: $plan');
    }
  }
}

Future<void> _requestPlanRevision(FlutterDriverSession session) async {
  await session.waitFor(
    find.byValueKey('plan-feedback-input'),
    timeout: const Duration(seconds: 30),
  );
  await session.tap(find.byValueKey('plan-feedback-input'));
  await session.enterText(
    '请在计划中逐项写明 allowed/normalize、allowed/validate、'
    'worktree_alpha、worktree_beta 的并行所有权；补充硬性顺序：两次 cherry-pick 都成功后才可开始'
    '任何 cleanup，再逐个 cleanup 并验证，禁止整合一个就清理一个。',
  );
  await _tapInteractionAction(session, 'plan-submit-revision');
}

Future<void> _approvePlan(FlutterDriverSession session) async {
  await session.waitFor(
    find.byValueKey('plan-approve'),
    timeout: const Duration(seconds: 30),
  );
  await _tapInteractionAction(session, 'plan-approve');
}

class _SubagentFlowEvidence {
  final Set<String> visitedStates = {};
  final Set<String> workflowTools = {};
  final Set<String> planTools = {};
  final Set<String> submittedPlans = {};
  var successfulPlanSubmissions = 0;
  bool sawClarification = false;
  bool sawPlanRevisionRequest = false;
  bool sawRevisedPlanWhilePlanning = false;
  bool sawRevisedPlanApproval = false;

  void observe(Map<String, dynamic> snapshot) {
    final workflow = snapshot['workflow'];
    final run = workflow is Map ? workflow['currentRun'] : null;
    if (run is Map) {
      final state = run['currentStateId'];
      if (state is String) visitedStates.add(state);
      for (final forbidden in ['stages', 'history', 'definition', 'prompt']) {
        if (run.containsKey(forbidden)) {
          throw StateError('GUI workflow snapshot leaked `$forbidden`: $run');
        }
      }
    }
    final timeline =
        (((snapshot['workspace'] as Map?)?['timeline'] as List?) ?? const [])
            .whereType<Map>()
            .toList();
    final tools = timeline.expand(
      (row) => (row['tools'] as List? ?? const []).whereType<Map>(),
    );
    for (final tool in tools) {
      final name = tool['name'];
      if (name == 'workflow_state') {
        throw StateError('legacy workflow_state appeared in the real GUI');
      }
      if (name == 'submit_plan') {
        throw StateError('legacy submit_plan appeared in the real GUI');
      }
      if (name is String && name.startsWith('plan_')) {
        planTools.add(name);
        if (name == 'plan_submit' && tool['status'] == 'succeeded') {
          successfulPlanSubmissions += 1;
          final arguments = tool['arguments'];
          final decoded = arguments is String
              ? jsonDecode(arguments)
              : arguments;
          if (decoded is Map && decoded['plan'] is String) {
            submittedPlans.add((decoded['plan'] as String).trim());
          }
        }
        final arguments = tool['arguments'];
        final encoded = arguments is String ? arguments : jsonEncode(arguments);
        for (final forbidden in ['definition', 'compile', 'supersede']) {
          if (encoded.contains('"$forbidden"')) {
            throw StateError(
              '$name received forbidden graph-authoring input: $encoded',
            );
          }
        }
      }
      if (name is! String || !name.startsWith('workflow_')) continue;
      workflowTools.add(name);
      final arguments = tool['arguments'];
      final encoded = arguments is String ? arguments : jsonEncode(arguments);
      for (final forbidden in ['definition', 'compile', 'supersede']) {
        if (encoded.contains('"$forbidden"')) {
          throw StateError(
            '$name received forbidden graph-authoring input: $encoded',
          );
        }
      }
      if (name == 'workflow_transition' && tool['status'] == 'succeeded') {
        final decoded = arguments is String ? jsonDecode(arguments) : arguments;
        if (decoded is Map && decoded['targetStateId'] is String) {
          final source = decoded['expectedStateId'];
          if (source is String) visitedStates.add(source);
          visitedStates.add(decoded['targetStateId'] as String);
        }
      }
    }
    for (final row in timeline) {
      final text = row['text'];
      if (text is String && submittedPlans.contains(text.trim())) {
        throw StateError(
          'hidden Plan continuation was projected as a GUI timeline message',
        );
      }
    }
  }

  void validate() {
    if (!sawClarification ||
        !sawPlanRevisionRequest ||
        !sawRevisedPlanWhilePlanning ||
        !sawRevisedPlanApproval) {
      throw StateError(
        'missing clarification/revision evidence: '
        '$sawClarification/$sawPlanRevisionRequest/'
        '$sawRevisedPlanWhilePlanning/$sawRevisedPlanApproval',
      );
    }
    for (final state in [
      'planning',
      'editing_documents',
      'working',
      'integrating',
      'reviewing',
      'completed',
    ]) {
      if (!visitedStates.contains(state)) {
        throw StateError('workflow trace is missing $state: $visitedStates');
      }
    }
    for (final required in [
      'workflow_current',
      'workflow_next',
      'workflow_graph',
      'workflow_history',
      'workflow_transition',
    ]) {
      if (!workflowTools.contains(required)) {
        throw StateError(
          'split workflow tool $required is missing: $workflowTools',
        );
      }
    }
    for (final required in [
      'plan_current',
      'plan_next',
      'plan_history',
      'plan_submit',
    ]) {
      if (!planTools.contains(required)) {
        throw StateError('required Plan tool $required is missing: $planTools');
      }
    }
    if (successfulPlanSubmissions < 2) {
      throw StateError(
        'expected two successful plan_submit calls, got '
        '$successfulPlanSubmissions',
      );
    }
  }
}

Future<void> _tapFirstUserInputOption(FlutterDriverSession session) async {
  if (await _isVisible(session, 'user-input-first-option')) {
    await session.tap(find.byValueKey('user-input-first-option'));
    await _tapInteractionAction(session, 'user-input-submit');
  } else if (await _isVisible(session, 'user-input-first-text')) {
    await session.tap(find.byValueKey('user-input-first-text'));
    await session.enterText('确认');
    await _tapInteractionAction(session, 'user-input-submit');
  } else {
    await session.tap(find.byValueKey('fallback-user-input'));
    await session.enterText('确认');
    await _tapInteractionAction(session, 'fallback-user-input-submit');
  }
}

Future<void> _tapInteractionAction(
  FlutterDriverSession session,
  String actionKey,
) async {
  final action = find.byValueKey(actionKey);
  await session.scrollUntilVisible(
    find.byValueKey('workspace-footer-scrollable'),
    action,
    dyScroll: -220,
    timeout: const Duration(seconds: 30),
  );
  await session.tap(action);
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

class _CanonicalRoute {
  const _CanonicalRoute(this.provider, this.model);

  final String provider;
  final String model;

  @override
  String toString() => '$provider/$model';
}

class _SshOptions {
  const _SshOptions({
    required this.host,
    required this.username,
    required this.password,
    required this.port,
    required this.workspace,
  });

  static const name = 'Pure SSH Acceptance';
  final String host;
  final String username;
  final String password;
  final int port;
  final String workspace;
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
    required this.ssh,
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
  final _SshOptions? ssh;

  static _Options parse(List<String> args) {
    String required(String name) {
      final index = args.indexOf(name);
      if (index < 0 || index + 1 >= args.length) {
        throw ArgumentError('$name is required');
      }
      return args[index + 1];
    }

    String? optional(String name) {
      final index = args.indexOf(name);
      if (index < 0) return null;
      if (index + 1 >= args.length) {
        throw ArgumentError('$name requires a value');
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
    final sshHost = optional('--ssh-host');
    final sshUsername = optional('--ssh-username');
    final sshPort = optional('--ssh-port');
    final sshWorkspace = optional('--ssh-workspace');
    final sshConfigured = [
      sshHost,
      sshUsername,
      sshPort,
      sshWorkspace,
    ].any((value) => value != null);
    _SshOptions? ssh;
    if (sshConfigured) {
      if ([
        sshHost,
        sshUsername,
        sshPort,
        sshWorkspace,
      ].any((value) => value == null)) {
        throw ArgumentError('all SSH acceptance arguments are required');
      }
      final password = Platform.environment['PURE_SUBAGENTS_SSH_PASSWORD'];
      if (password == null || password.isEmpty) {
        throw ArgumentError(
          'PURE_SUBAGENTS_SSH_PASSWORD is required in the Driver environment',
        );
      }
      ssh = _SshOptions(
        host: sshHost!,
        username: sshUsername!,
        password: password,
        port: int.parse(sshPort!),
        workspace: sshWorkspace!,
      );
    }
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
      ssh: ssh,
    );
  }
}
