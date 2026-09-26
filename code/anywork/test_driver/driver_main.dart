import 'dart:convert';
import 'dart:io';

import 'package:anywork/main.dart' as studio;
import 'package:anywork/src/app/studio_shutdown.dart';
import 'package:anywork/src/data/repositories/studio_repository.dart';
import 'package:anywork/src/shared/studio_driver_state.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_driver/driver_extension.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'raw_tap_extension.dart';
import 'pointer_scroll_extension.dart';
import 'key_press_extension.dart';

/// Native-only Driver entrypoint. Product and release builds use lib/main.dart.
void main() {
  if (const bool.fromEnvironment('dart.vm.product')) {
    throw StateError('Flutter Driver mode is unavailable in product builds');
  }
  enableFlutterDriverExtension(
    handler: _handleDriverData,
    commands: <CommandExtension>[
      RawTapCommandExtension(),
      PointerScrollCommandExtension(),
      KeyPressCommandExtension(),
    ],
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
    case 'pid':
      // Acceptance locates the X11 window by `_NET_WM_PID`, so the driver must
      // expose its own process id. Product builds use lib/main.dart, so this
      // stays inside the Driver-only entrypoint.
      return jsonEncode({'pid': pid});
    case 'statistics':
      final state = switch (_container.read(studioControllerProvider)) {
        AsyncData(:final value) => value,
        _ => null,
      };
      final performance = state?.modelPerformance;
      return jsonEncode({
        'revision': performance?.revision,
        'statisticsPending': performance?.statisticsPending,
        'statisticsGap': performance?.statisticsGap,
        'readFailed': performance?.readFailed,
        'summaries': [
          for (final summary in performance?.summaries ?? const [])
            {
              'providerInstanceId': summary.providerInstanceId,
              'model': summary.model,
              'effort': summary.reasoningEffort,
              'samples': summary.sampleCount,
              'tokens': summary.completionTokens,
              'tokensPerSecond': summary.tokensPerSecond,
            },
        ],
        'history': [
          for (final sample in performance?.history ?? const [])
            {
              'providerInstanceId': sample.providerInstanceId,
              'model': sample.model,
              'effort': sample.reasoningEffort,
              'tokens': sample.completionTokens,
              'ttftMillis': sample.ttftMillis,
              'decodeMillis': sample.decodeMillis,
              'responseMillis': sample.totalResponseMillis,
              'tokensPerSecond': sample.tokensPerSecond,
            },
        ],
      });
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
    case 'selection-body':
      // Read-only: the rendered split of the long assistant body.
      return jsonEncode(<String, Object?>{'ok': true, ...?_renderedLongBody()});
    case 'selection-select-all':
      return _handleSelectAll();
    case 'selection-copy':
      return _handleCopySelected();
    case 'selection-state':
      return _handleSelectionState();
    case 'shutdown':
      try {
        await (_shutdownTask ??= _runShutdown());
        return jsonEncode({'shutdown': 'completed'});
      } on Object catch (error, stackTrace) {
        debugPrint('driver_shutdown_error=$error\n$stackTrace');
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

// ---------------------------------------------------------------------------
// Cross-block text selection acceptance.
//
// The production timeline renders a long plain-text reply through
// `_PlainBodyText`, which seals the body into bounded chunks that each become a
// real `RenderParagraph`. This Driver-only bridge lets the acceptance journey
// drive the *real* `SelectionArea` selection and the product's own context-menu
// copy callback, then read the platform clipboard back. It never writes the
// domain text to the clipboard itself: every observation comes from the live
// element/render tree or from `Clipboard.getData`.
//
// These helpers live only in the Driver entrypoint; the product timeline widget
// and model are not touched.

/// The key prefixes the production `_PlainBodyText` gives its sealed chunks and
/// its trailing open chunk.
const _plainChunkKeyPrefix = 'plain-chunk-';
const _plainOpenKeyPrefix = 'plain-open-';

void _visitElements(Element element, void Function(Element) visit) {
  visit(element);
  element.visitChildren((Element child) => _visitElements(child, visit));
}

/// Every production body chunk `Text` currently in the element tree, in paint
/// order.
List<Element> _longBodyTextElements() {
  final root = WidgetsBinding.instance.rootElement;
  if (root == null) return const <Element>[];
  final result = <Element>[];
  _visitElements(root, (Element element) {
    final widget = element.widget;
    if (widget is! Text) return;
    final key = widget.key;
    if (key is! ValueKey<String>) return;
    final value = key.value;
    if (value.startsWith(_plainChunkKeyPrefix) ||
        value.startsWith(_plainOpenKeyPrefix)) {
      result.add(element);
    }
  });
  return result;
}

/// The `RenderParagraph` a chunk `Text` actually painted into.
RenderParagraph? _renderParagraphOf(Element element) {
  RenderParagraph? result;
  void visit(Element current) {
    if (result != null) return;
    if (current is RenderObjectElement) {
      final renderObject = current.renderObject;
      if (renderObject is RenderParagraph) {
        result = renderObject;
        return;
      }
    }
    current.visitChildren(visit);
  }

  visit(element);
  return result;
}

/// The `SelectableRegionState` (the real `SelectionArea`) above the body chunks.
SelectableRegionState? _longBodySelectableRegion() {
  final elements = _longBodyTextElements();
  if (elements.isEmpty) return null;
  SelectableRegionState? region;
  elements.first.visitAncestorElements((Element ancestor) {
    if (ancestor is StatefulElement &&
        ancestor.state is SelectableRegionState) {
      region = ancestor.state as SelectableRegionState;
      return false;
    }
    return true;
  });
  return region;
}

/// The rendered body split: how many real paragraphs carry it and what they
/// concatenate to. Reading the `RenderParagraph` text keeps the evidence at the
/// render layer instead of the domain model.
Map<String, Object?>? _renderedLongBody() {
  final elements = _longBodyTextElements();
  if (elements.isEmpty) return null;
  final paragraphs = <RenderParagraph>[];
  final buffer = StringBuffer();
  for (final element in elements) {
    final paragraph = _renderParagraphOf(element);
    if (paragraph != null) {
      paragraphs.add(paragraph);
      buffer.write(paragraph.text.toPlainText());
      continue;
    }
    final widget = element.widget as Text;
    buffer.write(widget.data ?? widget.textSpan?.toPlainText() ?? '');
  }
  final rendered = buffer.toString();
  return <String, Object?>{
    'paragraphCount': paragraphs.length,
    'chunkCount': elements.length,
    'characters': rendered.length,
    'head': _headOf(rendered),
    'tail': _tailOf(rendered),
    'hash': _fnv1a64Hex(rendered),
  };
}

List<String> _menuTypes(SelectableRegionState region) {
  try {
    return <String>[
      for (final item in region.contextMenuButtonItems) item.type.name,
    ];
  } on Object {
    return const <String>[];
  }
}

ContextMenuButtonItem? _copyItemOf(SelectableRegionState region) {
  try {
    for (final item in region.contextMenuButtonItems) {
      if (item.type == ContextMenuButtonType.copy) return item;
    }
  } on Object {
    return null;
  }
  return null;
}

/// Pumps one frame so the selection geometry can settle. Bounded so a frame the
/// acceptance window cannot produce never wedges the Driver request; the copy
/// availability poll below is the real observation.
Future<void> _pumpFrame() async {
  final binding = SchedulerBinding.instance;
  binding.scheduleFrame();
  try {
    await binding.endOfFrame.timeout(const Duration(milliseconds: 120));
  } on Object {
    // See above: a missing frame is not a selection result.
  }
}

/// Selects the whole timeline region and waits (bounded) until the selection is
/// actually copyable, so the real context-menu copy callback becomes available.
Future<bool> _ensureCopyable(SelectableRegionState region) async {
  for (var attempt = 0; attempt < 60; attempt += 1) {
    if (_copyItemOf(region) != null) return true;
    if (attempt == 0 || attempt == 20 || attempt == 40) region.selectAll();
    await _pumpFrame();
  }
  return _copyItemOf(region) != null;
}

Future<String> _handleSelectAll() async {
  final region = _longBodySelectableRegion();
  if (region == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'reason': 'no production body chunk paragraphs are rendered',
    });
  }
  region.selectAll();
  final copyable = await _ensureCopyable(region);
  return jsonEncode(<String, Object?>{
    'ok': true,
    'copyable': copyable,
    'menuTypes': _menuTypes(region),
    ...?_renderedLongBody(),
  });
}

Future<String> _handleCopySelected() async {
  final region = _longBodySelectableRegion();
  if (region == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'reason': 'no production body chunk paragraphs are rendered',
    });
  }
  final copyable = await _ensureCopyable(region);
  final item = _copyItemOf(region);
  if (!copyable || item == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'reason': 'the timeline selection never became copyable',
      'menuTypes': _menuTypes(region),
      ...?_renderedLongBody(),
    });
  }
  // Reset the clipboard first so the readback can only come from this copy.
  await Clipboard.setData(const ClipboardData(text: ''));
  // Re-run the real select-all with no frame in between and copy immediately:
  // the product `_copy()` reads the selected content synchronously, so the copy
  // can never race a rebuild that seals another chunk and leaves it unselected.
  region.selectAll();
  final copyItem = _copyItemOf(region);
  if (copyItem == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'reason': 'the re-selected timeline region is not copyable',
      'menuTypes': _menuTypes(region),
      ...?_renderedLongBody(),
    });
  }
  final startedAt = DateTime.now();
  copyItem.onPressed?.call();
  String? copied;
  for (var attempt = 0; attempt < 400; attempt += 1) {
    await Future<void>.delayed(const Duration(milliseconds: 25));
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final text = data?.text;
    if (text != null && text.isNotEmpty) {
      copied = text;
      break;
    }
  }
  final copyMillis = DateTime.now().difference(startedAt).inMilliseconds;
  // Hide only the toolbar; the acceptance journey cancels the selection with a
  // real click and re-reads the region state afterwards.
  region.hideToolbar();
  await _pumpFrame();
  final copyableAfterCopy = _copyItemOf(region) != null;
  return jsonEncode(<String, Object?>{
    'ok': copied != null,
    'copied': copied,
    'copiedLength': copied?.length,
    'copiedHead': copied == null ? null : _headOf(copied),
    'copiedTail': copied == null ? null : _tailOf(copied),
    'copiedHash': copied == null ? null : _fnv1a64Hex(copied),
    'copyMillis': copyMillis,
    'copyableAfterCopy': copyableAfterCopy,
    ...?_renderedLongBody(),
  });
}

/// Whether the region still holds a copyable (uncollapsed) selection.
Future<String> _handleSelectionState() async {
  final region = _longBodySelectableRegion();
  if (region == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'reason': 'no production body chunk paragraphs are rendered',
    });
  }
  return jsonEncode(<String, Object?>{
    'ok': true,
    'copyable': _copyItemOf(region) != null,
    'menuTypes': _menuTypes(region),
  });
}

String _headOf(String text) =>
    text.substring(0, text.length < 64 ? text.length : 64);

String _tailOf(String text) =>
    text.substring(text.length < 64 ? 0 : text.length - 64);

/// FNV-1a over the code units, formatted as hex.
///
/// Deterministic across runs and isolates, so the acceptance script can compare
/// the clipboard readback against the canonical fixture body without shipping
/// the whole payload twice.
String _fnv1a64Hex(String text) {
  var hash = 0xcbf29ce484222325;
  for (var index = 0; index < text.length; index += 1) {
    hash ^= text.codeUnitAt(index);
    hash = hash * 0x100000001b3;
  }
  return hash.toRadixString(16);
}
