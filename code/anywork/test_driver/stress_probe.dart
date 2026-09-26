import 'dart:convert';
import 'dart:io';

import 'flutter_driver_session.dart';

/// Native content-window capacity for the stress acceptance.
///
/// The reading window is the shared `ChatView` window in `pl-core/src/chat.rs`:
/// a focus paints `INITIAL_ITEMS = 32` items, each `load`/`load-older` page adds
/// up to `PAGE_ITEMS = 32`, and the window trims its opposite edge as soon as it
/// would exceed `WINDOW_ITEMS = 96`. The Driver `timelineWindow.itemIds` mirrors
/// that same window, so a unique id list that never exceeds this capacity is the
/// bounded-memory proof.
///
/// The removed `timelineWindow.historyCount`/`overlayCount` fields are
/// deliberately not read back: the window is the only body owner, and silently
/// defaulting a missing field to zero is exactly what turned their removal into
/// a permanent "bounded" pass.
const _windowCapacityItems = 96;

/// Upper bound on the `load-older` pages the probe drives.
///
/// `INITIAL_ITEMS` plus this many `PAGE_ITEMS` covers the capacity several times
/// over, so an unresponsive window fails visibly instead of polling forever.
const _maxOlderPages = 4;

/// Milliseconds the probe waits for one `load-older` page to reach the window.
const _pageMoveTimeoutMillis = 20000;

/// The typed content window's item identities.
///
/// Returns null when the snapshot does not carry the current contract: a
/// `timelineWindow.itemIds` list of strings whose length equals the reported
/// `windowItemCount`. A missing typed field is a failure to observe, never a
/// silent zero.
List<String>? _windowItemIds(Map<String, dynamic> snapshot) {
  final window = snapshot['timelineWindow'];
  if (window is! Map) return null;
  final ids = window['itemIds'];
  if (ids is! List) return null;
  final windowCount = window['windowItemCount'];
  if (windowCount is! num || windowCount != ids.length) return null;
  final result = <String>[];
  for (final id in ids) {
    if (id is! String) return null;
    result.add(id);
  }
  return result;
}

Object? _windowField(Map<String, dynamic> snapshot, String key) {
  final window = snapshot['timelineWindow'];
  return window is Map ? window[key] : null;
}

bool? _windowBool(Map<String, dynamic> snapshot, String key) {
  final value = _windowField(snapshot, key);
  return value is bool ? value : null;
}

/// Length of one of the window's id lists (`previewedItemIds`, ...), or null
/// when the field is not the typed list the contract promises.
int? _windowIdListLength(Map<String, dynamic> snapshot, String key) {
  final value = _windowField(snapshot, key);
  return value is List ? value.length : null;
}

String? _windowAnchorItemId(Map<String, dynamic> snapshot) {
  final anchor = _windowField(snapshot, 'anchor');
  final id = anchor is Map ? anchor['itemId'] : null;
  return id is String ? id : null;
}

/// A field of the workspace's actual timeline rows (`timelineProgress`), so the
/// paging evidence can tie the window identities back to the delivered rows
/// instead of trusting a window count alone.
Object? _rowsField(Map<String, dynamic> snapshot, String key) {
  final workspace = snapshot['workspace'];
  final progress = workspace is Map ? workspace['timelineProgress'] : null;
  return progress is Map ? progress[key] : null;
}

bool _sameIds(List<String> left, List<String> right) {
  if (left.length != right.length) return false;
  for (var index = 0; index < left.length; index += 1) {
    if (left[index] != right[index]) return false;
  }
  return true;
}

/// The probe's window evidence file, a sibling of the sample report
/// (`probe-report.json` -> `probe-report.paging.json`).
File _pagingEvidenceFile(String reportPath) {
  final separator = reportPath.lastIndexOf(Platform.pathSeparator);
  final dot = reportPath.lastIndexOf('.');
  if (dot > separator) {
    return File(
      '${reportPath.substring(0, dot)}.paging${reportPath.substring(dot)}',
    );
  }
  return File('$reportPath.paging.json');
}

/// Polls the live window (read-only, bounded, no screenshots) until
/// `load-older` moved it. Stops early when the window reports no older content,
/// so a genuinely exhausted history fails fast instead of waiting out the
/// timeout.
Future<Map<String, dynamic>> _awaitWindowMove(
  FlutterDriverSession driver,
  List<String> before,
) async {
  final deadline = DateTime.now().add(
    const Duration(milliseconds: _pageMoveTimeoutMillis),
  );
  var snapshot = await driver.readSnapshot();
  while (DateTime.now().isBefore(deadline)) {
    final ids = _windowItemIds(snapshot);
    if (ids != null && !_sameIds(ids, before)) return snapshot;
    if (_windowBool(snapshot, 'hasOlder') == false) return snapshot;
    await Future<void>.delayed(const Duration(milliseconds: 120));
    snapshot = await driver.readSnapshot();
  }
  return snapshot;
}

/// Drives the real `load-older` command until the live window is saturated at
/// its capacity, recording one evidence step per page.
///
/// Each step proves the window actually moved (it grew toward older content) and
/// that identity was retained (every id present before the page is still present
/// after it), rather than inferring a move from a version counter. The loop is
/// bounded by [_maxOlderPages] and never polls forever.
Future<void> _pageOlderIntoWindow(
  FlutterDriverSession driver,
  Map<String, dynamic> snapshot,
  List<Map<String, Object?>> pages,
) async {
  var current = snapshot;
  var previous = _windowItemIds(snapshot);
  for (var page = 1; page <= _maxOlderPages; page += 1) {
    if (previous == null || previous.length >= _windowCapacityItems) break;
    if (_windowBool(current, 'hasOlder') != true) break;
    final before = previous;
    final anchorBefore = _windowAnchorItemId(current);
    final epochBefore = _windowField(current, 'epoch');
    await driver.requestData('load-older');
    final next = await _awaitWindowMove(driver, before);
    final ids = _windowItemIds(next);
    final unique = ids != null && ids.toSet().length == ids.length;
    final retained = ids != null && before.every(ids.contains);
    final grew = ids != null && ids.length > before.length;
    final anchorAfter = _windowAnchorItemId(next);
    pages.add(<String, Object?>{
      'page': page,
      'beforeItems': before.length,
      'beforeFirst': before.isEmpty ? null : before.first,
      'beforeLast': before.isEmpty ? null : before.last,
      'afterItems': ids?.length,
      'afterFirst': ids == null || ids.isEmpty ? null : ids.first,
      'afterLast': ids == null || ids.isEmpty ? null : ids.last,
      'unique': unique,
      'withinCapacity': ids != null && ids.length <= _windowCapacityItems,
      'grewTowardOlder': grew,
      'identityRetained': retained,
      'movedOlder':
          ids != null &&
          grew &&
          retained &&
          before.isNotEmpty &&
          ids.isNotEmpty &&
          ids.first != before.first &&
          ids.last == before.last,
      'hasOlder': _windowBool(next, 'hasOlder'),
      'hasNewer': _windowBool(next, 'hasNewer'),
      'rowCount': _rowsField(next, 'rowCount'),
      'rowLastSequence': _rowsField(next, 'lastSequence'),
      'epochBefore': epochBefore,
      'epochAfter': _windowField(next, 'epoch'),
      'anchorBefore': anchorBefore,
      'anchorAfter': anchorAfter,
      'anchorRetained':
          anchorAfter == null || ids == null || ids.contains(anchorAfter),
    });
    current = next;
    if (ids == null || !grew) break;
    previous = ids;
  }
}

Future<void> main(List<String> args) async {
  if (args.length != 6) {
    stderr.writeln(
      'usage: stress_probe.dart VM_URL STOP_FILE REPORT_FILE READY_FILE FRAMES_FILE FINISHED_FILE',
    );
    exitCode = 64;
    return;
  }
  final stop = File(args[1]);
  final finished = File(args[5]);
  final pagingFile = _pagingEvidenceFile(args[2]);
  final pagingPages = <Map<String, Object?>>[];
  final samples = <Map<String, Object?>>[];
  var lastReport = 0;
  Map<String, Object?>? authoritative;
  Map<String, Object?>? persistenceDiagnostics;
  var paginationStarted = false;
  final driver = await FlutterDriverSession.connect(vmServiceUrl: args[0]);

  Future<void> writePagingEvidence() async {
    final paged = pagingPages.isNotEmpty;
    final finalItems = paged ? pagingPages.last['afterItems'] : null;
    final evidence = <String, Object?>{
      'capacity': _windowCapacityItems,
      'maxPages': _maxOlderPages,
      'paged': paged,
      'pageCount': pagingPages.length,
      'pages': pagingPages,
      'finalItems': finalItems,
      'saturated': paged && finalItems == _windowCapacityItems,
      'movedOlder':
          paged && pagingPages.any((page) => page['movedOlder'] == true),
      'identityRetained':
          paged &&
          pagingPages.every((page) => page['identityRetained'] == true),
      'anchorRetained':
          paged && pagingPages.every((page) => page['anchorRetained'] == true),
      'allWithinCapacity':
          paged && pagingPages.every((page) => page['withinCapacity'] == true),
      'allUnique': paged && pagingPages.every((page) => page['unique'] == true),
    };
    await pagingFile.writeAsString('${jsonEncode(evidence)}\n');
  }

  try {
    await driver.requestData('frame-start');
    await File(args[3]).writeAsString('ready\n');
    while (!await stop.exists()) {
      final started = DateTime.now().millisecondsSinceEpoch;
      try {
        final snapshot = await driver.readSnapshot();
        final workspace = snapshot['workspace'];
        final persistence = snapshot['persistence'];
        final usage = workspace is Map ? workspace['usage'] : null;
        final timeline = workspace is Map ? workspace['timeline'] : null;
        final ids = _windowItemIds(snapshot);
        final bounded =
            ids != null &&
            ids.length <= _windowCapacityItems &&
            ids.toSet().length == ids.length;
        final rowKinds = timeline is List
            ? {
                for (final row in timeline)
                  if (row is Map) row['type'],
              }
            : <Object?>{};
        final diverse =
            timeline is List &&
            timeline.length >= 60 &&
            rowKinds.contains('finalAnswer') &&
            rowKinds.contains('commentary') &&
            rowKinds.contains('reasoningSummary');
        final streamCompleted =
            workspace is Map &&
            persistence is Map &&
            usage is Map &&
            (usage['outputTokens'] as num? ?? 0) >= 20000 &&
            workspace['isBusy'] == false &&
            workspace['syncState'] == 'ready' &&
            bounded &&
            persistence['kind'] == 'ready' &&
            persistence['pendingCommits'] == 0;
        if (!paginationStarted &&
            streamCompleted &&
            _windowBool(snapshot, 'hasOlder') == true) {
          // The stream is durably settled and the window still reports older
          // content: drive the real page commands and record how the window
          // moved. The trigger is the typed `hasOlder` flag plus the bounded
          // window, not the removed history/overlay counters.
          paginationStarted = true;
          await _pageOlderIntoWindow(driver, snapshot, pagingPages);
          await writePagingEvidence();
          continue;
        }
        if (!await finished.exists() &&
            paginationStarted &&
            streamCompleted &&
            ids.length == _windowCapacityItems &&
            diverse) {
          await finished.writeAsString(
            '${DateTime.now().millisecondsSinceEpoch}\n',
          );
        }
        if (workspace is Map &&
            workspace['isBusy'] == false &&
            (usage is Map ? (usage['outputTokens'] as num? ?? 0) : 0) < 20000 &&
            started - lastReport >= 1000) {
          try {
            authoritative = (jsonDecode(
              await driver.requestData('thread-current'),
            ) as Map).cast<String, Object?>();
          } catch (_) {
            authoritative = {'error': 'unavailable'};
          }
        }
        if (persistence is Map &&
            persistence['kind'] == 'blocked' &&
            started - lastReport >= 1000) {
          try {
            persistenceDiagnostics = (jsonDecode(
              await driver.requestData('persistence-queue'),
            ) as Map).cast<String, Object?>();
          } catch (_) {
            persistenceDiagnostics = {'error': 'unavailable'};
          }
        }
        samples.add({
          'startedUnixMillis': started,
          'elapsedMillis': DateTime.now().millisecondsSinceEpoch - started,
          'ok': bounded,
          'itemCount': ids?.length,
          'windowItemCount': _windowField(snapshot, 'windowItemCount'),
          'hasOlder': _windowBool(snapshot, 'hasOlder'),
          'hasNewer': _windowBool(snapshot, 'hasNewer'),
          'windowLoading': _windowBool(snapshot, 'loading'),
          'windowDirection': _windowField(snapshot, 'direction'),
          'windowEpoch': _windowField(snapshot, 'epoch'),
          'windowAnchorItemId': _windowAnchorItemId(snapshot),
          'previewedCount': _windowIdListLength(snapshot, 'previewedItemIds'),
          'loadedCount': _windowIdListLength(snapshot, 'loadedItemIds'),
          'pendingBodyCount': _windowIdListLength(
            snapshot,
            'pendingItemBodyIds',
          ),
          'rowCount': timeline is List ? timeline.length : null,
          'outputTokens': usage is Map ? usage['outputTokens'] : null,
          'authoritative': authoritative,
          'isBusy': workspace is Map ? workspace['isBusy'] : null,
          'syncState': workspace is Map ? workspace['syncState'] : null,
          'syncError': workspace is Map ? workspace['loadError'] : null,
          'rowKinds': rowKinds.toList(),
          'persistenceKind': persistence is Map ? persistence['kind'] : null,
          'persistenceDiagnostics': persistenceDiagnostics,
          'persistenceErrorCode': persistence is Map
              ? persistence['errorCode']
              : null,
          'pendingCommits': persistence is Map
              ? persistence['pendingCommits']
              : null,
        });
      } catch (error) {
        samples.add({
          'startedUnixMillis': started,
          'elapsedMillis': DateTime.now().millisecondsSinceEpoch - started,
          'ok': false,
          'errorType': error.runtimeType.toString(),
        });
      }
      // Keep only recent measurements when a manual session stays open for hours.
      if (samples.length > 20000) samples.removeRange(0, 1000);
      if (started - lastReport >= 1000) {
        await File(args[2]).writeAsString('${jsonEncode(samples)}\n');
        lastReport = started;
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
  } finally {
    try {
      final frames = await driver.requestData('frame-stop');
      await File(args[4]).writeAsString('$frames\n');
    } catch (error) {
      await File(args[4])
          .writeAsString('${jsonEncode({'error': error.toString()})}\n');
    }
    try {
      // The window evidence must survive a Driver failure as well, so a review
      // can still see whether the window ever moved.
      await writePagingEvidence();
    } catch (_) {
      // A failed evidence write must not mask the journey's own result or leak
      // the VM connection; the coordinator fails on the missing artifact.
    }
    await File(args[2]).writeAsString('${jsonEncode(samples)}\n');
    await driver.close();
  }
}
