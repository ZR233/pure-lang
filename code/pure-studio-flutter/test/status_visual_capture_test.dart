import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pure_studio_flutter/src/app/theme/material3_theme.dart';
import 'package:pure_studio_flutter/src/data/frb/studio_api.dart';
import 'package:pure_studio_flutter/src/data/repositories/studio_repository.dart';
import 'package:pure_studio_flutter/src/features/shell/studio_shell.dart';
import 'package:pure_studio_flutter/src/l10n/app_localizations.dart';

void main() {
  testWidgets('capture status detail screenshots', (tester) async {
    if (!const bool.fromEnvironment('PURE_CAPTURE_VISUALS')) {
      return;
    }

    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final boundaryKey = GlobalKey();
    await tester.pumpWidget(
      RepaintBoundary(
        key: boundaryKey,
        child: ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(DemoStudioApi())],
          child: MaterialApp(
            locale: const Locale('en'),
            localizationsDelegates: AppLocalizations.localizationsDelegates,
            supportedLocales: AppLocalizations.supportedLocales,
            theme: pureStudioTheme(Brightness.light),
            home: const StudioShell(),
          ),
        ),
      ),
    );
    await tester.pump(const Duration(milliseconds: 600));

    final outputDir = Directory('output/visual-check');
    outputDir.createSync(recursive: true);
    await _capture(boundaryKey, '${outputDir.path}/status-baseline.png');

    final contextGesture = await tester.createGesture(
      kind: ui.PointerDeviceKind.mouse,
    );
    await contextGesture.addPointer();
    await contextGesture.moveTo(
      tester.getCenter(find.bySemanticsLabel('Context')),
    );
    await tester.pump(const Duration(milliseconds: 250));
    await _capture(boundaryKey, '${outputDir.path}/status-context-detail.png');
    await contextGesture.removePointer();
    await tester.pump(const Duration(milliseconds: 250));

    final capabilityText = find.text('2 skills · 1 MCP · 1 LSP');
    final capabilityRect = tester.getRect(capabilityText);
    await tester.tapAt(
      Offset(capabilityRect.left + 8, capabilityRect.center.dy),
    );
    await tester.pump(const Duration(milliseconds: 250));
    await _capture(
      boundaryKey,
      '${outputDir.path}/status-capability-detail.png',
    );

    final agentText = find.text('4 agents · 2 running');
    final agentRect = tester.getRect(agentText);
    await tester.tapAt(Offset(agentRect.left + 8, agentRect.center.dy));
    await tester.pump(const Duration(milliseconds: 250));
    await _capture(boundaryKey, '${outputDir.path}/status-agent-detail.png');
  });
}

Future<void> _capture(GlobalKey key, String path) async {
  final boundary =
      key.currentContext!.findRenderObject()! as RenderRepaintBoundary;
  final image = await boundary.toImage(pixelRatio: 1);
  final data = await image.toByteData(format: ui.ImageByteFormat.png);
  File(path).writeAsBytesSync(data!.buffer.asUint8List());
}
