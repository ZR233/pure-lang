import 'dart:convert';

import 'package:anywork/src/shared/studio_driver_state.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter_driver/driver_extension.dart';
import 'package:flutter_driver/flutter_driver.dart' hide find;
import 'package:flutter_test/flutter_test.dart';

import 'tool_probe.dart';

class ToolProbeExtension extends CommandExtension {
  @override
  String get commandKind => 'ToolProbe';
  @override
  Command deserialize(
    Map<String, String> params,
    DeserializeFinderFactory finderFactory,
    DeserializeCommandFactory commandFactory,
  ) => ToolProbe.deserialize(params, finderFactory);
  @override
  Future<Result> call(
    Command command,
    WidgetController prober,
    CreateFinderFactory finderFactory,
    CommandHandlerFactory handlerFactory,
  ) async {
    final probe = command as ToolProbe;
    final target = probe.finder is ByValueKey
        ? find.byKey(
            ValueKey<String>((probe.finder as ByValueKey).keyValue as String),
            skipOffstage: false,
          )
        : finderFactory.createFinder(probe.finder);
    if (probe.action == 'observe' && target.evaluate().isEmpty) {
      return _ProbeResult([
        {'mounted': false},
      ]);
    }
    Map<String, Object?> sample() {
      final box = target.evaluate().single.renderObject! as RenderBox;
      final offset = box.localToGlobal(Offset.zero);
      final nested = find.descendant(
        of: target,
        matching: find.byType(Scrollable),
      );
      return {
        'top': offset.dy,
        'left': offset.dx,
        'height': box.size.height,
        'scroll': jsonDecode(
          StudioDriverState.snapshotJson(),
        )['timelineScroll'],
        'nested': [
          for (final e in nested.evaluate())
            if (e is StatefulElement && e.state is ScrollableState)
              {
                'axis': (e.state as ScrollableState).position.axis.name,
                'pixels': (e.state as ScrollableState).position.pixels,
                'max': (e.state as ScrollableState).position.maxScrollExtent,
              },
        ],
      };
    }

    final samples = <Object?>[sample()];
    final box = target.evaluate().single.renderObject! as RenderBox;
    final top = box.localToGlobal(Offset.zero);
    if (probe.action == 'tap') {
      // Header, not the center of the whole expanded tile.
      await prober.tapAt(top + Offset(30, 20));
    } else if (probe.action == 'wheelOutside') {
      GestureBinding.instance.handlePointerEvent(
        PointerScrollEvent(
          position: top + Offset(4, box.size.height / 2),
          scrollDelta: Offset(0, probe.dy),
          kind: PointerDeviceKind.mouse,
        ),
      );
    } else if (probe.action == 'wheelInner') {
      final scrollables = find.descendant(
        of: target,
        matching: find.byType(Scrollable),
      );
      final candidates = scrollables
          .evaluate()
          .whereType<StatefulElement>()
          .where(
            (e) =>
                (e.state as ScrollableState).position.axis == Axis.vertical &&
                (e.state as ScrollableState).position.maxScrollExtent > 0,
          );
      if (candidates.isEmpty) {
        throw StateError('No vertically scrollable tool payload');
      }
      final inner = candidates.last.renderObject! as RenderBox;
      final viewport =
          find
                  .byKey(const ValueKey('timeline-scrollable'))
                  .evaluate()
                  .single
                  .renderObject!
              as RenderBox;
      final visible = (inner.localToGlobal(Offset.zero) & inner.size).intersect(
        viewport.localToGlobal(Offset.zero) & viewport.size,
      );
      if (visible.isEmpty) {
        throw StateError('Tool payload is outside the viewport');
      }
      GestureBinding.instance.handlePointerEvent(
        PointerScrollEvent(
          position: visible.center,
          scrollDelta: Offset(0, probe.dy),
          kind: PointerDeviceKind.mouse,
        ),
      );
    } else if (probe.action != 'observe') {
      throw ArgumentError.value(probe.action, 'action');
    }
    for (var i = 0; i < (probe.action == 'tap' ? 24 : 2); i++) {
      SchedulerBinding.instance.scheduleFrame();
      await SchedulerBinding.instance.endOfFrame;
      await Future<void>.delayed(const Duration(milliseconds: 16));
      samples.add(sample());
    }
    return _ProbeResult(samples);
  }
}

class _ProbeResult extends Result {
  _ProbeResult(this.samples);
  final List<Object?> samples;
  @override
  Map<String, dynamic> toJson() => {'samples': samples};
}
