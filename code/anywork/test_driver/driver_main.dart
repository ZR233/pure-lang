import 'dart:convert';

import 'package:anywork/main.dart' as studio;
import 'package:anywork/src/app/studio_shutdown.dart';
import 'package:anywork/src/data/repositories/studio_repository.dart';
import 'package:anywork/src/shared/studio_driver_state.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter_driver/driver_extension.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'raw_tap_extension.dart';

/// Native-only Driver entrypoint. Product and release builds use lib/main.dart.
void main() {
  if (const bool.fromEnvironment('dart.vm.product')) {
    throw StateError('Flutter Driver mode is unavailable in product builds');
  }
  enableFlutterDriverExtension(
    handler: _handleDriverData,
    commands: <CommandExtension>[RawTapCommandExtension()],
  );
  SchedulerBinding.instance.addTimingsCallback(_recordFrameTimings);
  _container = ProviderContainer();
  studio.bootstrapStudio(container: _container);
}

late final ProviderContainer _container;
Future<void>? _shutdownTask;
bool _recordingFrames = false;
int _frameCount = 0;
int _slowFrames = 0;
int _verySlowFrames = 0;
int _maxFrameMicros = 0;
final List<Map<String, num>> _frameSamples = [];

void _recordFrameTimings(List<FrameTiming> timings) {
  if (!_recordingFrames) return;
  final completedAt = DateTime.now().millisecondsSinceEpoch;
  for (final timing in timings) {
    final elapsed = timing.totalSpan.inMicroseconds;
    _frameCount++;
    if (elapsed > 16667) _slowFrames++;
    if (elapsed > 33333) _verySlowFrames++;
    if (elapsed > _maxFrameMicros) _maxFrameMicros = elapsed;
    _frameSamples.add({
      'completedUnixMillis': completedAt,
      'totalMillis': elapsed / 1000,
      'vsyncOverheadMillis': timing.vsyncOverhead.inMicroseconds / 1000,
      'buildMillis': timing.buildDuration.inMicroseconds / 1000,
      'rasterMillis': timing.rasterDuration.inMicroseconds / 1000,
    });
  }
  if (_frameSamples.length > 8192) _frameSamples.removeRange(0, 4096);
}

Future<String> _handleDriverData(String? message) async {
  switch (message) {
    case 'frame-start':
      _frameCount = 0;
      _slowFrames = 0;
      _verySlowFrames = 0;
      _maxFrameMicros = 0;
      _frameSamples.clear();
      _recordingFrames = true;
      return jsonEncode({'recording': true});
    case 'frame-stop':
      _recordingFrames = false;
      return jsonEncode({
        'frames': _frameCount,
        'over16Millis': _slowFrames,
        'over33Millis': _verySlowFrames,
        'maxFrameMillis': _maxFrameMicros / 1000,
        'samples': _frameSamples,
      });
    case 'snapshot':
      final state = switch (_container.read(studioControllerProvider)) {
        AsyncData(:final value) => value,
        _ => null,
      };
      if (state != null) StudioDriverState.publishState(state);
      return StudioDriverState.snapshotJson();
    case 'thread-current':
      final state = switch (_container.read(studioControllerProvider)) {
        AsyncData(:final value) => value,
        _ => null,
      };
      final threadId = state?.selectedThreadId;
      if (threadId == null) return jsonEncode({'outputTokens': null});
      final snapshot = await _container
          .read(studioApiProvider)
          .readThreadSnapshot(threadId);
      return jsonEncode({
        'outputTokens': snapshot.runtime.completionTokens,
        'revision': snapshot.revision,
      });
    case 'persistence-queue':
      final queue = await _container
          .read(studioControllerProvider.notifier)
          .readPersistenceQueue();
      return jsonEncode({
        'pendingOperations': queue?.pendingOperations,
        'threads': [
          for (final thread in queue?.threads ?? const [])
            {
              'fault': thread.fault,
              'generation': thread.faultGeneration,
              'stateDirty': thread.stateDirtyRevision,
              'stateDurable': thread.stateDurableRevision,
              'historyAdmitted': thread.historyAdmittedSequence,
              'historyDurable': thread.historyDurableSequence,
              'pending': thread.pendingOperations,
              'error': thread.lastError,
            },
        ],
      });
    case 'load-older':
      final state = switch (_container.read(studioControllerProvider)) {
        AsyncData(:final value) => value,
        _ => null,
      };
      final threadId = state?.selectedThreadId;
      if (threadId == null) return jsonEncode({'loaded': false});
      await _container
          .read(studioControllerProvider.notifier)
          .loadOlderHistory(threadId);
      return jsonEncode({'loaded': true});
    case 'shutdown':
      try {
        await (_shutdownTask ??= _runShutdown());
        return jsonEncode({'shutdown': 'completed'});
      } on Object {
        _shutdownTask = null;
        return jsonEncode({'shutdown': 'failed'});
      }
    default:
      return jsonEncode({'error': 'unsupported driver request'});
  }
}

Future<void> _runShutdown() async {
  final api = _container.read(studioApiProvider);
  final progress = _container.read(
    studioShutdownProgressStateProvider.notifier,
  );
  await runStudioShutdown(api, progress.update);
}
