@Tags(<String>['visual'])
library;

import 'dart:async';
import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
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
  (name: '1280x800', size: Size(1280, 800), brightness: Brightness.dark),
  (name: '900x700', size: Size(900, 700), brightness: Brightness.light),
  (name: '760x720', size: Size(760, 720), brightness: Brightness.light),
];

const _activityLabel = '1 skill · 1 MCP · 1 LSP · 1 agent';
const _visualFontPath = 'test/assets/fonts/NotoSans-Variable.ttf';
const _visualFontFamily = 'PureStudioVisualTest';
const _visualFontFamilies = [
  _visualFontFamily,
  'Inter',
  'Noto Sans SC',
  'Segoe UI',
  'Consolas',
  'JetBrains Mono',
];
final _updateVisuals =
    Platform.environment['PURE_CAPTURE_VISUALS']?.toLowerCase() == 'true' ||
    const bool.fromEnvironment('PURE_CAPTURE_VISUALS');

class _VisualStudioApi extends DemoStudioApi {
  _VisualStudioApi([StudioState? state])
    : visualState = state ?? responsiveVisualState();

  final StudioState visualState;

  @override
  Future<StudioState> bootstrap() async => visualState;

  @override
  Future<List<ProviderUsageView>> loadProviderUsages() async =>
      visualState.providerUsages;

  @override
  Stream<Object> subscribeProductEvents() => const Stream.empty();

  @override
  Stream<SessionStreamFrame> subscribeSessionEvents(
    String sessionId, {
    int? afterSequence,
  }) => const Stream.empty();
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(_loadVisualFonts);

  for (final viewport in _visualViewports) {
    testWidgets(
      'capture chat at ${viewport.name} (${viewport.brightness.name})',
      (tester) async {
        _configureVisualView(tester, viewport.size);
        final chatBoundary = await _pumpVisual(
          tester,
          home: const StudioShell(),
          brightness: viewport.brightness,
        );
        expect(tester.takeException(), isNull);
        _expectVisualFontApplied(find.text(responsiveVisualSessionTitle));
        _expectAllTextTruncated(find.text(responsiveVisualSessionTitle));
        await _verifyVisual(
          chatBoundary,
          'chat-${viewport.name}.png',
          viewport.size,
        );
      },
    );

    testWidgets('capture activity popover at ${viewport.name} '
        '(${viewport.brightness.name})', (tester) async {
      _configureVisualView(tester, viewport.size);
      final chatBoundary = await _pumpVisual(
        tester,
        home: const StudioShell(),
        brightness: viewport.brightness,
      );
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
      await _verifyVisual(
        chatBoundary,
        'activity-popover-${viewport.name}.png',
        viewport.size,
      );
    });

    testWidgets('capture task activity popover at ${viewport.name} '
        '(${viewport.brightness.name})', (tester) async {
      _configureVisualView(tester, viewport.size);
      final chatBoundary = await _pumpVisual(
        tester,
        home: const StudioShell(),
        brightness: viewport.brightness,
        state: _taskVisualState(),
      );
      final activityTrigger = find.text(
        'Implementing · 1 skill · 1 MCP · 1 LSP · 1 agent',
      );
      expect(activityTrigger, findsOneWidget);
      await tester.ensureVisible(activityTrigger);
      await tester.pump();
      final triggerRect = tester.getRect(activityTrigger);
      await tester.tapAt(Offset(triggerRect.left + 8, triggerRect.center.dy));
      await tester.pump(const Duration(milliseconds: 250));
      expect(find.text('TASK COORDINATOR'), findsOneWidget);
      expect(find.text('WORK UNITS'), findsOneWidget);
      expect(find.text('MERGES AND CONFLICTS'), findsOneWidget);
      expect(find.text('REVIEWS'), findsOneWidget);
      expect(tester.takeException(), isNull);
      await _verifyVisual(
        chatBoundary,
        'task-activity-popover-${viewport.name}.png',
        viewport.size,
      );
    });

    testWidgets('capture provider settings at ${viewport.name} '
        '(${viewport.brightness.name})', (tester) async {
      _configureVisualView(tester, viewport.size);
      final settingsBoundary = await _pumpVisual(
        tester,
        home: const SettingsPage(),
        brightness: viewport.brightness,
      );
      expect(find.text('Search providers'), findsOneWidget);
      expect(find.text(responsiveVisualProviderName), findsOneWidget);
      _expectAllTextTruncated(find.text(responsiveVisualProviderName));
      expect(tester.takeException(), isNull);
      await _verifyVisual(
        settingsBoundary,
        'provider-settings-${viewport.name}.png',
        viewport.size,
      );
    });
  }
}

Future<void> _loadVisualFonts() async {
  final bytes = await File(_visualFontPath).readAsBytes();
  for (final family in _visualFontFamilies) {
    final loader = FontLoader(family)
      ..addFont(Future.value(ByteData.sublistView(bytes)));
    await loader.load();
  }
  final materialIcons = await rootBundle.load(
    'fonts/MaterialIcons-Regular.otf',
  );
  final iconLoader = FontLoader('MaterialIcons')
    ..addFont(Future.value(materialIcons));
  await iconLoader.load();
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
  required Brightness brightness,
  StudioState? state,
}) async {
  final boundaryKey = GlobalKey();
  await tester.pumpWidget(
    RepaintBoundary(
      key: boundaryKey,
      child: ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(_VisualStudioApi(state)),
        ],
        child: MaterialApp(
          debugShowCheckedModeBanner: false,
          locale: const Locale('en'),
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          theme: _visualTheme(brightness),
          home: home,
        ),
      ),
    ),
  );
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 600));
  return boundaryKey;
}

StudioState _taskVisualState() {
  final state = responsiveVisualState();
  return state.copyWith(
    sessions: [
      for (final session in state.sessions)
        session.copyWith(mode: StudioMode.task),
    ],
    runtime: state.runtime.copyWith(
      task: const TaskRuntimeView(
        runId: 'task-run-visual',
        phase: 'implementing',
        branch: 'codex/task-mode-orchestrator',
        expectedHead: '1234567890abcdef',
        statusMessage: 'Executor delivery ready for merge',
        workUnits: [
          TaskWorkUnitView(
            id: 'unit-visual',
            title: 'Implement coordinator activity projection',
            status: 'delivered',
            worktreePath: '.pure/worktrees/task-run-visual/agent-executor',
            branch: 'pure-task-task-run-visual-agent-executor',
            agentId: 'agent-executor',
          ),
        ],
        agents: [
          TaskAgentOutcomeView(
            agentId: 'agent-executor',
            role: 'executor',
            status: 'completed',
            initiatedBy: 'planner',
            requestedByCallId: 'call-spawn-visual',
            summary: 'Coordinator projection implemented',
            error: null,
            headCommit: 'abcdef1234567890',
          ),
          TaskAgentOutcomeView(
            agentId: 'agent-explorer',
            role: 'explorer',
            status: 'running',
            initiatedBy: 'planner',
            requestedByCallId: 'call-explore-visual',
            summary: 'Inspecting the affected design contract',
            error: null,
            headCommit: null,
          ),
        ],
        merges: [
          TaskMergeView(
            id: 'merge-visual',
            agentId: 'agent-executor',
            status: 'conflicted',
            mergeCommit: null,
            conflictFiles: ['lib/src/features/status/session_status_bar.dart'],
            resolutionSummary: null,
          ),
        ],
        reviews: [
          TaskReviewView(
            round: 1,
            headCommit: '1234567890abcdef',
            verdict: 'changesRequired',
            reviewerAgentId: 'agent-reviewer',
            summary: 'One responsive issue remains',
            designReferences: [
              'design/11-studio-ui.md#Flutter interaction and layout',
            ],
          ),
        ],
      ),
    ),
  );
}

ThemeData _visualTheme(Brightness brightness) {
  final theme = pureStudioTheme(brightness);
  return theme.copyWith(
    textTheme: theme.textTheme.apply(fontFamily: _visualFontFamily),
    primaryTextTheme: theme.primaryTextTheme.apply(
      fontFamily: _visualFontFamily,
    ),
    appBarTheme: theme.appBarTheme.copyWith(
      titleTextStyle: theme.appBarTheme.titleTextStyle?.copyWith(
        fontFamily: _visualFontFamily,
      ),
    ),
  );
}

Future<void> _verifyVisual(
  GlobalKey key,
  String filename,
  Size expectedSize,
) async {
  final imageFuture = captureImage(key.currentContext! as Element);
  await TestWidgetsFlutterBinding.instance.runAsync(() async {
    final image = await imageFuture;
    try {
      expect(image.width, expectedSize.width.toInt());
      expect(image.height, expectedSize.height.toInt());
      final data = await image.toByteData(format: ui.ImageByteFormat.png);
      final bytes = data!.buffer.asUint8List();
      expect(bytes.length, greaterThan(1000));

      final baseline = File('output/visual-check/$filename');
      if (!_updateVisuals) {
        expect(
          baseline.existsSync(),
          isTrue,
          reason:
              'Missing visual baseline $filename. Set '
              'PURE_CAPTURE_VISUALS=true to create it.',
        );
        final expectedBytes = baseline.readAsBytesSync();
        expect(
          _firstByteDifference(bytes, expectedBytes),
          isNull,
          reason:
              'Visual baseline drifted for $filename '
              '(current ${bytes.length} bytes, '
              'baseline ${expectedBytes.length} bytes). Set '
              'PURE_CAPTURE_VISUALS=true only when intentionally updating.',
        );
        return;
      }

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

int? _firstByteDifference(Uint8List current, Uint8List baseline) {
  if (current.length != baseline.length) {
    return math.min(current.length, baseline.length);
  }
  for (var index = 0; index < current.length; index++) {
    if (current[index] != baseline[index]) {
      return index;
    }
  }
  return null;
}

void _expectAllTextTruncated(Finder finder) {
  final paragraphs = [
    for (final element in finder.evaluate())
      if (element.renderObject case final RenderParagraph paragraph) paragraph,
  ];
  expect(paragraphs, isNotEmpty);
  expect(paragraphs.every((paragraph) => paragraph.didExceedMaxLines), isTrue);
}

void _expectVisualFontApplied(Finder finder) {
  final paragraphs = [
    for (final element in finder.evaluate())
      if (element.renderObject case final RenderParagraph paragraph) paragraph,
  ];
  expect(paragraphs, isNotEmpty);
  expect(
    paragraphs.every(
      (paragraph) => paragraph.text.style?.fontFamily == _visualFontFamily,
    ),
    isTrue,
  );
}
