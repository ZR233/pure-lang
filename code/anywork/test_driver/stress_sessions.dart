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

/// Native content-window capacity, the same bound the stress probe saturates.
///
/// The reading window is the shared `ChatView` window in `pl-core/src/chat.rs`
/// (`WINDOW_ITEMS = 96`): a focus paints `INITIAL_ITEMS = 32` and each page adds
/// up to `PAGE_ITEMS = 32` until the window is trimmed back to this capacity.
/// The Driver `timelineWindow.itemIds` mirrors that window, so the list must stay
/// unique and within this capacity; there is no `historyCount`/`overlayCount`
/// field to fall back on.
const _windowCapacityItems = 96;

int boundedWindow(Map<String, dynamic> snapshot) {
  final window = snapshot['timelineWindow'] as Map;
  final ids = (window['itemIds'] as List).cast<String>();
  // The typed contract reports `windowItemCount` for the same window; a missing
  // or inconsistent field is an observation failure, never a silent pass.
  if (window['windowItemCount'] != ids.length ||
      ids.length > _windowCapacityItems ||
      ids.toSet().length != ids.length) {
    throw StateError('invalid window: ${ids.length} items');
  }
  return ids.length;
}

/// Fixture session 1 replies with `Long body 1: ` + `content ` repeated 16_384
/// times (see `gui_stress_script`), so the complete assistant body is
/// 13 + 16_384 * 8 = 131_085 UTF-16 code units. That is below the native 256 KiB
/// timeline budget, so the new contract must show the whole body without a
/// manual full-body load. A truncated preview (for example 8192 code units) is
/// not a complete body.
const _largeSessionPrefix = 'Long body 1: ';
const _largeSessionCharacters = 131085;

/// The longest row body text in the snapshot, or null when none is available.
/// The multi-item stress fixture delivers its large reply as one message, so the
/// longest row is that assistant body.
Map<String, Object?>? _largestAssistantBody(Map<String, dynamic> snapshot) {
  final workspace = snapshot['workspace'];
  if (workspace is! Map) return null;
  final timeline = workspace['timeline'];
  if (timeline is! List) return null;
  Map<String, Object?>? best;
  for (final row in timeline) {
    if (row is! Map) continue;
    final id = row['id'];
    final text = row['text'];
    if (id is! String || text is! String) continue;
    if (best == null || text.length > (best['text'] as String).length) {
      best = {'id': id, 'text': text};
    }
  }
  return best;
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
    if (!settled(first, originalId) ||
        boundedWindow(first) != _windowCapacityItems) {
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
      // `FlutterDriver.waitFor` defaults to no timeout, so every finder wait here
      // carries an explicit deadline instead of hanging the host.
      await driver.waitFor(
        find.byValueKey('start-page-selectors'),
        timeout: const Duration(seconds: 30),
      );
      await driver.waitFor(
        find.byValueKey('composer-input'),
        timeout: const Duration(seconds: 30),
      );
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
      final window = finished['timelineWindow'] as Map;
      final previewed = (window['previewedItemIds'] as List).length;
      final pendingBodies = (window['pendingItemBodyIds'] as List).length;
      final body = _largestAssistantBody(finished);
      final bodyText = body?['text'] as String?;
      final bodyId = body?['id'] as String?;
      sessions.add({
        'ordinal': ordinal,
        'threadId': id,
        'windowItems': countInWindow,
        'previewedBodies': previewed,
        'pendingBodies': pendingBodies,
        'assistantCharacters': bodyText?.length,
      });
      if (ordinal == 1) {
        // The large reply must be delivered whole, without falling back to a
        // lazy body preview or awaiting a manual full-body load. The previous
        // assertion that a large response *must* enter lazy preview checked the
        // removed 8 KiB Dart truncation and is deliberately not kept.
        if (previewed != 0) {
          throw StateError(
            'large response fell back to a lazy body preview: $previewed items',
          );
        }
        if (pendingBodies != 0) {
          throw StateError(
            'large response awaited a manual body load: $pendingBodies items',
          );
        }
        if (bodyText == null ||
            !bodyText.startsWith(_largeSessionPrefix) ||
            bodyText.length != _largeSessionCharacters) {
          throw StateError(
            'large response body incomplete: ${bodyText?.length} of '
            '$_largeSessionCharacters characters',
          );
        }
        // No manual full-body load affordance may be present once the body is
        // complete; the journey never taps one.
        if (bodyId != null) {
          await driver.waitForAbsent(
            find.byValueKey('timeline-item-body-load-$bodyId'),
            timeout: const Duration(seconds: 5),
          );
        }
      }
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
