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
    // Pure geometry/identity so the integrated timeline scroll contract can be
    // re-checked from evidence; no message text is duplicated here.
    'timelineScroll': _timelineScroll(snapshot),
    // Activity stage identity plus expand state only; summary/error stay as
    // lengths so no provider text or credential can leak into the snapshot.
    'conversationActivity': _conversationActivity(snapshot),
    // Driver-only application-payload counters; `bodyUtf8Bytes` is a UTF-8
    // measure of delivered body text and is explicitly not FRB/wire bytes.
    'contentDelivery': _contentDelivery(snapshot),
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

Map<String, Object?>? _timelineScroll(Map<String, dynamic> snapshot) {
  final scroll = snapshot['timelineScroll'];
  if (scroll is! Map) return null;
  final anchor = scroll['anchor'];
  return {
    'threadId': scroll['threadId'],
    'centerId': scroll['centerId'],
    'centerIndex': scroll['centerIndex'],
    'rowCount': scroll['rowCount'],
    'followingBottom': scroll['followingBottom'],
    'detachedByUser': scroll['detachedByUser'],
    'pendingNewEvents': scroll['pendingNewEvents'],
    'pixels': scroll['pixels'],
    'minScrollExtent': scroll['minScrollExtent'],
    'maxScrollExtent': scroll['maxScrollExtent'],
    'viewportDimension': scroll['viewportDimension'],
    'extentAfter': scroll['extentAfter'],
    'bottomSlack': scroll['bottomSlack'],
    'hasNewer': scroll['hasNewer'],
    'showJumpToLatest': scroll['showJumpToLatest'],
    'anchor': anchor is Map
        ? {
            'itemId': anchor['itemId'],
            'offset': anchor['offset'],
            'followingBottom': anchor['followingBottom'],
          }
        : null,
  };
}

Map<String, Object?>? _conversationActivity(Map<String, dynamic> snapshot) {
  final activity = snapshot['conversationActivity'];
  if (activity is! Map) return null;
  final details = activity['details'];
  final summary = activity['summary'];
  final error = activity['errorMessage'];
  return {
    'identity': activity['identity'],
    'kind': activity['kind'],
    'activeToolCount': activity['activeToolCount'],
    // `expandable` is the capability; `expanded` is the user's actual state.
    'expandable': activity['expandable'],
    'expanded': activity['expanded'],
    'summaryLength': summary is String ? summary.length : 0,
    'errorLength': error is String ? error.length : 0,
    'hasDetails': details is List && details.isNotEmpty,
    'detailCount': details is List ? details.length : 0,
    'detailIds': [
      if (details is List)
        for (final detail in details)
          if (detail is Map) detail['id'],
    ],
  };
}

Map<String, Object?>? _contentDelivery(Map<String, dynamic> snapshot) {
  final delivery = snapshot['contentDelivery'];
  if (delivery is! Map) return null;
  return {
    'enabled': delivery['enabled'],
    'metric': delivery['metric'],
    'resets': delivery['resets'],
    'patches': delivery['patches'],
    'contentChanges': delivery['contentChanges'],
    'bodyUtf8Bytes': delivery['bodyUtf8Bytes'],
    'maxWindowItems': delivery['maxWindowItems'],
  };
}
