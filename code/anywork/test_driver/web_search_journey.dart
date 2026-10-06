// Manual native-GUI web-search + MCP isolation probe, not a Flutter test gate.
//
// It drives the real GUI through the shared Flutter Driver session against the
// native web-search + MCP acceptance fixture: it opens an isolated project, runs
// a model turn on each fixture session model, drives the real Web Search and
// DeepSeek settings cards through their exact widget keys, and records canonical
// snapshots, screenshots, widget trees and the Driver-only search / MCP state
// projections at every stage.
//
// The probe never trusts the visible UI in place of behavior: a required exact
// key that is missing fails the run, a save is only accepted once the canonical
// `search-settings` projection reflects it, and a new answer is only accepted
// when this Turn produced a new, durable final-answer row on a new Turn id.
// Every capture writes a real screenshot and widget tree; a missing screenshot
// fails the run instead of being reported as complete.
//
// Usage (named form; the native acceptance coordinator uses this):
//   dart run test_driver/web_search_journey.dart \
//     --phase=first --vm=<VM_URL> --project=<DIR> --output=<DIR> [--coord=<DIR>] \
//     [--probe-mode=fixture|real] [--provider-ids=a,b,c] [--models=x,y,z]
//
// Positional fallback (single `first` phase, fixture providers):
//   dart run test_driver/web_search_journey.dart VM_URL PROJECT_DIR OUTPUT_DIR
//
// `--probe-mode=real` reuses the same probe against a real-provider GUI the
// coordinator launched (`cargo xtask run-gui --driver`): prompts are natural and
// the probe waits for whatever this Turn's new answer is instead of the
// fixture-only markers, and it never claims the fixture endpoints passed.
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';
import 'realtime_journey.dart'
    show isDurableSettled, timelineRows, turnId, turnStatus, workspaceOf;

// Prompt markers understood by the fixture model endpoint. Byte-identical to
// the Rust contract in `pl-provider-fixture::web_search`.
const _markOpenAi = '[[openai_search]]';
const _markDeepSeek = '[[deepseek_search]]';
const _markMcp = '[[mcp_search]]';
const _markExpectSearch = '[[expect_search]]';
const _markExpectNoSearch = '[[expect_no_search]]';
const _markSlow = '[[slow_response]]';
// Root-side child flow marker. The fixture embeds its own child marker in the
// `spawn_agent` message, so the probe only submits this one.
const _markSpawnChild = '[[spawn_child]]';
// Unique nonce embedded only in the restart MCP prompt. The coordinator uses it
// to locate that Turn's model request in the fixture log and prove the restart
// really issued a fresh MCP discover/tools/call instead of answering from a
// previous Turn's identical call id.
const _restartMcpNonce = 'ws-restart-mcp';

// The web-search surface a child Thread must assemble next to ordinary tools.
// `mcp__*` is matched by prefix because the real MCP tool name carries its
// server id.
const _searchToolNames = <String>{
  'web_search',
  'deepseek_web_search',
  'discover_tools',
};

bool _isSearchToolName(String name) =>
    _searchToolNames.contains(name) || name.startsWith('mcp__');

const _defaultProviders = <String>[
  'gui-fixture-1-openai',
  'gui-fixture-2-deepseek',
  'gui-fixture-3-zhipu',
];
const _defaultModels = <String>[
  'fixture-openai',
  'fixture-deepseek',
  'fixture-glm',
];
const _sessionLabels = <String>['openai', 'deepseek', 'glm'];
const _toolMarks = <String>[_markOpenAi, _markDeepSeek, _markMcp];

enum _ProbeMode { fixture, real }

typedef _SettingsPredicate = bool Function(
  Map<String, dynamic> web,
  Map<String, dynamic> deep,
);

Future<void> main(List<String> args) async {
  late final _Config config;
  try {
    config = _Config.parse(args);
  } on ArgumentError catch (error) {
    stderr.writeln('$error');
    stderr.writeln(
      'usage: web_search_journey.dart --vm URL --project DIR --output DIR '
      '[--coord DIR] [--phase first|restart] [--probe-mode fixture|real] '
      '[--provider-ids a,b,c] [--models x,y,z]',
    );
    exitCode = 64;
    return;
  }
  await Directory(config.output).create(recursive: true);
  await Directory(config.coord).create(recursive: true);
  final driver = await FlutterDriverSession.connect(vmServiceUrl: config.vm);
  final probe = _Probe(driver, config);
  Object? failure;
  StackTrace? failureStack;
  var shutdown = 'failed';
  try {
    await probe.run();
  } catch (error, stack) {
    failure = error;
    failureStack = stack;
    await probe.captureFailure();
  }
  try {
    final reply = jsonDecode(
      await driver.requestData(
        'shutdown',
        timeout: const Duration(seconds: 60),
      ),
    );
    if (reply is! Map || reply['shutdown'] != 'completed') {
      throw StateError('native shutdown did not complete');
    }
    shutdown = 'completed';
  } catch (error, stack) {
    failure ??= error;
    failureStack ??= stack;
  } finally {
    try {
      await driver.close().timeout(const Duration(seconds: 5));
    } catch (_) {
      // A completed native shutdown may close the VM connection first.
    }
  }
  await probe.writeSummary(
    status: failure == null ? 'complete' : 'failed',
    shutdown: shutdown,
    error: failure?.toString(),
  );
  if (failure != null) {
    Error.throwWithStackTrace(failure, failureStack!);
  }
}

class _Config {
  _Config({
    required this.phase,
    required this.vm,
    required this.project,
    required this.output,
    required this.coord,
    required this.mode,
    required this.providers,
    required this.models,
    required this.allowTurnFailure,
  });

  final String phase;
  final String vm;
  final String project;
  final String output;
  final String coord;
  final _ProbeMode mode;
  final List<String> providers;
  final List<String> models;

  /// Fault runs may legitimately end a tool Turn as failed/cancelled; the probe
  /// records that outcome instead of treating it as a missing answer.
  final bool allowTurnFailure;

  static _Config parse(List<String> args) {
    final flags = <String, String>{};
    final positional = <String>[];
    for (final arg in args) {
      if (arg.startsWith('--')) {
        final separator = arg.indexOf('=');
        if (separator > 2) {
          flags[arg.substring(2, separator)] = arg.substring(separator + 1);
        }
      } else {
        positional.add(arg);
      }
    }
    String requireFlag(String name) {
      final value = flags[name];
      if (value == null || value.isEmpty) {
        throw ArgumentError('--$name is required');
      }
      return value;
    }

    final phase = flags['phase'] ?? 'first';
    if (phase != 'first' && phase != 'restart') {
      throw ArgumentError('--phase must be first or restart');
    }
    late final String vm;
    late final String project;
    late final String output;
    if (flags.containsKey('vm')) {
      vm = requireFlag('vm');
      project = requireFlag('project');
      output = requireFlag('output');
    } else {
      if (positional.length != 3) {
        throw ArgumentError(
          'expected VM_URL PROJECT_DIR OUTPUT_DIR or named --vm/--project/--output',
        );
      }
      vm = positional[0];
      project = positional[1];
      output = positional[2];
    }
    final mode = (flags['probe-mode'] ?? 'fixture') == 'real'
        ? _ProbeMode.real
        : _ProbeMode.fixture;
    final providers = _splitList(flags['provider-ids'], _defaultProviders);
    final models = _splitList(flags['models'], _defaultModels);
    if (providers.length != 3 || models.length != 3) {
      throw ArgumentError('exactly three --provider-ids and three --models');
    }
    return _Config(
      phase: phase,
      vm: vm,
      project: project,
      output: output,
      coord: flags['coord'] ?? output,
      mode: mode,
      providers: providers,
      models: models,
      allowTurnFailure: flags['allow-turn-failure'] == 'true',
    );
  }
}

List<String> _splitList(String? raw, List<String> fallback) {
  if (raw == null || raw.trim().isEmpty) return List<String>.of(fallback);
  return raw
      .split(',')
      .map((value) => value.trim())
      .where((value) => value.isNotEmpty)
      .toList();
}

class _Probe {
  _Probe(this.driver, this.config);

  final FlutterDriverSession driver;
  final _Config config;
  final Map<String, Object?> _controls = {};
  final List<Object?> _turns = [];
  final List<Object?> _captures = [];
  int _captureIndex = 0;
  Map<String, Object?>? _finalSettings;
  Map<String, Object?>? _finalMcp;
  Map<String, Object?>? _child;

  bool get _fixture => config.mode == _ProbeMode.fixture;

  File get _observed => File('${config.coord}/observed.json');

  Future<Map<String, dynamic>> snapshot() => driver.readSnapshot();

  Future<void> stage(String name) =>
      File('${config.output}/web-search-${config.phase}-stage.txt')
          .writeAsString(name);

  Future<Map<String, dynamic>> searchSettings() async {
    final raw = await driver.requestData(
      'search-settings',
      timeout: const Duration(seconds: 15),
    );
    final decoded = jsonDecode(raw);
    if (decoded is! Map || decoded['ok'] != true) {
      throw StateError('search-settings projection unavailable: $raw');
    }
    return decoded.cast<String, dynamic>();
  }

  Future<Map<String, dynamic>> searchMcpState() async {
    final raw = await driver.requestData(
      'search-mcp-state',
      timeout: const Duration(seconds: 15),
    );
    final decoded = jsonDecode(raw);
    if (decoded is! Map || decoded['ok'] != true) {
      throw StateError('search-mcp-state projection unavailable: $raw');
    }
    return decoded.cast<String, dynamic>();
  }

  static String _settingsFingerprint(Map<String, dynamic> settings) =>
      jsonEncode({
        'webSearch': settings['webSearch'],
        'deepSeekWebSearch': settings['deepSeekWebSearch'],
      });

  String? get _finalFingerprint {
    final settings = _finalSettings;
    if (settings == null) return null;
    return _settingsFingerprint(settings.cast<String, dynamic>());
  }

  Future<void> capture(String name, {bool required = true}) async {
    final ordinal = _captureIndex++;
    final prefix = '${config.phase}-$ordinal-$name';
    final record = <String, Object?>{'ordinal': ordinal, 'name': name};
    final state = await snapshot();
    await File(
      '${config.output}/snapshot-$prefix.json',
    ).writeAsString('${const JsonEncoder.withIndent('  ').convert(state)}\n');
    record['snapshot'] = 'snapshot-$prefix.json';
    try {
      await File('${config.output}/screenshot-$prefix.png')
          .writeAsBytes(await driver.screenshot());
      record['screenshot'] = true;
    } catch (error) {
      record['screenshot'] = false;
      record['screenshotError'] = '$error';
      _captures.add(record);
      if (required) {
        throw StateError('required screenshot "$name" failed: $error');
      }
      return;
    }
    try {
      await File('${config.output}/tree-$prefix.txt')
          .writeAsString(await driver.renderTree());
      record['tree'] = true;
    } catch (error) {
      record['tree'] = false;
      record['treeError'] = '$error';
      _captures.add(record);
      if (required) {
        throw StateError('required widget tree "$name" failed: $error');
      }
      return;
    }
    try {
      await File('${config.output}/search-settings-$prefix.json').writeAsString(
        '${const JsonEncoder.withIndent('  ').convert(await searchSettings())}\n',
      );
    } catch (error) {
      record['searchSettingsError'] = '$error';
    }
    try {
      await File(
        '${config.output}/search-mcp-state-$prefix.json',
      ).writeAsString(
        '${const JsonEncoder.withIndent('  ').convert(await searchMcpState())}\n',
      );
    } catch (error) {
      record['searchMcpStateError'] = '$error';
    }
    _captures.add(record);
  }

  Future<void> captureFailure() async {
    try {
      await capture('failure', required: false);
    } catch (_) {
      // The window may already be gone; the original failure stands.
    }
  }

  Future<void> writeSummary({
    required String status,
    required String shutdown,
    String? error,
  }) async {
    await File('${config.output}/${config.phase}-summary.json').writeAsString(
      '${const JsonEncoder.withIndent('  ').convert({'phase': config.phase, 'probeMode': config.mode.name, 'providers': config.providers, 'models': config.models, 'status': status, 'shutdown': shutdown, 'humanVerdict': 'pending', 'settings': _finalSettings, 'settingsFingerprint': _finalFingerprint, 'mcp': _finalMcp, 'child': _child, 'controls': _controls, 'turns': _turns, 'captures': _captures, 'error': error})}\n',
    );
  }

  Future<void> run() async {
    await stage('connected');
    final baseline = await snapshot();
    _controls['baseline'] = {'hasWorkspace': workspaceOf(baseline) != null};
    await capture('baseline');
    if (config.phase == 'first') {
      await _runFirst();
    } else {
      await _runRestart();
    }
    await stage('completed');
    stdout.writeln(
      'web-search ${config.phase} journey completed; human verdict pending.',
    );
  }

  Future<void> _runFirst() async {
    await _openProject();
    await capture('project-open');
    await _waitForMcpSettled();
    await capture('mcp-settled');

    for (var index = 0; index < 3; index++) {
      final label = _sessionLabels[index];
      await _selectModel(index);
      final settings = await searchSettings();
      _controls['session-$label'] = {
        'providerId': config.providers[index],
        'model': config.models[index],
        'webSearch': settings['webSearch'],
        'deepSeekWebSearch': settings['deepSeekWebSearch'],
      };
      await capture('session-$label');
      if (_fixture) {
        await _turn(
          'search-tools-$label',
          '$_markExpectSearch session $label',
          marker: _markExpectSearch,
        );
        await _turn(
          'tool-$label',
          '${_toolMarks[index]} session $label',
          marker: _toolMarks[index],
        );
      } else {
        await _turn(
          'tool-$label',
          'Use the available web search tool to find one current fact about '
              'the Flutter framework and cite the source URL.',
        );
      }
      await _recordTools('used-$label');
      await capture('turn-$label');
    }

    // Child Thread tool assembly: spawn a real child through the production
    // `spawn_agent` tool while search is still enabled, then prove the child
    // assembled and called the same three-tool surface next to ordinary tools.
    await _childSearchProbe();

    // Hot refresh: disable both search services through the real cards, prove
    // the next Turn no longer declares either tool, then restore and recover.
    await _openGeneralSettings();
    await _setMode('disabled');
    await _toggleDeepSeek(false);
    await _focus('deepseek_web_search_settings');
    await capture('ds-card-off');
    await _leaveSettings();
    if (_fixture) {
      await _turn(
        'no-search-tools',
        '$_markExpectNoSearch no search tools',
        marker: _markExpectNoSearch,
      );
      await _recordTools('no-search-tools');
    }

    await _openGeneralSettings();
    await _setMode('live');
    await _toggleDeepSeek(true);
    await capture('ds-card-on');
    await _leaveSettings();
    if (_fixture) {
      await _turn(
        'search-tools-restored',
        '$_markExpectSearch recovered',
        marker: _markExpectSearch,
      );
      await _recordTools('search-tools-restored');
    }

    await _editWebSearchSettings();
    await _settingsPersistenceRoundTrip();
    await _cancelRoundTrip();

    final state = await snapshot();
    final threadId = workspaceOf(state)?['threadId'];
    if (threadId is! String || threadId.isEmpty) {
      throw StateError('canonical Thread id missing before saving observed');
    }
    final settings = await searchSettings();
    _finalSettings = settings;
    _finalMcp = await searchMcpState();
    await _observed.writeAsString(
      jsonEncode({
        'threadId': threadId,
        'settings': settings,
        'settingsFingerprint': _settingsFingerprint(settings),
      }),
    );
  }

  Future<void> _runRestart() async {
    final observed =
        jsonDecode(await _observed.readAsString()) as Map<String, dynamic>;
    final threadId = observed['threadId'] as String;
    await _requireKey(
      'thread-row-$threadId',
      timeout: const Duration(seconds: 120),
    );
    await _tap('thread-row-$threadId');
    await _waitUntil(
      (state) =>
          workspaceOf(state)?['threadId'] == threadId &&
          isDurableSettled(state),
      'restored-session',
      timeout: const Duration(seconds: 120),
    );
    await capture('restored');
    final settings = await searchSettings();
    final expectedFingerprint =
        observed['settingsFingerprint'] as String? ??
        _settingsFingerprint(
          (observed['settings'] as Map).cast<String, dynamic>(),
        );
    if (_settingsFingerprint(settings) != expectedFingerprint) {
      throw StateError(
        'canonical web-search settings changed across restart: '
        '${_settingsFingerprint(settings)} != $expectedFingerprint',
      );
    }
    _finalSettings = settings;
    _finalMcp = await searchMcpState();
    _controls['restart'] = {
      'threadId': threadId,
      'settingsFingerprint': _settingsFingerprint(settings),
    };
    if (_fixture) {
      await _turn(
        'restart-search-tools',
        '$_markExpectSearch after restart',
        marker: _markExpectSearch,
      );
      await _turn(
        'restart-mcp',
        '$_markMcp $_restartMcpNonce after restart',
        marker: _markMcp,
      );
      await _recordTools('restart-mcp');
    }
    await capture('restart-complete');
  }

  // -------------------------------------------------------------------------
  // Driver primitives.

  SerializableFinder _key(String key) => find.byValueKey(key);

  Future<void> _requireKey(
    String key, {
    Duration timeout = const Duration(seconds: 60),
  }) async {
    try {
      await driver.waitFor(_key(key), timeout: timeout);
    } catch (error) {
      throw StateError('required exact widget key "$key" not found: $error');
    }
  }

  /// Scrolls the settings pane until [key] is laid out, trying downward first.
  Future<void> _focus(String key) async {
    try {
      await driver.scrollUntilVisible(
        _key('settings-pane-scroll'),
        _key(key),
        dyScroll: -220,
        timeout: const Duration(seconds: 45),
      );
      return;
    } catch (_) {
      // The control may be above the current viewport.
    }
    await driver.scrollUntilVisible(
      _key('settings-pane-scroll'),
      _key(key),
      dyScroll: 220,
      timeout: const Duration(seconds: 45),
    );
  }

  Future<void> _tap(String key) async {
    await _requireKey(key);
    await driver.rawTap(_key(key));
    await Future<void>.delayed(const Duration(milliseconds: 200));
  }

  Future<void> _tapControl(String key) async {
    await _focus(key);
    await _requireKey(key);
    await driver.rawTap(_key(key));
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }

  Future<Map<String, dynamic>> _waitUntil(
    bool Function(Map<String, dynamic> state) predicate,
    String label, {
    Duration timeout = const Duration(seconds: 180),
  }) async {
    final deadline = DateTime.now().add(timeout);
    Map<String, dynamic>? last;
    while (DateTime.now().isBefore(deadline)) {
      final state = await snapshot();
      last = state;
      if (predicate(state)) return state;
      await Future<void>.delayed(const Duration(milliseconds: 250));
    }
    final summary = <String, Object?>{
      'busy': workspaceOf(last ?? <String, dynamic>{})?['isBusy'],
      'turnId': last == null ? null : turnId(last),
      'turnStatus': last == null ? null : turnStatus(last),
    };
    throw StateError('$label timed out: ${jsonEncode(summary)}');
  }

  // -------------------------------------------------------------------------
  // Project + session actions.

  Future<void> _openProject() async {
    await stage('open-project');
    await _tap('sidebar-open-project');
    await _tap('add-project-local');
    await _requireKey(
      'add-project-continue-ready',
      timeout: const Duration(seconds: 30),
    );
    await _tap('add-project-continue-ready');
    await _requireKey(
      'project-path-input',
      timeout: const Duration(seconds: 30),
    );
    await _tap('project-path-input');
    await driver.enterText(config.project);
    await _tap('project-path-submit');
    await _requireKey('composer-input', timeout: const Duration(seconds: 60));
  }

  Future<void> _selectModel(int index) async {
    final providerId = config.providers[index];
    final model = config.models[index];
    await _tap('model-selector');
    final optionKey = 'model-${jsonEncode(<String>[providerId, model])}';
    final option = _key(optionKey);
    // The model catalog can be long, so the option may be built but laid out
    // outside the menu viewport: existence is not clickability. Scroll the
    // menu's own `Scrollable` ancestor (the option is a descendant of it) so
    // the row is really on screen before the raw tap.
    await _requireKey(optionKey, timeout: const Duration(seconds: 30));
    final menuScrollable = find.ancestor(
      of: option,
      matching: find.byType('Scrollable'),
      firstMatchOnly: true,
    );
    await driver.scrollUntilVisible(
      menuScrollable,
      option,
      dyScroll: -160,
      timeout: const Duration(seconds: 20),
    );
    await driver.rawTap(option);
    await _waitUntil(
      (state) => _modelSelectionApplied(state, providerId, model),
      'session-model-$index',
      timeout: const Duration(seconds: 60),
    );
  }

  /// Whether the requested session model is really the canonical selection.
  ///
  /// The start page has no root workspace yet, so the only canonical fact is
  /// the settings route table (`settings.modeModelRoutes`), which the
  /// start-page selector writes through `setModeModelRoute`. Once a root
  /// exists the durable workspace route is the fact the next Turn will use, so
  /// a fast submit cannot race an asynchronous route change that has not been
  /// persisted yet.
  bool _modelSelectionApplied(
    Map<String, dynamic> state,
    String providerId,
    String model,
  ) {
    final workspace = workspaceOf(state);
    if (workspace == null) {
      // Start page: the exact mode the next session will use is
      // `navigation.newThreadMode` (full id such as `mode.simple`), so only that
      // mode's route may satisfy the requested provider/model. Matching any
      // route would accept an unrelated mode's stale route.
      final navigation = state['navigation'];
      final modeId = navigation is Map ? navigation['newThreadMode'] : null;
      if (modeId is! String || modeId.isEmpty) return false;
      final settings = state['settings'];
      final routes = settings is Map ? settings['modeModelRoutes'] : null;
      if (routes is! List) return false;
      // The route write must also be committed, so a fast submit cannot race an
      // asynchronous `setModeModelRoute` that has only updated the draft.
      return _persistenceSettled(state) &&
          routes.whereType<Map>().any(
            (route) =>
                route['modeId'] == modeId &&
                route['providerId'] == providerId &&
                route['model'] == model,
          );
    }
    final route = workspace['modelRoute'];
    return isDurableSettled(state) &&
        route is Map &&
        route['providerId'] == providerId &&
        route['model'] == model;
  }

  /// Whether the persistence queue reports no pending commits.
  bool _persistenceSettled(Map<String, dynamic> state) {
    final persistence = state['persistence'];
    return persistence is Map &&
        persistence['kind'] == 'ready' &&
        persistence['pendingCommits'] == 0;
  }

  Future<void> _submit(String prompt) async {
    await _tap('composer-input');
    await driver.enterText(prompt);
    await _tap('composer-submit');
  }

  /// Submits [prompt] and waits for *this* Turn's new, durable final answer.
  ///
  /// Before submitting it records the Turn id, the newest user-input id, the
  /// user-message row ids and every row id, so an older answer (even one with
  /// identical text) can never be mistaken for this one: the wait requires a
  /// new Turn with a new input id, a new user message row, a new final-answer
  /// row *after* it, a non-pending composer submission and a settled
  /// persistence queue.
  Future<String> _submitTurn({
    required String prompt,
    String? marker,
    Duration timeout = const Duration(seconds: 240),
  }) async {
    final before = await snapshot();
    final beforeTurn = turnId(before);
    final beforeInput = _turnInputId(before);
    final beforeUserIds = _userMessageIds(before);
    final beforeIds = <String>{
      for (final row in timelineRows(before)) '${row['id']}',
    };
    await _submit(prompt);
    final deadline = DateTime.now().add(timeout);
    Map<String, dynamic>? last;
    while (DateTime.now().isBefore(deadline)) {
      final state = await snapshot();
      last = state;
      // Each fact is checked separately: a *new* Turn carrying a *new* user
      // input id, the new user message row present, the composer submission
      // finished, no busy Turn and a settled persistence queue. An older
      // answer that happens to share text can satisfy none of the new ids.
      final rows = timelineRows(state);
      final currentTurn = turnId(state);
      final currentInput = _turnInputId(state);
      var newUserIndex = -1;
      for (var index = 0; index < rows.length; index++) {
        final row = rows[index];
        if (row['type'] == 'userMessage' &&
            row['id'] is String &&
            !beforeUserIds.contains('${row['id']}')) {
          newUserIndex = index;
        }
      }
      final currentInputIsNew =
          currentInput != null && currentInput != beforeInput;
      final turnChanged = currentTurn != null && currentTurn != beforeTurn;
      if (isDurableSettled(state) && !_submissionPending(state)) {
        final status = turnStatus(state);
        // A fault run may legitimately end a tool Turn as failed/cancelled; the
        // probe records that outcome instead of waiting for a missing answer.
        if (config.allowTurnFailure &&
            turnChanged &&
            (status == 'failed' || status == 'cancelled')) {
          final turn = workspaceOf(state)?['turn'];
          final reason = turn is Map ? '${turn['reason'] ?? ''}' : '';
          return '<turn $status${reason.isEmpty ? '' : ': $reason'}>';
        }
      }
      if (isDurableSettled(state) &&
          !_submissionPending(state) &&
          turnChanged &&
          currentInputIsNew &&
          newUserIndex >= 0) {
        final added = <String>[
          for (var index = 0; index < rows.length; index++)
            if (rows[index]['type'] == 'finalAnswer' &&
                !beforeIds.contains('${rows[index]['id']}') &&
                index > newUserIndex)
              '${rows[index]['text'] ?? ''}',
        ];
        if (added.isNotEmpty) {
          final text = added.join('\n');
          if (marker == null || text.contains(marker)) {
            return text;
          }
        }
      }
      await Future<void>.delayed(const Duration(milliseconds: 300));
    }
    final summary = <String, Object?>{
      'beforeTurn': beforeTurn,
      'beforeInput': beforeInput,
      'turnId': last == null ? null : turnId(last),
      'turnInput': last == null ? null : _turnInputId(last),
      'turnStatus': last == null ? null : turnStatus(last),
      'busy': workspaceOf(last ?? <String, dynamic>{})?['isBusy'],
      'submissionPending': last == null ? null : _submissionPending(last),
    };
    throw StateError(
      'this Turn never produced a new durable answer for "$prompt": '
      '${jsonEncode(summary)}',
    );
  }

  /// The user-input id of the newest observed Turn (the active turn first, then
  /// the last completed turn), or null when this workspace has no Turn yet.
  ///
  /// The Driver snapshot exposes `inputId` on the active/last Turn and on user
  /// timeline rows; it does not link an individual final-answer row to a Turn,
  /// so this id plus the user-row ordering below is the strongest snapshot-only
  /// linkage of a new answer to its own submission.
  String? _turnInputId(Map<String, dynamic> state) {
    final workspace = workspaceOf(state);
    for (final key in const ['turn', 'lastTurn']) {
      final turn = workspace?[key];
      if (turn is Map && turn['inputId'] is String) {
        return turn['inputId'] as String;
      }
    }
    return null;
  }

  /// User-message row ids currently present in the timeline window.
  Set<String> _userMessageIds(Map<String, dynamic> state) => {
    for (final row in timelineRows(state))
      if (row['type'] == 'userMessage' && row['id'] is String)
        row['id'] as String,
  };

  /// Whether the composer is still submitting the input.
  bool _submissionPending(Map<String, dynamic> state) {
    final composer = workspaceOf(state)?['composer'];
    return composer is Map && composer['submissionPending'] == true;
  }

  Future<void> _turn(
    String name,
    String prompt, {
    String? marker,
    Duration timeout = const Duration(seconds: 240),
  }) async {
    await stage('turn-$name');
    final text = await _submitTurn(
      prompt: prompt,
      marker: marker,
      timeout: timeout,
    );
    _turns.add({
      'name': name,
      'prompt': prompt,
      'marker': marker,
      'answer': text,
    });
  }

  Future<void> _recordTools(String name) async {
    final state = await snapshot();
    _turns.add({'name': 'tools-$name', 'tools': _toolNames(state)});
  }

  /// Canonical child Agents currently projected for the selected root Thread.
  List<Map<String, dynamic>> _agents(Map<String, dynamic> state) {
    final agents = workspaceOf(state)?['agents'];
    return agents is List
        ? agents.whereType<Map<String, dynamic>>().toList()
        : const <Map<String, dynamic>>[];
  }

  /// Spawns a real child Agent, opens its canonical Thread, and records the
  /// tools it actually assembled and called.
  ///
  /// Only fixture mode automates the child: the fixture model endpoint scripts
  /// the real `spawn_agent`/`wait` and the child's search/discover/MCP calls, so
  /// a real provider GUI has no deterministic child turn and the probe records
  /// that honestly instead of inventing one.
  Future<void> _childSearchProbe() async {
    // Fault runs deliberately break a search/MCP path, so the child would never
    // assemble the full surface: the fault run records that honestly instead of
    // forcing the happy-path child probe to time out.
    if (!_fixture || config.allowTurnFailure) {
      _controls['childSearch'] = !_fixture
          ? 'skipped-real-mode'
          : 'skipped-fault-run';
      return;
    }
    await stage('child-search');
    final before = await snapshot();
    final rootThreadId = workspaceOf(before)?['threadId'] as String?;
    if (rootThreadId == null || rootThreadId.isEmpty) {
      throw StateError('root Thread id missing before spawning the child');
    }
    // Real Turn: the fixture endpoint asks for `spawn_agent`, then `wait`, so the
    // answer only lands once the child's terminal notification was consumed.
    await _turn(
      'child-spawn',
      '$_markSpawnChild run the fixture child tool surface and await it',
      timeout: const Duration(seconds: 300),
    );
    final spawned = await _waitUntil(
      (state) =>
          _agents(state).any((agent) => agent['threadId'] != rootThreadId) &&
          isDurableSettled(state),
      'child-agent-created',
      timeout: const Duration(seconds: 120),
    );
    final child = _agents(spawned).firstWhere(
      (agent) =>
          agent['threadId'] is String && agent['threadId'] != rootThreadId,
      orElse: () => const <String, dynamic>{},
    );
    final childThreadId = child['threadId'] as String?;
    if (childThreadId == null || childThreadId.isEmpty) {
      throw StateError('canonical child Thread id missing after spawn_agent');
    }
    _controls['childAgent'] = {
      'threadId': childThreadId,
      'rootThreadId': child['rootThreadId'],
      'role': child['role'],
      'task': child['task'],
      'status': child['status'],
    };

    await _selectAgentThread(childThreadId);
    await capture('child-thread');
    // The child's own rows prove which tools it really called; wait until the
    // full surface is present instead of sampling once and accepting a partial
    // timeline.
    final childState = await _waitUntil(
      (state) =>
          _toolNames(state).toSet().containsAll(_searchToolNames) &&
          _toolNames(state).any((name) => name.startsWith('mcp__')),
      'child-tools-complete',
      timeout: const Duration(seconds: 90),
    );
    final toolNames = _toolNames(childState);
    final callIds = _toolCallIds(childState);
    _turns.add({'name': 'tools-child', 'tools': toolNames});
    _child = {
      'threadId': childThreadId,
      'rootThreadId': rootThreadId,
      'role': child['role'],
      'status': child['status'],
      'turnId': turnId(childState),
      'inputId': _turnInputId(childState),
      'toolNames': toolNames,
      'toolCallIds': callIds,
      // The child calls only the search/discover/MCP surface, so an ordinary
      // tool normally appears as a *declaration* (proven by the fixture log)
      // rather than a call row; record both facts without conflating them.
      'hasNormalToolRow': toolNames.any((name) => !_isSearchToolName(name)),
    };
    await capture('child-tools');

    // Return to the root Thread so the rest of the journey edits the root
    // session's settings and composer.
    await _selectAgentThread(rootThreadId);
    await _waitUntil(
      (state) =>
          workspaceOf(state)?['threadId'] == rootThreadId &&
          isDurableSettled(state),
      'root-thread-restored',
      timeout: const Duration(seconds: 60),
    );
  }

  Future<void> _openAgentMenu() async {
    await _tap('agent-switcher');
  }

  Future<void> _selectAgentThread(String threadId) async {
    await _openAgentMenu();
    final rowKey = 'agent-thread-$threadId';
    await _requireKey(rowKey, timeout: const Duration(seconds: 30));
    final row = _key(rowKey);
    // The agent list can be longer than its menu viewport, so scroll the menu's
    // own Scrollable ancestor before the raw tap instead of trusting existence.
    final menuScrollable = find.ancestor(
      of: row,
      matching: find.byType('Scrollable'),
      firstMatchOnly: true,
    );
    await driver.scrollUntilVisible(
      menuScrollable,
      row,
      dyScroll: -120,
      timeout: const Duration(seconds: 20),
    );
    await driver.rawTap(row);
    await _waitUntil(
      (state) => workspaceOf(state)?['threadId'] == threadId,
      'agent-thread-$threadId-selected',
      timeout: const Duration(seconds: 60),
    );
  }

  List<String> _toolNames(Map<String, dynamic> state) => <String>[
    for (final row in timelineRows(state))
      for (final tool in (row['tools'] as List? ?? const []))
        if (tool is Map && tool['name'] is String) '${tool['name']}',
  ];

  List<String> _toolCallIds(Map<String, dynamic> state) => <String>[
    for (final row in timelineRows(state))
      for (final tool in (row['tools'] as List? ?? const []))
        if (tool is Map && tool['callId'] is String) '${tool['callId']}',
  ];

  // -------------------------------------------------------------------------
  // Settings cards.

  Future<void> _openGeneralSettings() async {
    await stage('open-settings');
    await _tap('settings-open');
    await _requireKey('settings-page');
    await _tap('settings-tab-general');
    await _requireKey(
      'web_search_settings',
      timeout: const Duration(seconds: 30),
    );
    await _focus('web_search_settings');
  }

  Future<void> _leaveSettings() async {
    await stage('leave-settings');
    await _tap('settings-back');
    await _requireKey('composer-input');
  }

  /// Opens the select menu and activates the exact item key.
  ///
  /// The menu must expose the item key; the probe never dismisses a menu with a
  /// stray key to fall back to a stale value.
  Future<void> _selectOption(String fieldKey, String itemKey) async {
    await _tapControl(fieldKey);
    await _requireKey(itemKey, timeout: const Duration(seconds: 15));
    await driver.rawTap(_key(itemKey));
    await Future<void>.delayed(const Duration(milliseconds: 300));
    _controls[fieldKey] = {'item': itemKey, 'found': true};
  }

  Future<void> _setText(String fieldKey, String value) async {
    await _tapControl(fieldKey);
    await driver.enterText(value);
    await Future<void>.delayed(const Duration(milliseconds: 200));
    _controls[fieldKey] = {'value': value, 'found': true};
  }

  /// Clicks `web_search_save` and waits until the canonical projection matches.
  Future<Map<String, dynamic>> _saveWebSearch({
    required _SettingsPredicate accept,
    required String label,
  }) async {
    await _tapControl('web_search_save');
    final deadline = DateTime.now().add(const Duration(seconds: 60));
    Map<String, dynamic>? last;
    while (DateTime.now().isBefore(deadline)) {
      final settings = await searchSettings();
      last = settings;
      final web = (settings['webSearch'] as Map).cast<String, dynamic>();
      final deep = (settings['deepSeekWebSearch'] as Map)
          .cast<String, dynamic>();
      if (accept(web, deep)) return settings;
      await Future<void>.delayed(const Duration(milliseconds: 200));
    }
    throw StateError(
      'canonical web-search settings never matched $label: '
      '${jsonEncode(last?['webSearch'])}',
    );
  }

  Future<void> _setMode(String mode) async {
    await _selectOption('web_search_mode', 'web_search_mode_$mode');
    await _saveWebSearch(
      accept: (web, deep) => web['configuredMode'] == mode,
      label: 'mode=$mode',
    );
    _controls['mode'] = mode;
  }

  Future<void> _toggleDeepSeek(bool enabled) async {
    final before = await searchSettings();
    final current =
        ((before['deepSeekWebSearch'] as Map)['configuredEnabled'] as bool?) ??
        false;
    if (current == enabled) {
      _controls['deepseekEnabled'] = enabled;
      return;
    }
    await _tapControl('deepseek_web_search_enabled');
    final deadline = DateTime.now().add(const Duration(seconds: 30));
    while (DateTime.now().isBefore(deadline)) {
      final settings = await searchSettings();
      final deep = (settings['deepSeekWebSearch'] as Map)
          .cast<String, dynamic>();
      if (deep['configuredEnabled'] == enabled) {
        _controls['deepseekEnabled'] = enabled;
        return;
      }
      // The card saves its own command on toggle; only the canonical value
      // proves it, so wait for it instead of trusting the draft.
      await Future<void>.delayed(const Duration(milliseconds: 250));
    }
    throw StateError('DeepSeek web-search toggle never persisted $enabled');
  }

  Future<void> _editWebSearchSettings() async {
    await _openGeneralSettings();
    await stage('edit-settings');
    for (final mode in ['disabled', 'cached', 'indexed', 'live']) {
      await _setMode(mode);
    }
    for (final size in ['default', 'low', 'medium', 'high']) {
      await _selectOption(
        'web_search_context_size',
        'web_search_context_size_$size',
      );
      final Object? expected = size == 'default' ? null : size;
      await _saveWebSearch(
        accept: (web, deep) => web['contextSize'] == expected,
        label: 'context=$size',
      );
      _controls['contextSize'] = expected;
    }
    await _setText(
      'web_search_domains',
      '  example.com ,, example.org , example.com  ',
    );
    await _setText('web_search_country', '  US  ');
    await _setText('web_search_region', '  California  ');
    await _setText('web_search_city', '  San Francisco  ');
    await _setText('web_search_timezone', '  America/Los_Angeles  ');
    await _saveWebSearch(
      accept: (web, deep) =>
          _listEquals(web['allowedDomains'], ['example.com', 'example.org']) &&
          web['country'] == 'US' &&
          web['region'] == 'California' &&
          web['city'] == 'San Francisco' &&
          web['timezone'] == 'America/Los_Angeles',
      label: 'trim/dedupe text fields',
    );
    for (final field in [
      'web_search_domains',
      'web_search_country',
      'web_search_region',
      'web_search_city',
      'web_search_timezone',
    ]) {
      await _setText(field, '');
    }
    await _saveWebSearch(
      accept: (web, deep) =>
          _listEquals(web['allowedDomains'], <String>[]) &&
          web['country'] == null &&
          web['region'] == null &&
          web['city'] == null &&
          web['timezone'] == null,
      label: 'cleared text fields',
    );
    // Leave the canonical card at the values the restart phase must recover.
    await _setMode('live');
    await _selectOption(
      'web_search_context_size',
      'web_search_context_size_medium',
    );
    await _saveWebSearch(
      accept: (web, deep) => web['contextSize'] == 'medium',
      label: 'context=medium',
    );
    await _setText('web_search_domains', 'example.com, example.org');
    await _setText('web_search_country', 'CN');
    await _setText('web_search_region', 'Shanghai');
    await _setText('web_search_city', 'Shanghai');
    await _setText('web_search_timezone', 'Asia/Shanghai');
    _finalSettings = await _saveWebSearch(
      accept: (web, deep) =>
          web['configuredMode'] == 'live' &&
          web['contextSize'] == 'medium' &&
          _listEquals(web['allowedDomains'], ['example.com', 'example.org']) &&
          web['country'] == 'CN' &&
          web['region'] == 'Shanghai' &&
          web['city'] == 'Shanghai' &&
          web['timezone'] == 'Asia/Shanghai',
      label: 'final persisted values',
    );
    await _focus('deepseek_web_search_settings');
    await capture('settings-edited');
  }

  Future<void> _settingsPersistenceRoundTrip() async {
    await stage('persistence-round-trip');
    await _leaveSettings();
    final afterLeave = await searchSettings();
    _requireSettingsEqual(
      afterLeave,
      _finalSettings!,
      'after leaving settings',
    );
    await _openGeneralSettings();
    await _focus('deepseek_web_search_settings');
    await capture('settings-reopened');
    final afterReopen = await searchSettings();
    _requireSettingsEqual(
      afterReopen,
      _finalSettings!,
      'after reopening settings',
    );
  }

  void _requireSettingsEqual(
    Map<String, dynamic> actual,
    Map<String, dynamic> expected,
    String label,
  ) {
    final actualFingerprint = _settingsFingerprint(actual);
    final expectedFingerprint = _settingsFingerprint(expected);
    if (actualFingerprint != expectedFingerprint) {
      throw StateError(
        'canonical web-search settings differ $label: '
        '$actualFingerprint != $expectedFingerprint',
      );
    }
  }

  Future<void> _cancelRoundTrip() async {
    if (!_fixture) {
      _controls['cancel'] = 'skipped-real-mode';
      return;
    }
    await stage('cancel');
    await _leaveSettings();
    await _submit('$_markSlow cancel probe');
    await _waitUntil(
      (state) => workspaceOf(state)?['isBusy'] == true,
      'slow-turn-running',
      timeout: const Duration(seconds: 60),
    );
    await capture('cancel-running');
    await _tap('composer-stop');
    await _waitUntil(
      (state) =>
          isDurableSettled(state) &&
          (turnStatus(state) == 'cancelled' || turnStatus(state) == 'failed'),
      'slow-turn-cancelled',
      timeout: const Duration(seconds: 90),
    );
    await capture('cancel-settled');
  }

  Future<void> _waitForMcpSettled() async {
    final deadline = DateTime.now().add(const Duration(seconds: 60));
    Map<String, dynamic>? last;
    while (DateTime.now().isBefore(deadline)) {
      final mcp = await searchMcpState();
      last = mcp;
      final servers = mcp['servers'];
      if (servers is List && servers.isNotEmpty) {
        final settled = servers.whereType<Map>().every((server) {
          final state = server['state'];
          return state is Map && state['kind'] != 'checking';
        });
        if (settled) break;
      }
      await Future<void>.delayed(const Duration(milliseconds: 300));
    }
    _finalMcp = last;
    _controls['mcpServers'] = [
      for (final server in (last?['servers'] as List? ?? const []))
        if (server is Map)
          {
            'id': server['id'],
            'endpoint': server['endpoint'],
            'state': server['state'],
          },
    ];
  }
}

bool _listEquals(Object? value, List<String> expected) {
  if (value is! List || value.length != expected.length) return false;
  for (var index = 0; index < expected.length; index++) {
    if ('${value[index]}' != expected[index]) return false;
  }
  return true;
}
