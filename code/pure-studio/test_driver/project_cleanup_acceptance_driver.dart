import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

Future<void> main(List<String> arguments) async {
  final options = _DriverOptions.parse(arguments);
  final workspace = Directory(options.workspace).absolute;
  FlutterDriverSession? session;
  try {
    session = await FlutterDriverSession.connect(
      vmServiceUrl: options.vmServiceUrl,
    );
    await _command(
      session.waitFor(
        find.byValueKey('studio-shell'),
        timeout: const Duration(minutes: 2),
      ),
      'Studio shell',
    );
    await _openProject(session, workspace.path);
    final opened = await _waitForSnapshot(
      session,
      'selected temporary project',
      (snapshot) =>
          _selectedProjectPath(snapshot) == _normalized(workspace.path),
    );
    final project = opened['project'];
    final projectId = project is Map<String, dynamic> ? project['id'] : null;
    if (projectId is! String || projectId.isEmpty) {
      throw StateError('Driver snapshot has no selected project id');
    }

    final cleanupButton = find.byValueKey('project-cleanup-$projectId');
    await _command(
      session.waitFor(cleanupButton, timeout: const Duration(minutes: 1)),
      'project cleanup button',
    );
    await _command(session.tap(cleanupButton), 'open project cleanup dialog');
    final confirm = find.byValueKey('project-cleanup-confirm');
    await _command(
      session.waitFor(confirm, timeout: const Duration(minutes: 1)),
      'project cleanup confirmation',
    );
    await _command(
      session.waitUntilNoTransientCallbacks(
        timeout: const Duration(seconds: 30),
      ),
      'project cleanup preview',
    );
    await _command(session.tap(confirm), 'confirm project cleanup');
    await _command(
      session.waitForAbsent(confirm, timeout: const Duration(minutes: 2)),
      'project cleanup dialog close',
    );
    await _command(
      session.waitForAbsent(cleanupButton, timeout: const Duration(minutes: 2)),
      'cleaned project removal',
    );
    final cleaned = await _waitForSnapshot(
      session,
      'cleaned project selection removal',
      (snapshot) {
        final selected = snapshot['project'];
        return selected == null ||
            (selected is Map<String, dynamic> && selected['id'] != projectId);
      },
    );
    if (!await workspace.exists()) {
      throw StateError('Project cleanup deleted the user workspace');
    }

    await File(
      options.snapshotOutput,
    ).writeAsString('${jsonEncode(cleaned)}\n', flush: true);
    stdout.writeln(
      jsonEncode({
        'result': 'completed',
        'projectId': projectId,
        'workspace': workspace.path,
        'workspacePreserved': true,
      }),
    );
  } finally {
    await session?.close();
  }
}

Future<void> _openProject(
  FlutterDriverSession session,
  String workspace,
) async {
  await _command(
    session.tap(find.byValueKey('sidebar-open-project')),
    'open project dialog',
  );
  await _command(
    session.waitFor(
      find.byValueKey('project-path-dialog'),
      timeout: const Duration(seconds: 30),
    ),
    'project path dialog',
  );
  await _command(
    session.tap(find.byValueKey('project-path-input')),
    'project path input',
  );
  await _command(session.enterText(workspace), 'project path entry');
  await _command(
    session.tap(find.byValueKey('project-path-submit')),
    'project path submit',
  );
  await _command(
    session.waitForAbsent(
      find.byValueKey('project-path-dialog'),
      timeout: const Duration(minutes: 2),
    ),
    'project path dialog close',
  );
}

Future<Map<String, dynamic>> _waitForSnapshot(
  FlutterDriverSession session,
  String description,
  bool Function(Map<String, dynamic> snapshot) predicate,
) async {
  final deadline = DateTime.now().add(const Duration(minutes: 2));
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await session.readSnapshot();
    if (predicate(snapshot)) return snapshot;
    await Future<void>.delayed(const Duration(milliseconds: 250));
  }
  throw TimeoutException('Timed out waiting for $description');
}

Future<T> _command<T>(Future<T> command, String description) {
  return command.timeout(const Duration(minutes: 2)).onError((
    error,
    stackTrace,
  ) {
    Error.throwWithStackTrace(
      StateError('$description failed: $error'),
      stackTrace,
    );
  });
}

String? _selectedProjectPath(Map<String, dynamic> snapshot) {
  final project = snapshot['project'];
  final path = project is Map<String, dynamic> ? project['path'] : null;
  return path is String ? _normalized(path) : null;
}

String _normalized(String path) {
  var normalized = File(path).absolute.path.replaceAll('\\', '/');
  while (normalized.length > 1 && normalized.endsWith('/')) {
    normalized = normalized.substring(0, normalized.length - 1);
  }
  return Platform.isWindows ? normalized.toLowerCase() : normalized;
}

class _DriverOptions {
  const _DriverOptions({
    required this.vmServiceUrl,
    required this.workspace,
    required this.snapshotOutput,
  });

  final String vmServiceUrl;
  final String workspace;
  final String snapshotOutput;

  static _DriverOptions parse(List<String> arguments) {
    final values = <String, String>{};
    for (var index = 0; index < arguments.length; index += 2) {
      if (index + 1 >= arguments.length || !arguments[index].startsWith('--')) {
        throw const FormatException('Expected --name value arguments');
      }
      values[arguments[index].substring(2)] = arguments[index + 1];
    }
    String required(String name) {
      final value = values[name];
      if (value == null || value.isEmpty) {
        throw FormatException('Missing --$name');
      }
      return value;
    }

    return _DriverOptions(
      vmServiceUrl: required('vm-service-url'),
      workspace: required('workspace'),
      snapshotOutput: required('snapshot-output'),
    );
  }
}
