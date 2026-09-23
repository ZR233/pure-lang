import 'dart:convert';
import 'dart:io';

import 'flutter_driver_session.dart';

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
  final samples = <Map<String, Object?>>[];
  var lastReport = 0;
  Map<String, Object?>? authoritative;
  Map<String, Object?>? persistenceDiagnostics;
  var paginationStarted = false;
  final driver = await FlutterDriverSession.connect(vmServiceUrl: args[0]);
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
        final window = snapshot['timelineWindow'];
        final ids = window is Map ? window['itemIds'] : null;
        final bounded =
            window is Map &&
            ids is List &&
            ids.length <= 96 &&
            ids.toSet().length == ids.length &&
            (window['historyCount'] as num? ?? 0) <= 96 &&
            (window['overlayCount'] as num? ?? -1) == 0;
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
        if (!paginationStarted &&
            workspace is Map &&
            persistence is Map &&
            usage is Map &&
            (usage['outputTokens'] as num? ?? 0) >= 20000 &&
            workspace['isBusy'] == false &&
            workspace['syncState'] == 'ready' &&
            bounded &&
            persistence['kind'] == 'ready' &&
            persistence['pendingCommits'] == 0) {
          paginationStarted = true;
          await driver.requestData('load-older');
          await driver.requestData('load-older');
          continue;
        }
        if (!await finished.exists() &&
            paginationStarted &&
            workspace is Map &&
            persistence is Map &&
            usage is Map &&
            (usage['outputTokens'] as num? ?? 0) >= 20000 &&
            workspace['isBusy'] == false &&
            workspace['syncState'] == 'ready' &&
            bounded &&
            ids.length == 96 &&
            diverse &&
            persistence['kind'] == 'ready' &&
            persistence['pendingCommits'] == 0) {
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
          'itemCount': ids is List ? ids.length : null,
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
          'historyCount': window is Map ? window['historyCount'] : null,
          'overlayCount': window is Map ? window['overlayCount'] : null,
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
    await File(args[2]).writeAsString('${jsonEncode(samples)}\n');
    await driver.close();
  }
}
