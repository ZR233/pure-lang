// Remote worktree session acceptance through the real native GUI.
//
// The script drives the add-project wizard against a real SSH server, opens a
// real remote Git repository, creates a `worktree` workspace session, submits a
// real user prompt, waits for the live model Turn to reach a terminal state,
// and records screenshot plus render-tree evidence for every step.
//
// It never uses the demo API, a scripted provider, or fixture responses. The
// caller launches the GUI with `cargo xtask run-gui --driver` (Flutter Driver
// entrypoint) against an isolated Studio home and provides an SSH server whose
// key authentication already works.
//
// Taps are sent as [RawTap] because the stock `Tap` command pre-filters with
// `hitTestable()`, which finds no candidates on the Linux desktop embedder and
// therefore blocks every interaction until its timeout.
//
// Usage:
//   dart run test_driver/remote_worktree_acceptance_driver.dart \
//     --vm-service-url <url> --output-dir <dir> \
//     --ssh-host <host> --ssh-username <user> --ssh-identity <path> \
//     --remote-repository <posix path> --prompt <text> --prompt-marker <token>
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'raw_tap.dart';

Future<void> main(List<String> arguments) async {
  final options = _Options.parse(arguments);
  final output = Directory(options.outputDir);
  await output.create(recursive: true);
  final driver = await FlutterDriver.connect(
    dartVmServiceUrl: options.vmServiceUrl,
    printCommunication: false,
    logCommunicationToFile: false,
  );
  try {
    // Frame sync must stay disabled: with it enabled every `waitFor` first
    // waits for `transientCallbackCount == 0`, so any running progress spinner
    // (for example while the SSH directory browser is loading) blocks the
    // command until its timeout. Disabled, waits resolve immediately when the
    // finder already matches and otherwise retry on each produced frame.
    await driver.sendCommand(const SetFrameSync(false));
    final receipt = await _run(driver, options, output);
    stdout.writeln(jsonEncode({'result': 'completed', ...receipt}));
  } catch (error, stackTrace) {
    stderr.writeln('remote worktree driver failed: $error');
    stderr.writeln(stackTrace);
    try {
      await File('${output.path}/failure.png')
          .writeAsBytes(await driver.screenshot(), flush: true);
      await File(
        '${output.path}/failure.render-tree.txt',
      ).writeAsString((await driver.getRenderTree()).tree ?? '', flush: true);
    } on Object {
      // Preserve the original acceptance failure.
    }
    rethrow;
  } finally {
    await driver.close();
  }
}

Future<Map<String, Object?>> _run(
  FlutterDriver driver,
  _Options options,
  Directory output,
) async {
  Future<void> tap(String key, {Duration? timeout}) async {
    final finder = find.byValueKey(key);
    await driver.waitFor(
      finder,
      timeout: timeout ?? const Duration(seconds: 60),
    );
    await driver.sendCommand(
      RawTap(finder, timeout: timeout ?? const Duration(seconds: 60)),
    );
  }

  Future<void> capture(String name) async {
    await File('${output.path}/$name.png')
        .writeAsBytes(await driver.screenshot(), flush: true);
    final tree = (await driver.getRenderTree()).tree ?? '';
    await File('${output.path}/$name.render-tree.txt')
        .writeAsString(tree, flush: true);
    if (tree.contains('OVERFLOWING')) {
      throw StateError('overflow rendered during $name');
    }
  }

  Future<Map<String, dynamic>> snapshot() async => jsonDecode(
    await driver.requestData('snapshot', timeout: const Duration(seconds: 20)),
  ) as Map<String, dynamic>;

  await driver.waitFor(
    find.byValueKey('studio-shell'),
    timeout: const Duration(minutes: 2),
  );
  await capture('00-shell');

  // 1. Add the real SSH server through the add-project wizard.
  await tap('sidebar-open-project');
  await driver.waitFor(find.byValueKey('add-project-dialog'));
  await tap('add-project-remote');
  await tap('add-project-continue');
  await driver.waitFor(
    find.byValueKey('add-project-new-connection'),
    timeout: const Duration(minutes: 1),
  );
  await tap('add-project-new-connection');
  await driver.waitFor(find.byValueKey('ssh-server-dialog'));
  for (final field in <(String, String)>[
    ('ssh-server-alias-input', options.sshAlias),
    ('ssh-server-host-input', options.sshHost),
    ('ssh-server-port-input', '${options.sshPort}'),
    ('ssh-server-username-input', options.sshUsername),
    ('ssh-server-identity-input', options.sshIdentity),
  ]) {
    await tap(field.$1);
    await driver.enterText(field.$2);
  }
  await capture('01-ssh-connection');
  await tap('ssh-server-save');
  // The wizard saves the server, tests the connection, and opens the remote
  // directory browser only after the SSH transport is ready.
  await driver.waitFor(
    find.byValueKey('ssh-directory-dialog'),
    timeout: const Duration(minutes: 4),
  );

  // 2. Browse to the real remote repository and open it as a Project.
  await _navigateRemoteDirectory(driver, tap, options.remoteRepository);
  await capture('02-remote-directory');
  final projectPath = await _openRemoteProject(
    driver,
    snapshot,
    tap,
    options,
    output,
  );
  await capture('03-remote-project');

  // 3. Create the session in worktree workspace mode on the start page.
  await driver.waitFor(find.byValueKey('sidebar-new-session'));
  await tap('sidebar-new-session');
  await driver.waitFor(
    find.byValueKey('studio-start-page'),
    timeout: const Duration(seconds: 60),
  );
  final worktreeModeVia = await _selectWorktreeMode(driver, tap, snapshot);
  await capture('04-start-page-worktree');

  // 4. Submit the real user prompt and wait for the live Turn to finish.
  await tap('composer-input');
  await driver.enterText(options.prompt);
  final submittedAt = DateTime.now().toUtc();
  final terminal = await _submitAndAwaitTurn(
    driver,
    snapshot,
    options,
    submittedAt,
  );
  await capture('05-turn-completed');

  // 5. Assert the canonical worktree facts and the real model output.
  final workspace = terminal['workspace'] as Map?;
  if (workspace == null) {
    throw StateError('terminal snapshot has no workspace');
  }
  final threadId = workspace['threadId'];
  if (threadId is! String || threadId.isEmpty) {
    throw StateError('terminal workspace has no thread id: $threadId');
  }
  final sidebar = terminal['sidebarDirectory'] as Map?;
  final modes = (sidebar?['workspaceModes'] as Map?) ?? const {};
  final workspaceMode = modes[threadId];
  if (workspaceMode != 'worktree') {
    throw StateError(
      'thread $threadId workspace mode is not worktree: $workspaceMode',
    );
  }
  await driver.waitFor(
    find.byValueKey('thread-workspace-mode-$threadId'),
    timeout: const Duration(seconds: 30),
  );

  final timeline = (workspace['timeline'] as List? ?? const [])
      .whereType<Map>()
      .toList();
  final assistant = timeline
      .where(
        (row) => row['type'] == 'finalAnswer' || row['type'] == 'commentary',
      )
      .map((row) => row['text'])
      .whereType<String>()
      .where((text) => text.trim().isNotEmpty)
      .toList();
  if (assistant.isEmpty) {
    throw StateError('timeline has no non-empty assistant output');
  }
  final assistantText = assistant.join('\n');
  if (!assistantText.contains(options.promptMarker)) {
    throw StateError(
      'assistant output does not contain ${options.promptMarker}: '
      '$assistantText',
    );
  }
  final userEcho = timeline
      .where((row) => row['type'] == 'userMessage')
      .map((row) => row['text'])
      .whereType<String>()
      .any((text) => text.trim() == options.prompt.trim());
  if (!userEcho) {
    throw StateError('timeline does not echo the submitted user prompt');
  }
  final lastTurn = workspace['lastTurn'] as Map?;
  final turnStatus = lastTurn?['status'];
  if (turnStatus != 'completed') {
    throw StateError('last Turn did not complete: $lastTurn');
  }
  final tools = [
    for (final row in timeline)
      for (final tool in (row['tools'] as List? ?? const []).whereType<Map>())
        {
          'name': tool['name'],
          'status': tool['status'],
          'exitCode': tool['exitCode'],
        },
  ];
  return <String, Object?>{
    'threadId': threadId,
    'projectPath': projectPath,
    'workspaceMode': workspaceMode,
    'worktreeModeVia': worktreeModeVia,
    'turnStatus': turnStatus,
    'assistantText': assistantText,
    'submittedAt': submittedAt.toIso8601String(),
    'tools': tools,
    'timelineRowCount': timeline.length,
  };
}

/// Opens the remote Project from the directory dialog.
///
/// Selects `worktree` as the start-page workspace mode.
///
/// Tier 1 is the real popup-menu interaction (the [RawTap] handler waits for the
/// target center to stop moving before dispatching, which covers the menu
/// animation). If the item still cannot be activated through the driver
/// transport, tier 2 uses the acceptance-only data command to set the same
/// canonical draft fact; the popup-menu interaction itself is covered by the
/// native integration test `remote Project start page offers worktree and drives
/// a worktree marker` in `cargo xtask verify-gui --integration`. The tier used
/// is reported in the receipt.
Future<String> _selectWorktreeMode(
  FlutterDriver driver,
  Future<void> Function(String key, {Duration? timeout}) tap,
  Future<Map<String, dynamic>> Function() snapshot,
) async {
  Future<Object?> mode() async =>
      ((await snapshot())['navigation'] as Map?)?['newThreadWorkspaceMode'];
  Future<bool> waitForWorktree(Duration budget) async {
    final settle = DateTime.now().add(budget);
    while (DateTime.now().isBefore(settle)) {
      if (await mode() == 'worktree') return true;
      await Future<void>.delayed(const Duration(milliseconds: 300));
    }
    return false;
  }

  if (await mode() == 'worktree') return 'already-set';

  await tap('session-workspace-mode-selector');
  await driver.waitFor(
    find.byValueKey('session-workspace-mode-worktree'),
    timeout: const Duration(seconds: 30),
  );
  await tap('session-workspace-mode-worktree');
  if (await waitForWorktree(const Duration(seconds: 20))) {
    stdout.writeln(
      jsonEncode({'event': 'worktreeModeSelected', 'via': 'popup-menu'}),
    );
    return 'popup-menu';
  }

  final reply = jsonDecode(
    await driver.requestData(
      'set-new-thread-workspace-mode:worktree',
      timeout: const Duration(seconds: 20),
    ),
  );
  if (await waitForWorktree(const Duration(seconds: 20))) {
    stdout.writeln(
      jsonEncode({
        'event': 'worktreeModeSelected',
        'via': 'acceptance-datacommand',
        'reply': reply,
      }),
    );
    return 'acceptance-datacommand';
  }
  throw StateError('start page draft mode is not worktree: ${await mode()}');
}

/// Browsing the remote directory is racy: the dialog's initial listing load
/// overwrites the path field when it completes, so the requested path must be
/// typed only after a settled listing exists, and the result must be observed in
/// the render tree because the listing path is not part of the state snapshot.
Future<void> _navigateRemoteDirectory(
  FlutterDriver driver,
  Future<void> Function(String key, {Duration? timeout}) tap,
  String expected,
) async {
  final targetKey = 'ssh-directory-current-$expected';
  for (var attempt = 1; attempt <= 4; attempt++) {
    await driver.waitFor(
      find.byValueKey('ssh-directory-dialog'),
      timeout: const Duration(minutes: 2),
    );
    final settled = await _pollTree(
      driver,
      'ssh-directory-current-',
      const Duration(minutes: 2),
    );
    if (!settled) {
      throw StateError('remote directory dialog produced no listing');
    }
    await tap('ssh-directory-path-input');
    await driver.enterText(expected);
    await driver.sendTextInputAction(
      TextInputAction.go,
      timeout: const Duration(seconds: 30),
    );
    var reached = await _pollTree(
      driver,
      targetKey,
      const Duration(seconds: 45),
    );
    if (!reached) {
      await tap('ssh-directory-go');
      reached = await _pollTree(driver, targetKey, const Duration(seconds: 45));
    }
    stdout.writeln(
      jsonEncode({
        'event': 'remoteDirectoryAttempt',
        'attempt': attempt,
        'reached': reached,
      }),
    );
    if (reached) return;
  }
  throw StateError('remote directory browsing did not reach $expected');
}

Future<bool> _pollTree(
  FlutterDriver driver,
  String needle,
  Duration budget,
) async {
  final deadline = DateTime.now().add(budget);
  while (DateTime.now().isBefore(deadline)) {
    final tree = (await driver.getRenderTree()).tree ?? '';
    if (tree.contains(needle)) return true;
    await Future<void>.delayed(const Duration(milliseconds: 500));
  }
  return false;
}

/// The stock `ssh-open-current-directory` action is the only product path that
/// calls `openRemoteProject`, but its pointer event does not activate the
/// dialog's filled action button in this embedder. Two directed activation
/// attempts are tried: first an Enter ("go") rebuild followed by another tap,
/// then the same product action from the Settings SSH entry whose dialog has no
/// project-name field.
Future<String> _openRemoteProject(
  FlutterDriver driver,
  Future<Map<String, dynamic>> Function() snapshot,
  Future<void> Function(String key, {Duration? timeout}) tap,
  _Options options,
  Directory output,
) async {
  Future<bool> pollProject(Duration budget) async {
    final deadline = DateTime.now().add(budget);
    while (DateTime.now().isBefore(deadline)) {
      final path = ((await snapshot())['project'] as Map?)?['path'];
      if (path == options.remoteRepository) return true;
      await Future<void>.delayed(const Duration(milliseconds: 250));
    }
    return false;
  }

  Future<bool> activateOpen(String attempt) async {
    try {
      await tap(
        'ssh-open-current-directory',
        timeout: const Duration(seconds: 30),
      );
    } on Object catch (error) {
      stdout.writeln(
        jsonEncode({
          'event': 'openActionTapFailed',
          'attempt': attempt,
          'error': '$error',
        }),
      );
      return false;
    }
    final opened = await pollProject(const Duration(seconds: 45));
    stdout.writeln(
      jsonEncode({
        'event': 'openActionAttempt',
        'attempt': attempt,
        'opened': opened,
      }),
    );
    return opened;
  }

  Future<void> dumpTree(String name) async {
    await File('${output.path}/$name.render-tree.txt')
        .writeAsString((await driver.getRenderTree()).tree ?? '', flush: true);
    await File('${output.path}/$name.png')
        .writeAsBytes(await driver.screenshot(), flush: true);
  }

  // Attempt 1: rebuild the dialog through the path field's `go` action, then
  // activate the open action again.
  await tap('ssh-directory-path-input');
  await driver.sendTextInputAction(
    TextInputAction.go,
    timeout: const Duration(seconds: 30),
  );
  stdout.writeln(jsonEncode({'event': 'pathFieldGoPressed'}));
  await _pollTree(
    driver,
    'ssh-directory-current-${options.remoteRepository}',
    const Duration(seconds: 45),
  );
  if (await activateOpen('go-rebuild')) return options.remoteRepository;
  await dumpTree('06-open-attempt1-failed');

  // Attempt 2: same product action from the Settings SSH entry (no project-name
  // field, therefore a shorter dialog action bar).
  await tap('settings-open');
  await driver.waitFor(
    find.byValueKey('settings-page'),
    timeout: const Duration(seconds: 30),
  );
  await tap('settings-tab-ssh');
  await tap('ssh-open-${options.sshAlias}');
  await driver.waitFor(
    find.byValueKey('ssh-directory-dialog'),
    timeout: const Duration(minutes: 2),
  );
  await tap('ssh-directory-path-input');
  await driver.enterText(options.remoteRepository);
  await tap('ssh-directory-go');
  await _pollTree(
    driver,
    'ssh-directory-current-${options.remoteRepository}',
    const Duration(seconds: 60),
  );
  if (await activateOpen('settings-entry')) {
    await tap('settings-back');
    return options.remoteRepository;
  }
  await dumpTree('07-open-attempt2-failed');
  throw StateError(
    'remote directory dialog did not open the Project after two activation '
    'attempts; see 06/07 render trees',
  );
}

Future<Map<String, dynamic>> _submitAndAwaitTurn(
  FlutterDriver driver,
  Future<Map<String, dynamic>> Function() snapshot,
  _Options options,
  DateTime submittedAt,
) async {
  final deadline = DateTime.now().add(options.timeout);
  var changedAt = DateTime.now();
  String? fingerprint;
  var sawTurn = false;
  final handledInteractions = <Object?>{};
  Map<String, dynamic>? last;
  final submit = find.byValueKey('composer-submit');
  await driver.waitFor(submit, timeout: options.stall);
  await driver.sendCommand(RawTap(submit, timeout: options.stall));
  stdout.writeln(
    jsonEncode({
      'event': 'promptSubmitted',
      'capturedAt': submittedAt.toIso8601String(),
    }),
  );
  while (DateTime.now().isBefore(deadline)) {
    last = await snapshot();
    final workspace = last['workspace'] as Map?;
    final current = jsonEncode({
      'turn': workspace?['turn'],
      'lastTurn': workspace?['lastTurn'],
      'interaction': workspace?['activeInteraction'],
      'rows': workspace?['timeline'] == null ? null : 'set',
    });
    if (current != fingerprint) {
      fingerprint = current;
      changedAt = DateTime.now();
    } else if (DateTime.now().difference(changedAt) >= options.stall) {
      throw StateError('remote worktree Turn made no observable progress');
    }
    if (workspace == null) {
      await Future<void>.delayed(const Duration(milliseconds: 300));
      continue;
    }
    final interaction = workspace['activeInteraction'];
    if (interaction is Map &&
        !handledInteractions.contains(interaction['id'])) {
      final kind = interaction['kind'];
      if (kind == 'toolApproval') {
        final finder = find.byValueKey('tool-approve');
        await driver.waitFor(finder, timeout: const Duration(seconds: 30));
        await driver.sendCommand(RawTap(finder));
        handledInteractions.add(interaction['id']);
      } else if (kind == 'userInput') {
        for (final key in ['user-input-first-option', 'user-input-submit']) {
          final finder = find.byValueKey(key);
          await driver.waitFor(finder, timeout: const Duration(seconds: 30));
          await driver.sendCommand(RawTap(finder));
        }
        handledInteractions.add(interaction['id']);
      } else {
        throw StateError('unexpected interaction during Turn: $interaction');
      }
      continue;
    }
    if (workspace['turn'] != null) {
      sawTurn = true;
      await Future<void>.delayed(const Duration(milliseconds: 300));
      continue;
    }
    final lastTurn = workspace['lastTurn'] as Map?;
    final status = lastTurn?['status'];
    if (sawTurn && status == 'completed') {
      return last;
    }
    if (status == 'failed' ||
        status == 'cancelled' ||
        status == 'budgetLimited') {
      throw StateError('Turn ended as $status: $lastTurn');
    }
    await Future<void>.delayed(const Duration(milliseconds: 300));
  }
  throw StateError('remote worktree Turn timed out; last=$last');
}

class _Options {
  const _Options({
    required this.vmServiceUrl,
    required this.outputDir,
    required this.sshHost,
    required this.sshPort,
    required this.sshUsername,
    required this.sshAlias,
    required this.sshIdentity,
    required this.remoteRepository,
    required this.prompt,
    required this.promptMarker,
    required this.timeout,
    required this.stall,
  });

  final String vmServiceUrl;
  final String outputDir;
  final String sshHost;
  final int sshPort;
  final String sshUsername;
  final String sshAlias;
  final String sshIdentity;
  final String remoteRepository;
  final String prompt;
  final String promptMarker;
  final Duration timeout;
  final Duration stall;

  static _Options parse(List<String> arguments) {
    final values = <String, String>{};
    for (var index = 0; index < arguments.length; index += 2) {
      final name = arguments[index];
      if (!name.startsWith('--') || index + 1 >= arguments.length) {
        throw ArgumentError(
          'expected --name value pairs, got ${arguments[index]}',
        );
      }
      values[name.substring(2)] = arguments[index + 1];
    }
    String required(String name) {
      final value = values[name];
      if (value == null || value.isEmpty) {
        throw ArgumentError('--$name is required');
      }
      return value;
    }

    return _Options(
      vmServiceUrl: required('vm-service-url'),
      outputDir: required('output-dir'),
      sshHost: required('ssh-host'),
      sshPort: int.parse(values['ssh-port'] ?? '22'),
      sshUsername: required('ssh-username'),
      sshAlias: values['ssh-alias'] ?? 'n5-remote-worktree',
      sshIdentity: required('ssh-identity'),
      remoteRepository: required('remote-repository'),
      prompt: required('prompt'),
      promptMarker: required('prompt-marker'),
      timeout: Duration(seconds: int.parse(values['timeout-seconds'] ?? '900')),
      stall: Duration(seconds: int.parse(values['stall-seconds'] ?? '360')),
    );
  }
}
