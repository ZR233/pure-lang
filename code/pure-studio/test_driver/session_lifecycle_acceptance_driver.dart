// 会话生命周期破坏性重构的 Flutter Driver GUI 验收。
//
// 所有产品动作均通过可见控件完成；requestData 只承担 fixture、只读快照和关机。

import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

Future<void> main(List<String> args) async {
  final options = _Options.parse(args);
  final artifacts = Directory(options.artifacts)..createSync(recursive: true);
  final snapshots = File('${artifacts.path}/snapshots.jsonl');
  final session = await FlutterDriverSession.connect(
    vmServiceUrl: options.vmServiceUrl,
  );
  try {
    await session.waitFor(
      find.byValueKey('studio-shell'),
      timeout: const Duration(minutes: 3),
    );
    switch (options.mode) {
      case 'demo':
        await _acceptDemo(session, snapshots, artifacts);
      case 'runtime-seed':
        await _acceptRuntimeSeed(session, snapshots, artifacts, options);
      case 'runtime-restart':
        await _acceptRuntimeRestart(session, snapshots, artifacts);
      default:
        throw ArgumentError.value(options.mode, 'mode', 'unsupported');
    }
    await session.requestData(
      'shutdown-await',
      timeout: const Duration(minutes: 2),
    );
    stdout.writeln('session lifecycle acceptance (${options.mode}): ok');
  } catch (error, stackTrace) {
    await _screenshot(session, File('${artifacts.path}/failure.png'));
    stderr.writeln(error);
    stderr.writeln(stackTrace);
    exitCode = 1;
  } finally {
    await session.close();
  }
}

Future<void> _acceptDemo(
  FlutterDriverSession session,
  File snapshots,
  Directory artifacts,
) async {
  final prepared =
      jsonDecode(await session.requestData('prepare-session-lifecycle-demo'))
          as Map<String, dynamic>;
  if (prepared['prepared'] != true) {
    throw StateError('demo scenario was not prepared: $prepared');
  }
  final initial = await _snapshot(session, snapshots, 'demo-initial');
  final initialIds = _directoryIds(initial);
  if (initialIds.length != 2) {
    throw StateError('expected two demo roots, got $initialIds');
  }

  await session.tap(find.byValueKey('sidebar-new-session'));
  await session.waitFor(find.byValueKey('studio-start-page'));
  final entered = await _waitForSnapshot(
    session,
    snapshots,
    'demo-start-page',
    (snapshot) => _isStartPage(snapshot),
  );
  _expectDirectory(entered, initialIds, 'entering the start page');

  await session.tap(find.byValueKey('sidebar-new-session'));
  final reset = await _snapshot(session, snapshots, 'demo-start-page-reset');
  _expectDirectory(reset, initialIds, 'resetting the transient draft');
  if (_newComposer(reset)['draft'] != '') {
    throw StateError('repeated new-session did not reset the draft');
  }

  const prompt = 'Driver first message creates exactly one session';
  await session.tap(find.byValueKey('composer-input'));
  await session.enterText(prompt);
  final drafted = await _waitForSnapshot(
    session,
    snapshots,
    'demo-draft',
    (snapshot) => _newComposer(snapshot)['draft'] == prompt,
  );
  _expectDirectory(drafted, initialIds, 'typing the transient draft');
  await _screenshot(session, File('${artifacts.path}/before-first-send.png'));

  await session.tap(find.byValueKey('composer-submit'));
  final created = await _waitForSnapshot(
    session,
    snapshots,
    'demo-first-send-created',
    (snapshot) {
      final selected = _selectedThreadId(snapshot);
      return selected != null &&
          !initialIds.contains(selected) &&
          _directoryIds(snapshot).contains(selected);
    },
    timeout: const Duration(seconds: 45),
  );
  final createdId = _selectedThreadId(created)!;
  await _waitForSnapshot(
    session,
    snapshots,
    'demo-first-turn-completed',
    (snapshot) {
      final workspace = snapshot['workspace'];
      if (workspace is! Map<String, dynamic> ||
          workspace['threadId'] != createdId) {
        return false;
      }
      final lastTurn = workspace['lastTurn'];
      return lastTurn is Map<String, dynamic> &&
          const {
            'completed',
            'failed',
            'cancelled',
          }.contains(lastTurn['status']);
    },
    timeout: const Duration(seconds: 45),
  );
  await _screenshot(session, File('${artifacts.path}/after-first-send.png'));

  final afterCreatedArchive = await _archiveSelected(
    session,
    snapshots,
    createdId,
    'demo-archive-created',
  );
  if (_selectedThreadId(afterCreatedArchive) != initialIds.first) {
    throw StateError(
      'archiving the new root should select ${initialIds.first}: '
      '${_selectedThreadId(afterCreatedArchive)}',
    );
  }

  var remaining = _directoryIds(afterCreatedArchive);
  while (remaining.isNotEmpty) {
    final selected = _selectedThreadId(
      await _snapshot(session, snapshots, 'demo-before-archive'),
    );
    if (selected == null) {
      throw StateError('directory still has roots but selection is null');
    }
    final next = await _archiveSelected(
      session,
      snapshots,
      selected,
      'demo-archive-$selected',
    );
    remaining = _directoryIds(next);
  }
  final empty = await _waitForSnapshot(
    session,
    snapshots,
    'demo-final-empty',
    (snapshot) =>
        _isStartPage(snapshot) &&
        _directoryIds(snapshot).isEmpty &&
        snapshot['workspace'] == null,
  );
  if (_selectedProjectId(empty) == null) {
    throw StateError('empty session state lost its selected project');
  }
  await _screenshot(session, File('${artifacts.path}/final-empty.png'));
}

Future<void> _acceptRuntimeSeed(
  FlutterDriverSession session,
  File snapshots,
  Directory artifacts,
  _Options options,
) async {
  final workspace = options.workspace;
  if (workspace == null) {
    throw ArgumentError('--workspace is required for runtime-seed');
  }
  await session.tap(find.byValueKey('sidebar-open-project'));
  await session.waitFor(find.byValueKey('project-path-dialog'));
  await session.tap(find.byValueKey('project-path-input'));
  await session.enterText(workspace);
  await session.sendTextInputAction(TextInputAction.done);
  await session.waitForAbsent(
    find.byValueKey('project-path-dialog'),
    timeout: const Duration(seconds: 30),
  );
  final empty = await _waitForSnapshot(
    session,
    snapshots,
    'runtime-open-empty-project',
    (snapshot) =>
        _selectedProjectId(snapshot) != null &&
        _isStartPage(snapshot) &&
        _directoryIds(snapshot).isEmpty,
  );
  if (empty['workspace'] != null) {
    throw StateError('opening an empty project retained a workspace snapshot');
  }
  await _screenshot(session, File('${artifacts.path}/opened-empty.png'));

  final seeded =
      jsonDecode(await session.requestData('seed-threads:${options.seedCount}'))
          as Map<String, dynamic>;
  if (seeded['seeded'] != options.seedCount) {
    throw StateError('runtime fixture seeding failed: $seeded');
  }
  var snapshot = await _waitForSnapshot(
    session,
    snapshots,
    'runtime-seeded',
    (candidate) => _directoryIds(candidate).length == options.seedCount,
  );
  while (_directoryIds(snapshot).isNotEmpty) {
    final selected = _selectedThreadId(snapshot);
    if (selected == null) {
      throw StateError('seeded roots have no selected Thread: $snapshot');
    }
    snapshot = await _archiveSelected(
      session,
      snapshots,
      selected,
      'runtime-archive-$selected',
    );
  }
  final finalEmpty = await _waitForSnapshot(
    session,
    snapshots,
    'runtime-final-empty',
    (candidate) =>
        _isStartPage(candidate) &&
        _directoryIds(candidate).isEmpty &&
        candidate['workspace'] == null,
  );
  if (_selectedProjectId(finalEmpty) == null) {
    throw StateError('runtime final empty state lost its Project');
  }
  await _screenshot(session, File('${artifacts.path}/final-empty.png'));
}

Future<void> _acceptRuntimeRestart(
  FlutterDriverSession session,
  File snapshots,
  Directory artifacts,
) async {
  final snapshot = await _waitForSnapshot(
    session,
    snapshots,
    'runtime-restart-empty',
    (candidate) =>
        _selectedProjectId(candidate) != null &&
        _isStartPage(candidate) &&
        _directoryIds(candidate).isEmpty &&
        candidate['workspace'] == null,
    timeout: const Duration(minutes: 2),
  );
  if (_newComposer(snapshot)['phase'] != 'idle') {
    throw StateError('restart restored a stale transient draft: $snapshot');
  }
  await session.waitFor(find.byValueKey('studio-start-page'));
  await _screenshot(session, File('${artifacts.path}/restart-empty.png'));
}

Future<Map<String, dynamic>> _archiveSelected(
  FlutterDriverSession session,
  File snapshots,
  String threadId,
  String stage,
) async {
  await session.waitFor(
    find.byValueKey('thread-archive-$threadId'),
    timeout: const Duration(seconds: 30),
  );
  await session.tap(find.byValueKey('thread-archive-$threadId'));
  return _waitForSnapshot(
    session,
    snapshots,
    stage,
    (snapshot) => !_directoryIds(snapshot).contains(threadId),
    timeout: const Duration(seconds: 30),
  );
}

Future<Map<String, dynamic>> _waitForSnapshot(
  FlutterDriverSession session,
  File snapshots,
  String stage,
  bool Function(Map<String, dynamic>) predicate, {
  Duration timeout = const Duration(seconds: 20),
}) async {
  final deadline = DateTime.now().add(timeout);
  Map<String, dynamic>? last;
  while (DateTime.now().isBefore(deadline)) {
    last = await session.readSnapshot();
    if (predicate(last)) {
      await _appendSnapshot(snapshots, stage, last);
      return last;
    }
    await Future<void>.delayed(const Duration(milliseconds: 150));
  }
  throw StateError('$stage timed out; last=$last');
}

Future<Map<String, dynamic>> _snapshot(
  FlutterDriverSession session,
  File snapshots,
  String stage,
) async {
  final snapshot = await session.readSnapshot();
  await _appendSnapshot(snapshots, stage, snapshot);
  return snapshot;
}

Future<void> _appendSnapshot(
  File output,
  String stage,
  Map<String, dynamic> snapshot,
) {
  return output.writeAsString(
    '${jsonEncode({'stage': stage, 'snapshot': snapshot})}\n',
    mode: FileMode.append,
    flush: true,
  );
}

Future<void> _screenshot(FlutterDriverSession session, File output) async {
  await output.parent.create(recursive: true);
  await output.writeAsBytes(await session.screenshot(), flush: true);
}

void _expectDirectory(
  Map<String, dynamic> snapshot,
  List<String> expected,
  String operation,
) {
  final actual = _directoryIds(snapshot);
  if (jsonEncode(actual) != jsonEncode(expected)) {
    throw StateError('$operation changed the directory: $actual != $expected');
  }
}

List<String> _directoryIds(Map<String, dynamic> snapshot) {
  final directory = snapshot['sidebarDirectory'];
  final ids = directory is Map<String, dynamic> ? directory['ids'] : null;
  return ids is List ? [for (final id in ids) id.toString()] : const [];
}

Map<String, dynamic> _navigation(Map<String, dynamic> snapshot) =>
    snapshot['navigation'] as Map<String, dynamic>? ?? const {};

Map<String, dynamic> _newComposer(Map<String, dynamic> snapshot) =>
    _navigation(snapshot)['newThreadComposer'] as Map<String, dynamic>? ??
    const {};

String? _selectedProjectId(Map<String, dynamic> snapshot) =>
    _navigation(snapshot)['selectedProjectId'] as String?;

String? _selectedThreadId(Map<String, dynamic> snapshot) =>
    _navigation(snapshot)['selectedThreadId'] as String?;

bool _isStartPage(Map<String, dynamic> snapshot) =>
    _navigation(snapshot)['isStartPage'] == true;

class _Options {
  const _Options({
    required this.vmServiceUrl,
    required this.mode,
    required this.artifacts,
    required this.workspace,
    required this.seedCount,
  });

  final String vmServiceUrl;
  final String mode;
  final String artifacts;
  final String? workspace;
  final int seedCount;

  static _Options parse(List<String> args) {
    final values = <String, String>{};
    for (var index = 0; index < args.length; index += 1) {
      final argument = args[index];
      if (!argument.startsWith('--') || index + 1 >= args.length) continue;
      values[argument.substring(2)] = args[++index];
    }
    String required(String name) =>
        values[name] ?? (throw ArgumentError('missing --$name'));
    return _Options(
      vmServiceUrl: required('vm-service-url'),
      mode: required('mode'),
      artifacts: required('artifacts'),
      workspace: values['workspace'],
      seedCount: int.tryParse(values['seed-count'] ?? '') ?? 3,
    );
  }
}
