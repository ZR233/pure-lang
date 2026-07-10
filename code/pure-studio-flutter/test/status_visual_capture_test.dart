import 'dart:async';
import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pure_studio_flutter/src/app/theme/material3_theme.dart';
import 'package:pure_studio_flutter/src/data/frb/studio_api.dart';
import 'package:pure_studio_flutter/src/data/repositories/studio_repository.dart';
import 'package:pure_studio_flutter/src/domain/models/studio_models.dart';
import 'package:pure_studio_flutter/src/features/settings/settings_page.dart';
import 'package:pure_studio_flutter/src/features/shell/studio_shell.dart';
import 'package:pure_studio_flutter/src/l10n/app_localizations.dart';

const _visualViewports = [
  (name: '1280x800', size: Size(1280, 800)),
  (name: '900x700', size: Size(900, 700)),
  (name: '760x720', size: Size(760, 720)),
];

const _activityLabel = '2 skills · 1 MCP · 1 LSP · 2 agents';
const _captureVisuals = bool.fromEnvironment('PURE_CAPTURE_VISUALS');

class _VisualStudioApi extends DemoStudioApi {
  _VisualStudioApi() : visualState = _visualState();

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
      expect(find.text('DeepSeek'), findsWidgets);
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

StudioState _visualState() {
  final timestamp = DateTime.fromMillisecondsSinceEpoch(1735689600000);
  const project = StudioProject(
    id: 'project-1',
    name: 'pure-lang',
    path: r'C:\Users\zhoudongsheng\Documents\opensource\pure-lang',
  );
  final session = StudioSession(
    id: 'session-1',
    projectId: project.id,
    title: 'Responsive Pure Studio layout review',
    mode: CompileMode.auto,
    updatedAt: timestamp,
  );
  final messages = [
    TimelineMessage(
      id: 'message-user',
      sessionId: session.id,
      role: 'user',
      createdAt: timestamp,
      sequence: 0,
    ),
    TimelineMessage(
      id: 'message-assistant',
      sessionId: session.id,
      role: 'assistant',
      createdAt: timestamp,
      sequence: 1,
    ),
  ];
  final parts = [
    TimelinePartSnapshot(
      id: 'part-user',
      messageId: messages.first.id,
      sessionId: session.id,
      turnId: 'turn-1',
      type: TimelinePartType.text,
      order: 0,
      revision: 0,
      sequence: 0,
      text:
          'Check the chat, activity summary, and provider settings at every target viewport.',
      status: 'completed',
      createdAt: timestamp,
      updatedAt: timestamp,
      textChannel: TimelineTextChannel.user,
    ),
    TimelinePartSnapshot(
      id: 'part-assistant',
      messageId: messages.last.id,
      sessionId: session.id,
      turnId: 'turn-1',
      type: TimelinePartType.text,
      order: 0,
      revision: 0,
      sequence: 1,
      text:
          '### Responsive verification\n\n'
          '- Conversation content remains readable.\n'
          '- Status details stay above their trigger.\n'
          '- Provider rows keep actions accessible.',
      status: 'completed',
      createdAt: timestamp,
      updatedAt: timestamp,
      textChannel: TimelineTextChannel.finalAnswer,
    ),
  ];
  const providers = [
    ProviderSettingsView(
      id: 'deepseek',
      templateKind: 'deepseek',
      name: 'DeepSeek',
      subtitle: 'DeepSeek Platform',
      baseUrl: 'https://api.deepseek.com',
      hasBearerToken: true,
      defaultModel: 'deepseek-reasoner',
      models: [
        ProviderModelView(
          slug: 'deepseek-reasoner',
          displayName: 'DeepSeek Reasoner',
          reasoningEfforts: ['high', 'max'],
        ),
        ProviderModelView(
          slug: 'deepseek-v4-flash',
          displayName: 'DeepSeek V4 Flash',
          reasoningEfforts: ['high', 'max'],
        ),
      ],
      status: 'ready',
      usageLabel: 'Balance available',
      modelCount: '2',
      providerKind: 'deep_seek',
    ),
    ProviderSettingsView(
      id: 'zhipu-coding-plan',
      templateKind: 'zhipu-coding-plan',
      name: 'Zhipu Coding Plan',
      subtitle: 'Zhipu Platform',
      baseUrl: 'https://open.bigmodel.cn/api/coding/paas/v4',
      hasBearerToken: true,
      defaultModel: 'glm-5.2',
      models: [
        ProviderModelView(
          slug: 'glm-5.2',
          displayName: 'GLM-5.2',
          reasoningEfforts: ['enabled'],
        ),
      ],
      status: 'ready',
      usageLabel: 'Coding plan ready',
      modelCount: '1',
      providerKind: 'zhipu',
    ),
  ];
  const providerUsages = [
    ProviderUsageView(
      providerId: 'deepseek',
      updatedAt: 1735689600,
      status: 'ready',
      usageKind: 'deepseekBalance',
      balance: DeepSeekBalanceUsageView(
        isAvailable: true,
        balances: [
          DeepSeekBalanceInfoView(
            currency: 'CNY',
            totalBalance: '88.00',
            grantedBalance: '8.00',
            toppedUpBalance: '80.00',
          ),
        ],
      ),
    ),
    ProviderUsageView(
      providerId: 'zhipu-coding-plan',
      updatedAt: 1735689600,
      status: 'ready',
      usageKind: 'zhipuCodingPlan',
      codingPlan: ZhipuCodingPlanUsageView(
        level: 'Pro',
        limits: [
          ZhipuQuotaLimitView(
            window: 'fiveHour',
            label: 'five hour',
            percentage: 75,
            total: 100,
            remaining: 25,
            nextResetAt: 1735689600,
            usageDetails: [],
          ),
          ZhipuQuotaLimitView(
            window: 'weekly',
            label: 'weekly',
            percentage: 50,
            total: 200,
            remaining: 100,
            nextResetAt: 1735689600,
            usageDetails: [],
          ),
          ZhipuQuotaLimitView(
            window: 'mcpMonthly',
            label: 'mcp',
            percentage: 20,
            nextResetAt: 1735689600,
            usageDetails: [],
          ),
        ],
      ),
    ),
  ];
  return StudioState(
    projects: const [project],
    sessions: [session],
    messagesBySession: {session.id: messages},
    partSnapshotsBySession: {
      session.id: {for (final part in parts) part.id: part},
    },
    agentsBySession: {
      session.id: {
        'agent-reviewer': StudioAgentView(
          id: 'agent-reviewer',
          sessionId: session.id,
          path: 'root/reviewer',
          role: 'reviewer',
          task: 'Audit responsive layout',
          status: 'running',
          summary: 'Checking the activity popover geometry.',
          updatedAt: timestamp,
        ),
        'agent-worker': StudioAgentView(
          id: 'agent-worker',
          sessionId: session.id,
          path: 'root/worker',
          role: 'worker',
          task: 'Capture viewport screenshots',
          status: 'completed',
          updatedAt: timestamp,
        ),
      },
    },
    providers: providers,
    defaultProviderId: 'deepseek',
    providerUsages: providerUsages,
    roles: const [
      RoleSettingsView(
        key: 'planner',
        providerId: 'deepseek',
        model: 'deepseek-reasoner',
        effort: 'high',
      ),
    ],
    mcpServers: const [],
    selectedProjectId: project.id,
    selectedSessionId: session.id,
    permissionMode: PermissionMode.requestApproval,
    turnPhase: TurnPhase.idle,
    runtime: const SessionRuntimeView(
      model: 'deepseek-reasoner',
      contextTokens: 42000,
      contextWindow: 100000,
      totalTokens: 128000,
      costLabel: 'CNY 12.34',
      activeSkills: ['flutter-ui', 'verification-before-completion'],
      activeMcpServers: ['dart'],
      activeLspServers: ['rust-analyzer'],
      agentCount: 2,
    ),
    pendingInteractions: const [],
  );
}
