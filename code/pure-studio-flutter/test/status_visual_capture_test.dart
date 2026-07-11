import 'dart:async';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pure_studio_flutter/src/app/theme/material3_theme.dart';
import 'package:pure_studio_flutter/src/data/frb/studio_api.dart';
import 'package:pure_studio_flutter/src/data/repositories/studio_repository.dart';
import 'package:pure_studio_flutter/src/domain/models/studio_models.dart';
import 'package:pure_studio_flutter/src/features/settings/settings_page.dart';
import 'package:pure_studio_flutter/src/features/shell/studio_shell.dart';
import 'package:pure_studio_flutter/src/l10n/app_localizations.dart';

import 'support/responsive_visual_fixture.dart';

const _visualViewports = [
  (name: '1280x800', size: Size(1280, 800)),
  (name: '900x700', size: Size(900, 700)),
  (name: '760x720', size: Size(760, 720)),
];

const _activityLabel = '1 skill · 1 MCP · 1 LSP · 2 agents';
const _captureVisuals = bool.fromEnvironment('PURE_CAPTURE_VISUALS');

class _VisualStudioApi extends DemoStudioApi {
  _VisualStudioApi() : visualState = responsiveVisualState();

  final StudioState visualState;

  @override
  Future<StudioState> bootstrap() async => visualState;

  @override
  Future<StudioState> loadSessionState(String sessionId) async => visualState;

  @override
  Future<List<ProviderUsageView>> loadProviderUsages() async =>
      visualState.providerUsages;

  @override
  Stream<Object> subscribeGlobalEvents() => const Stream.empty();

  @override
  Stream<Object> subscribeSessionEvents(String sessionId) =>
      const Stream.empty();
}

void main() {
  for (final viewport in _visualViewports) {
    testWidgets('capture chat at ${viewport.name}', (tester) async {
      if (!_captureVisuals) {
        return;
      }
      _configureVisualView(tester, viewport.size);
      final chatBoundary = await _pumpVisual(tester, home: const StudioShell());
      expect(tester.takeException(), isNull);
      _expectAllTextTruncated(find.text(responsiveVisualSessionTitle));
      await _capture(chatBoundary, 'chat-${viewport.name}.png', viewport.size);
    });

    testWidgets('capture activity popover at ${viewport.name}', (tester) async {
      if (!_captureVisuals) {
        return;
      }
      _configureVisualView(tester, viewport.size);
      final chatBoundary = await _pumpVisual(tester, home: const StudioShell());
      final activityTrigger = find.text(_activityLabel);
      expect(activityTrigger, findsOneWidget);
      await tester.ensureVisible(activityTrigger);
      await tester.pump();
      final triggerRect = tester.getRect(activityTrigger);
      await tester.tapAt(Offset(triggerRect.left + 8, triggerRect.center.dy));
      await tester.pump(const Duration(milliseconds: 250));
      expect(find.text('ACTIVE CAPABILITIES'), findsOneWidget);
      expect(find.text('SUBAGENTS'), findsOneWidget);
      _expectAllTextTruncated(find.text(responsiveVisualSessionTitle));
      expect(tester.takeException(), isNull);
      await _capture(
        chatBoundary,
        'activity-popover-${viewport.name}.png',
        viewport.size,
      );
    });

    testWidgets('capture provider settings at ${viewport.name}', (
      tester,
    ) async {
      if (!_captureVisuals) {
        return;
      }
      _configureVisualView(tester, viewport.size);
      final settingsBoundary = await _pumpVisual(
        tester,
        home: const SettingsPage(),
      );
      expect(find.text('Search providers'), findsOneWidget);
      expect(find.text(responsiveVisualProviderName), findsOneWidget);
      _expectAllTextTruncated(find.text(responsiveVisualProviderName));
      expect(tester.takeException(), isNull);
      await _capture(
        settingsBoundary,
        'provider-settings-${viewport.name}.png',
        viewport.size,
      );
    });
  }
}

void _configureVisualView(WidgetTester tester, Size size) {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
}

Future<GlobalKey> _pumpVisual(
  WidgetTester tester, {
  required Widget home,
}) async {
  final boundaryKey = GlobalKey();
  await tester.pumpWidget(
    RepaintBoundary(
      key: boundaryKey,
      child: ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(_VisualStudioApi())],
        child: MaterialApp(
          debugShowCheckedModeBanner: false,
          locale: const Locale('en'),
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          theme: pureStudioTheme(Brightness.light),
          home: home,
        ),
      ),
    ),
  );
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 600));
  return boundaryKey;
}

Future<void> _capture(GlobalKey key, String filename, Size expectedSize) async {
  final imageFuture = captureImage(key.currentContext! as Element);
  await TestWidgetsFlutterBinding.instance.runAsync(() async {
    final image = await imageFuture;
    try {
      expect(image.width, expectedSize.width.toInt());
      expect(image.height, expectedSize.height.toInt());
      final data = await image.toByteData(format: ui.ImageByteFormat.png);
      final bytes = data!.buffer.asUint8List();
      expect(bytes.length, greaterThan(1000));

      for (final directory in [
        Directory('output/visual-check'),
        Directory('../../.superpowers/sdd/task-4-screenshots'),
      ]) {
        directory.createSync(recursive: true);
        File('${directory.path}/$filename').writeAsBytesSync(bytes);
      }
    } finally {
      image.dispose();
    }
  });
}

void _expectAllTextTruncated(Finder finder) {
  final paragraphs = [
    for (final element in finder.evaluate())
      if (element.renderObject case final RenderParagraph paragraph) paragraph,
  ];
  expect(paragraphs, isNotEmpty);
  expect(paragraphs.every((paragraph) => paragraph.didExceedMaxLines), isTrue);
}
