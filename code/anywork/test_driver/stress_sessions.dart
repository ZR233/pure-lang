import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

Future<Map<String, dynamic>> waitForSnapshot(
  FlutterDriverSession driver,
  bool Function(Map<String, dynamic>) condition,
  String stage,
) async {
  final deadline = DateTime.now().add(const Duration(seconds: 60));
  Map<String, dynamic>? last;
  while (DateTime.now().isBefore(deadline)) {
    last = await driver.readSnapshot();
    if (condition(last)) return last;
    await Future<void>.delayed(const Duration(milliseconds: 150));
  }
  final navigation = last?['navigation'] as Map?;
  final workspace = last?['workspace'] as Map?;
  final composer = navigation?['newThreadComposer'] as Map?;
  final persistence = last?['persistence'] as Map?;
  throw StateError(
    'timed out at $stage: draftLength=${(composer?['draft'] as String?)?.length}, '
    'phase=${composer?['phase']}, busy=${workspace?['isBusy']}, '
    'sync=${workspace?['syncState']}, saving=${persistence?['kind']}',
  );
}

String selectedThread(Map<String, dynamic> snapshot) =>
    (snapshot['navigation'] as Map)['selectedThreadId'] as String;

int boundedWindow(Map<String, dynamic> snapshot) {
  final window = snapshot['timelineWindow'] as Map;
  final ids = (window['itemIds'] as List).cast<String>();
  if (ids.length > 96 || ids.toSet().length != ids.length) {
    throw StateError('invalid window: ${ids.length} items');
  }
  return ids.length;
}

bool settled(Map<String, dynamic> snapshot, String threadId) {
  final workspace = snapshot['workspace'];
  final persistence = snapshot['persistence'];
  return snapshot['navigation'] is Map &&
      (snapshot['navigation'] as Map)['selectedThreadId'] == threadId &&
      workspace is Map &&
      workspace['threadId'] == threadId &&
      workspace['syncState'] == 'ready' &&
      workspace['isBusy'] == false &&
      (workspace['usage'] as Map)['outputTokens'] is num &&
      (workspace['usage'] as Map)['outputTokens'] >= 5 &&
      persistence is Map &&
      persistence['kind'] == 'ready' &&
      persistence['pendingCommits'] == 0 &&
      (snapshot['timelineWindow'] as Map)['itemIds'] is List &&
      ((snapshot['timelineWindow'] as Map)['itemIds'] as List).length >= 2;
}

Future<void> main(List<String> args) async {
  if (args.length != 5) {
    stderr.writeln(
      'usage: stress_sessions.dart VM_URL COUNT PROMPT_PREFIX REPORT_FILE STAGE_FILE',
    );
    exitCode = 64;
    return;
  }
  final count = int.parse(args[1]);
  final report = File(args[3]);
  final stage = File(args[4]);
  final driver = await FlutterDriverSession.connect(vmServiceUrl: args[0]);
  try {
    await stage.writeAsString('initial');
    final first = await waitForSnapshot(
      driver,
      (snapshot) =>
          snapshot['navigation'] is Map &&
          (snapshot['navigation'] as Map)['selectedThreadId'] is String &&
          (snapshot['workspace'] as Map?)?['isBusy'] == false,
      'first session',
    );
    final originalId = selectedThread(first);
    if (!settled(first, originalId) || boundedWindow(first) != 96) {
      throw StateError('original stress session did not settle');
    }
    final ids = <String>{originalId};
    final sessions = <Map<String, Object?>>[];
    for (var ordinal = 1; ordinal <= count; ordinal++) {
      await stage.writeAsString('session_$ordinal');
      await driver.tap(find.byValueKey('sidebar-new-session'));
      await stage.writeAsString('session_${ordinal}_new_tapped');
      await waitForSnapshot(
        driver,
        (snapshot) =>
            snapshot['navigation'] is Map &&
            (snapshot['navigation'] as Map)['selectedThreadId'] == null,
        'new session $ordinal',
      );
      await stage.writeAsString('session_${ordinal}_composer');
      await driver.waitFor(find.byValueKey('start-page-selectors'));
      await driver.waitFor(find.byValueKey('composer-input'));
      await driver.tap(find.byValueKey('composer-input'));
      final prompt = '${args[2]} $ordinal';
      await driver.enterText(prompt);
      await stage.writeAsString('session_${ordinal}_typed');
      await waitForSnapshot(
        driver,
        (snapshot) =>
            (snapshot['navigation'] as Map?)?['newThreadComposer'] is Map &&
            ((snapshot['navigation'] as Map)['newThreadComposer']
                    as Map)['draft'] ==
                prompt,
        'new session $ordinal draft',
      );
      await driver.waitFor(
        find.byValueKey('composer-submit'),
        timeout: const Duration(seconds: 15),
      );
      await driver.tap(find.byValueKey('composer-submit'));
      await stage.writeAsString('session_${ordinal}_submitted');
      final opened = await waitForSnapshot(
        driver,
        (snapshot) =>
            snapshot['navigation'] is Map &&
            (snapshot['navigation'] as Map)['selectedThreadId'] is String &&
            !ids.contains((snapshot['navigation'] as Map)['selectedThreadId']),
        'submit session $ordinal',
      );
      final id = selectedThread(opened);
      if (!ids.add(id)) throw StateError('duplicate session identity: $id');
      final finished = await waitForSnapshot(
        driver,
        (snapshot) =>
            settled(snapshot, id) &&
            (snapshot['sidebarDirectory'] as Map)['titles'] is Map &&
            ((snapshot['sidebarDirectory'] as Map)['titles'] as Map)[id] ==
                'Fixture Session $ordinal',
        'complete session $ordinal',
      );
      final countInWindow = boundedWindow(finished);
      if ((finished['sidebarDirectory'] as Map)['count'] < ids.length) {
        throw StateError('session directory omitted $id');
      }
      final previewed =
          ((finished['timelineWindow'] as Map)['previewedItemIds'] as List)
              .length;
      if (ordinal == 1 && previewed == 0) {
        throw StateError('large response did not enter lazy body preview');
      }
      sessions.add({
        'ordinal': ordinal,
        'threadId': id,
        'windowItems': countInWindow,
        'previewedBodies': previewed,
      });
    }
    await stage.writeAsString('revisit_original');
    await driver.scrollUntilVisible(
      find.byValueKey('sidebar-project-tree'),
      find.byValueKey('thread-row-$originalId'),
      dyScroll: -200,
      timeout: const Duration(seconds: 45),
    );
    await driver.tap(find.byValueKey('thread-row-$originalId'));
    final revisited = await waitForSnapshot(
      driver,
      (snapshot) =>
          settled(snapshot, originalId) &&
          (snapshot['workspace'] as Map)['usage']['outputTokens'] >= 20000,
      'reopen original',
    );
    await report.writeAsString(
      '${jsonEncode({'originalThreadId': originalId, 'originalReopened': true, 'originalWindowItems': boundedWindow(revisited), 'sessionCount': sessions.length, 'directoryCount': (revisited['sidebarDirectory'] as Map)['count'], 'sessions': sessions})}\n',
    );
    await stage.writeAsString('complete');
  } finally {
    await driver.close();
  }
}
