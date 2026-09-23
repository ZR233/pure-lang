import 'dart:convert';
import 'dart:io';

import 'flutter_driver_session.dart';

Future<void> main(List<String> args) async {
  if (args.length != 2) {
    stderr.writeln(
      'usage: dart run test_driver/manual_capture.dart VM_URL OUTPUT_DIR',
    );
    exitCode = 64;
    return;
  }
  final output = Directory(args[1]);
  final driver = await FlutterDriverSession.connect(vmServiceUrl: args[0]);
  try {
    Future<void> stage(String value) =>
        File('${output.path}/capture-stage.txt').writeAsString(value);
    final snapshot = await driver.readSnapshot();
    await File('${output.path}/snapshot.json').writeAsString(
      '${const JsonEncoder.withIndent('  ').convert(_summary(snapshot))}\n',
    );
    await File('${output.path}/screenshot.png')
        .writeAsBytes(await driver.screenshot());
    await stage('screenshot_captured');
    late final Object? shutdown;
    try {
      shutdown = jsonDecode(
        await driver
            .requestData('shutdown', timeout: const Duration(seconds: 60))
            .timeout(const Duration(seconds: 65)),
      );
    } catch (_) {
      await stage('shutdown_request_failed');
      rethrow;
    }
    if (shutdown is! Map || shutdown['shutdown'] != 'completed') {
      await stage('shutdown_rejected');
      throw StateError('native GUI shutdown did not complete');
    }
    await stage('shutdown_completed');
    stdout.writeln('Evidence captured; human verdict pending.');
  } finally {
    try {
      await driver.close().timeout(const Duration(seconds: 5));
    } catch (_) {
      // A completed shutdown must not keep the acceptance process open.
    }
  }
}

Map<String, Object?> _summary(Map<String, dynamic> snapshot) {
  final workspace = snapshot['workspace'];
  final details = workspace is Map ? workspace : const {};
  final usage = details['usage'];
  final usageDetails = usage is Map ? usage : const {};
  final turn = details['turn'];
  final turnDetails = turn is Map ? turn : const {};
  final persistence = snapshot['persistence'];
  final persistenceDetails = persistence is Map ? persistence : const {};
  final timeline = details['timeline'];
  final composer = details['composer'];
  final composerDetails = composer is Map ? composer : const {};
  return {
    'projectOpened': snapshot['project'] != null,
    'persistence': {
      'kind': persistenceDetails['kind'],
      'pendingCommits': persistenceDetails['pendingCommits'],
      'needsAttention': persistenceDetails['needsAttention'],
    },
    'workspace': workspace == null
        ? null
        : {
            'threadStatus': details['threadStatus'],
            'isBusy': details['isBusy'],
            'syncState': details['syncState'],
            'turnStatus': turnDetails['status'],
            'timelineRows': timeline is List ? timeline.length : 0,
            'outputTokens': usageDetails['outputTokens'],
            'hasIncompleteUsage': usageDetails['hasIncompleteUsage'],
            'submissionPending': composerDetails['submissionPending'],
          },
  };
}
