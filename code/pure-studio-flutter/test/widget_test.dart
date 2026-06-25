import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pure_studio_flutter/src/data/frb/studio_api.dart';
import 'package:pure_studio_flutter/src/data/repositories/studio_repository.dart';
import 'package:pure_studio_flutter/src/domain/models/studio_models.dart';
import 'package:pure_studio_flutter/src/features/settings/settings_page.dart';
import 'package:pure_studio_flutter/src/features/shell/studio_shell.dart';
import 'package:pure_studio_flutter/src/features/timeline/markdown_repair.dart';
import 'package:pure_studio_flutter/src/features/timeline/timeline_view.dart';
import 'package:pure_studio_flutter/src/l10n/app_localizations.dart';

void main() {
  test(
    'composer submit waits for FRB events before timeline changes',
    () async {
      final api = _FakeStudioApi(_emptyState());
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      container.read(studioControllerProvider.notifier).updateComposer('hello');

      await container.read(studioControllerProvider.notifier).submitComposer();

      var state = container.read(studioControllerProvider).requireValue;
      expect(state.composerText, isEmpty);
      expect(state.turnPhase, TurnPhase.waitingForModel);
      expect(state.selectedMessages, isEmpty);

      api.emitSession(
        StudioBridgeEvent(
          kindType: 'messageUpdated',
          sessionId: 'session-1',
          payload: {
            'message': {
              'messageId': 'turn-1:assistant',
              'sessionId': 'session-1',
              'turnId': 'turn-1',
              'role': 'assistant',
              'status': 'streaming',
              'createdAt': 1,
              'updatedAt': 1,
            },
          },
        ),
      );
      api.emitSession(
        StudioBridgeEvent(
          kindType: 'messagePartUpdated',
          sessionId: 'session-1',
          payload: {
            'part': {
              'partId': 'part-1',
              'messageId': 'turn-1:assistant',
              'sessionId': 'session-1',
              'turnId': 'turn-1',
              'partType': 'text',
              'order': 0,
              'status': 'streaming',
              'createdAt': 1,
              'updatedAt': 1,
              'text': 'hel',
            },
          },
        ),
      );
      api.emitSession(
        StudioBridgeEvent(
          kindType: 'messagePartDelta',
          sessionId: 'session-1',
          payload: {
            'delta': {
              'sessionId': 'session-1',
              'messageId': 'turn-1:assistant',
              'partId': 'part-1',
              'field': 'text',
              'delta': 'lo',
            },
          },
        ),
      );
      await pumpEventQueue();

      state = container.read(studioControllerProvider).requireValue;
      expect(state.selectedMessages.single.role, 'assistant');
      expect(state.selectedMessages.single.parts.single.text, 'hello');
    },
  );

  test('timeline deltas use overlay revision guards', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      StudioBridgeEvent(
        kindType: 'messageUpdated',
        sessionId: 'session-1',
        payload: {
          'message': {
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'role': 'assistant',
            'status': 'streaming',
            'createdAt': 1,
            'updatedAt': 1,
          },
        },
      ),
    );
    api.emitSession(
      StudioBridgeEvent(
        kindType: 'messagePartUpdated',
        sessionId: 'session-1',
        payload: {
          'part': {
            'partId': 'part-1',
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'partType': 'text',
            'order': 7,
            'revision': 0,
            'status': 'streaming',
            'createdAt': 1,
            'updatedAt': 1,
            'textChannel': 'commentary',
            'text': '',
          },
        },
      ),
    );
    for (final revision in [1, 1, 2]) {
      api.emitSession(
        StudioBridgeEvent(
          kindType: 'messagePartDelta',
          sessionId: 'session-1',
          payload: {
            'delta': {
              'sessionId': 'session-1',
              'messageId': 'turn-1:assistant',
              'partId': 'part-1',
              'revision': revision,
              'field': 'text',
              'delta': revision == 1 ? 'a' : 'b',
            },
          },
        ),
      );
    }
    await pumpEventQueue();

    var state = container.read(studioControllerProvider).requireValue;
    var part = state.selectedMessages.single.parts.single;
    expect(part.text, 'ab');
    expect(part.order, 7);
    expect(part.textChannel, TimelineTextChannel.commentary);
    expect(state.partSnapshotsBySession['session-1']!['part-1']!.text, '');

    api.emitSession(
      StudioBridgeEvent(
        kindType: 'messagePartUpdated',
        sessionId: 'session-1',
        payload: {
          'part': {
            'partId': 'part-1',
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'partType': 'text',
            'order': 7,
            'revision': 2,
            'status': 'completed',
            'createdAt': 1,
            'updatedAt': 2,
            'textChannel': 'commentary',
            'text': 'snapshot',
          },
        },
      ),
    );
    api.emitSession(
      StudioBridgeEvent(
        kindType: 'messagePartDelta',
        sessionId: 'session-1',
        payload: {
          'delta': {
            'sessionId': 'session-1',
            'messageId': 'turn-1:assistant',
            'partId': 'part-1',
            'revision': 3,
            'field': 'text',
            'delta': 'late',
          },
        },
      ),
    );
    await pumpEventQueue();

    state = container.read(studioControllerProvider).requireValue;
    part = state.selectedMessages.single.parts.single;
    expect(part.text, 'snapshot');
    expect(state.partOverlaysBySession['session-1'], isEmpty);
  });

  test('timeline snapshot wins over same tick delta batch', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      StudioBridgeEvent(
        kindType: 'messageUpdated',
        sessionId: 'session-1',
        payload: {
          'message': {
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'role': 'assistant',
            'status': 'streaming',
            'createdAt': 1,
            'updatedAt': 1,
          },
        },
      ),
    );
    api.emitSession(
      StudioBridgeEvent(
        kindType: 'messagePartUpdated',
        sessionId: 'session-1',
        payload: {
          'part': {
            'partId': 'part-1',
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'partType': 'text',
            'order': 0,
            'revision': 0,
            'status': 'streaming',
            'createdAt': 1,
            'updatedAt': 1,
            'textChannel': 'final',
            'text': '',
          },
        },
      ),
    );
    api.emitSession(
      StudioBridgeEvent(
        kindType: 'messagePartDelta',
        sessionId: 'session-1',
        payload: {
          'delta': {
            'sessionId': 'session-1',
            'messageId': 'turn-1:assistant',
            'partId': 'part-1',
            'revision': 1,
            'field': 'text',
            'delta': 'partial',
          },
        },
      ),
    );
    api.emitSession(
      StudioBridgeEvent(
        kindType: 'messagePartUpdated',
        sessionId: 'session-1',
        payload: {
          'part': {
            'partId': 'part-1',
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'partType': 'text',
            'order': 0,
            'revision': 1,
            'status': 'completed',
            'createdAt': 1,
            'updatedAt': 2,
            'textChannel': 'final',
            'text': 'authoritative',
          },
        },
      ),
    );
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.selectedMessages.single.parts.single.text, 'authoritative');
    expect(state.partOverlaysBySession['session-1'], isEmpty);
  });

  test('session list stream updates only the addressed project', () async {
    final now = DateTime.fromMillisecondsSinceEpoch(1000);
    final api = _FakeStudioApi(
      _emptyState().copyWith(
        sessions: [
          StudioSession(
            id: 'session-1',
            projectId: 'project-1',
            title: 'Session 1',
            mode: CompileMode.auto,
            updatedAt: now,
          ),
          StudioSession(
            id: 'session-2',
            projectId: 'project-2',
            title: 'Session 2',
            mode: CompileMode.plan,
            updatedAt: now,
          ),
        ],
      ),
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitGlobal(
      StudioBridgeEvent(
        kindType: 'sessionListChanged',
        payload: {'projectId': 'project-1', 'sessions': <Object?>[]},
      ),
    );
    await pumpEventQueue();

    final sessions = container
        .read(studioControllerProvider)
        .requireValue
        .sessions;
    expect(sessions.map((session) => session.id), ['session-2']);
  });

  test(
    'session runtime stream preserves agents and refreshes active capabilities',
    () async {
      final api = _FakeStudioApi(
        _emptyState().copyWith(
          runtime: const SessionRuntimeView(
            model: 'planner/old',
            contextTokens: 1,
            contextWindow: 100,
            totalTokens: 2,
            costLabel: '',
            activeSkills: ['old-skill'],
            activeMcpServers: ['old-mcp'],
            activeLspServers: ['old-lsp'],
            agentCount: 2,
          ),
        ),
      );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      api.emitSession(
        StudioBridgeEvent(
          kindType: 'sessionRuntimeChanged',
          payload: {
            'runtime': {
              'sessionId': 'session-1',
              'usage': {
                'model': 'planner/new',
                'latestContextTokens': 42,
                'contextWindow': 128000,
                'promptTokens': 21,
                'completionTokens': 21,
                'cachedPromptTokens': 0,
                'totalTokens': 42,
                'estimatedCosts': [
                  {'currency': 'CNY', 'amount': '0.1600'},
                ],
                'hasUnpricedUsage': false,
                'updatedAt': 2,
              },
              'activeSkills': ['new-skill'],
              'activeMcpServers': ['new-mcp'],
              'activeLspServers': ['new-lsp'],
              'updatedAt': 2,
            },
          },
        ),
      );
      await pumpEventQueue();

      final runtime = container
          .read(studioControllerProvider)
          .requireValue
          .runtime;
      expect(runtime.model, 'planner/new');
      expect(runtime.contextTokens, 42);
      expect(runtime.costLabel, 'CNY 0.16');
      expect(runtime.activeSkills, ['new-skill']);
      expect(runtime.activeMcpServers, ['new-mcp']);
      expect(runtime.activeLspServers, ['new-lsp']);
      expect(runtime.agentCount, 2);

      api.emitSession(
        StudioBridgeEvent(
          kindType: 'sessionRuntimeChanged',
          payload: {
            'runtime': {
              'sessionId': 'other-session',
              'usage': {
                'model': 'planner/other',
                'latestContextTokens': 7,
                'contextWindow': 128000,
                'promptTokens': 7,
                'completionTokens': 0,
                'cachedPromptTokens': 0,
                'totalTokens': 7,
                'estimatedCosts': <Object?>[],
                'hasUnpricedUsage': false,
                'updatedAt': 3,
              },
              'activeSkills': ['other-skill'],
              'activeMcpServers': ['other-mcp'],
              'activeLspServers': ['other-lsp'],
              'updatedAt': 3,
            },
          },
        ),
      );
      await pumpEventQueue();

      final unchangedRuntime = container
          .read(studioControllerProvider)
          .requireValue
          .runtime;
      expect(unchangedRuntime.model, 'planner/new');
      expect(unchangedRuntime.activeSkills, ['new-skill']);
    },
  );

  test('demo API emits prompt and assistant timeline events', () async {
    final api = DemoStudioApi();
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    container
        .read(studioControllerProvider.notifier)
        .updateComposer('demo hello');

    await container.read(studioControllerProvider.notifier).submitComposer();
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.turnPhase, TurnPhase.completed);
    expect(
      state.selectedMessages
          .where((message) => message.role == 'user')
          .last
          .parts
          .single
          .text,
      'demo hello',
    );
    expect(
      state.selectedMessages
          .where((message) => message.role == 'assistant')
          .last
          .parts
          .single
          .text,
      contains('Demo response for'),
    );
  });

  test('bootstrap loads selected session history', () async {
    final api = _FakeStudioApi(
      _twoProjectState(selectedProjectId: 'project-a'),
    );
    api.sessionStates['session-a'] = _sessionHistoryState(
      projectId: 'project-a',
      sessionId: 'session-a',
      text: 'restored history from session a',
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    final state = await container.read(studioControllerProvider.future);

    expect(api.loadedSessionIds, ['session-a']);
    expect(state.selectedSessionId, 'session-a');
    expect(
      state.selectedMessages.single.parts.single.text,
      'restored history from session a',
    );
  });

  test('project selection reloads selected session history', () async {
    final api = _FakeStudioApi(
      _twoProjectState(selectedProjectId: 'project-a'),
    );
    api.selectProjectStates['project-b'] = _twoProjectState(
      selectedProjectId: 'project-b',
    );
    api.sessionStates['session-b'] = _sessionHistoryState(
      projectId: 'project-b',
      sessionId: 'session-b',
      text: 'restored history from session b',
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);

    await container
        .read(studioControllerProvider.notifier)
        .selectProject('project-b');

    final state = container.read(studioControllerProvider).requireValue;
    expect(api.loadedSessionIds, ['session-a', 'session-b']);
    expect(state.selectedProjectId, 'project-b');
    expect(state.selectedSessionId, 'session-b');
    expect(
      state.selectedMessages.single.parts.single.text,
      'restored history from session b',
    );
  });

  test(
    'archive project switches project and reloads selected session history',
    () async {
      final api = _FakeStudioApi(
        _twoProjectState(selectedProjectId: 'project-a'),
      );
      api.archiveProjectStates['project-a'] = _twoProjectState(
        selectedProjectId: 'project-b',
        projects: const [
          StudioProject(id: 'project-b', name: 'Project B', path: 'b'),
        ],
      );
      api.sessionStates['session-b'] = _sessionHistoryState(
        projectId: 'project-b',
        sessionId: 'session-b',
        text: 'history after project close',
      );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);

      await container
          .read(studioControllerProvider.notifier)
          .archiveProject('project-a');

      final state = container.read(studioControllerProvider).requireValue;
      expect(api.archivedProjectId, 'project-a');
      expect(api.archiveSelectedProjectId, 'project-a');
      expect(api.loadedSessionIds, ['session-a', 'session-b']);
      expect(state.projects.map((project) => project.id), ['project-b']);
      expect(state.selectedProjectId, 'project-b');
      expect(
        state.selectedMessages.single.parts.single.text,
        'history after project close',
      );
    },
  );

  test('archive last project clears current selection', () async {
    final api = _FakeStudioApi(
      _twoProjectState(selectedProjectId: 'project-a'),
    );
    api.archiveProjectStates['project-a'] = _noProjectState();
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);

    await container
        .read(studioControllerProvider.notifier)
        .archiveProject('project-a');

    final state = container.read(studioControllerProvider).requireValue;
    expect(api.archivedProjectId, 'project-a');
    expect(state.projects, isEmpty);
    expect(state.sessions, isEmpty);
    expect(state.selectedProjectId, isNull);
    expect(state.selectedSessionId, isNull);
    expect(state.selectedMessages, isEmpty);
  });

  test(
    'agent markdown repair keeps incomplete streaming blocks provisional',
    () {
      expect(
        repairAgentMarkdownForDisplay('```dart\nvoid main() {}'),
        '```dart\nvoid main() {}',
      );
      expect(
        repairAgentMarkdownForDisplay('| Name | State |\n| ---'),
        '| Name | State |\n| ---',
      );
    },
  );

  test('agent markdown repair recovers agent heading and fence boundaries', () {
    expect(
      repairAgentMarkdownForDisplay(
        '###整体层级```\n'
        '└──<html>\n'
        'CSS组织```\n'
        'body { margin: 0; }',
      ),
      '### 整体层级\n'
      '```\n'
      '└──<html>\n'
      'CSS组织\n'
      '```\n'
      'body { margin: 0; }',
    );
    expect(
      repairAgentMarkdownForDisplay(
        '```text\n'
        'WttrResponse ├ weather: Vec<WeatherDay>```\n\n'
        '## 依赖选型\n\n'
        '| 依赖 | 用途 |\n'
        '| --- | --- |\n'
        '| serde | JSON |',
      ),
      '```text\n'
      'WttrResponse ├ weather: Vec<WeatherDay>\n'
      '```\n\n'
      '## 依赖选型\n\n'
      '| 依赖 | 用途 |\n'
      '| --- | --- |\n'
      '| serde | JSON |',
    );
  });

  test('timeline uses gpt markdown directly without renderer facade', () {
    final timelineSource = File(
      'lib/src/features/timeline/timeline_view.dart',
    ).readAsStringSync();
    final facadeFile = File(
      'lib/src/features/timeline/streaming_markdown.dart',
    );

    expect(timelineSource, contains("package:gpt_markdown/gpt_markdown.dart"));
    expect(timelineSource, isNot(contains("import 'streaming_markdown.dart'")));
    expect(timelineSource, isNot(contains('AgentMarkdown(')));
    expect(facadeFile.existsSync(), isFalse);
  });

  testWidgets('timeline renders streaming markdown blocks', (tester) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final messages = [
      TimelineMessage(
        id: 'message-1',
        sessionId: 'session-1',
        role: 'assistant',
        createdAt: now,
        parts: const [
          TimelinePart(
            id: 'text-1',
            messageId: 'message-1',
            type: TimelinePartType.text,
            text:
                '# Build result\n'
                '- **Compile** runtime\n'
                '- Render `timeline`\n\n'
                '| File | State |\n'
                '| --- | --- |\n'
                '| app.dart | ready |\n\n'
                '```dart\n'
                'void main() {\n'
                "  print('ok');\n"
                '}',
            status: 'streaming',
          ),
          TimelinePart(
            id: 'reasoning-1',
            messageId: 'message-1',
            type: TimelinePartType.reasoning,
            title: 'Reasoning',
            text: '> hidden raw reasoning\n\n- keep this out of timeline',
            collapsed: false,
          ),
          TimelinePart(
            id: 'plan-1',
            messageId: 'message-1',
            type: TimelinePartType.plan,
            title: 'Plan',
            text: '## Next steps\n1. Analyze\n2. Ship',
          ),
        ],
      ),
    ];

    await tester.pumpWidget(
      _localizedApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 820,
            child: TimelineView(sessionId: 'session-1', messages: messages),
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.pump();

    expect(find.textContaining('Build result'), findsOneWidget);
    expect(find.textContaining('Compile'), findsOneWidget);
    expect(find.textContaining('File'), findsOneWidget);
    expect(find.textContaining('app.dart'), findsOneWidget);
    expect(find.textContaining("print('ok')"), findsOneWidget);
    expect(find.text('Reasoning'), findsOneWidget);
    expect(find.textContaining('hidden raw reasoning'), findsNothing);
    expect(find.textContaining('Next steps'), findsOneWidget);
    expect(find.textContaining('Analyze'), findsOneWidget);
  });

  testWidgets('timeline renders markdown after inline code fence closure', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final messages = [
      TimelineMessage(
        id: 'message-inline-fence',
        sessionId: 'session-1',
        role: 'assistant',
        createdAt: now,
        parts: const [
          TimelinePart(
            id: 'plan-inline-fence',
            messageId: 'message-inline-fence',
            type: TimelinePartType.plan,
            title: 'Plan',
            text:
                '```text\n'
                'WttrResponse ├ weather: Vec<WeatherDay>```\n\n'
                '## 依赖选型\n\n'
                '| 依赖 | 用途 |\n'
                '| --- | --- |\n'
                '| serde | JSON |',
          ),
        ],
      ),
    ];

    await tester.pumpWidget(
      _localizedApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 820,
            child: TimelineView(sessionId: 'session-1', messages: messages),
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.pump();

    expect(find.textContaining('依赖选型'), findsOneWidget);
    expect(find.textContaining('serde'), findsOneWidget);
    expect(find.textContaining('JSON'), findsOneWidget);
    expect(find.textContaining('## 依赖选型'), findsNothing);
    expect(find.textContaining('| serde | JSON |'), findsNothing);
  });

  testWidgets('timeline renders agent markdown with tight CJK headings', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final now = DateTime.fromMillisecondsSinceEpoch(0);
    final messages = [
      TimelineMessage(
        id: 'message-agent-markdown',
        sessionId: 'session-1',
        role: 'assistant',
        createdAt: now,
        parts: const [
          TimelinePart(
            id: 'plan-agent-markdown',
            messageId: 'message-agent-markdown',
            type: TimelinePartType.plan,
            title: 'Plan',
            text:
                'glm-intro.html代码结构单文件 HTML（~850行），GLM产品介绍落地页。\n\n'
                '###整体层级```\n'
                '└──<html>\n'
                '├──<head>\n'
                '│ └──<style> → 全部 CSS\n'
                'CSS组织```\n'
                'hero { display: grid; }\n\n'
                '###实现计划\n'
                '- 拆分结构\n'
                '- 保持动效',
          ),
        ],
      ),
    ];

    await tester.pumpWidget(
      _localizedApp(
        home: Scaffold(
          body: SizedBox(
            width: 980,
            height: 820,
            child: TimelineView(sessionId: 'session-1', messages: messages),
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.pump();

    expect(find.textContaining('整体层级'), findsOneWidget);
    expect(find.textContaining('实现计划'), findsOneWidget);
    expect(find.textContaining('└──<html>'), findsOneWidget);
    expect(find.textContaining('###整体层级```'), findsNothing);
    expect(find.textContaining('CSS组织```'), findsNothing);
  });

  testWidgets('timeline follows appended messages from the bottom', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-scroll',
        messages: _scrollMessages('session-scroll', 18),
      ),
    );
    await tester.pumpAndSettle();
    expect(_timelineExtentAfter(tester), lessThanOrEqualTo(80));

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-scroll',
        messages: _scrollMessages('session-scroll', 19),
      ),
    );
    await tester.pumpAndSettle();

    expect(_timelineExtentAfter(tester), lessThanOrEqualTo(80));
    expect(find.textContaining('message 18'), findsOneWidget);
  });

  testWidgets('timeline does not steal scroll when user reads older messages', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-detached',
        messages: _scrollMessages('session-detached', 24),
      ),
    );
    await tester.pumpAndSettle();

    await tester.drag(find.byType(ListView), const Offset(0, 260));
    await tester.pumpAndSettle();
    final offsetBeforeAppend = _timelinePixels(tester);
    expect(_timelineExtentAfter(tester), greaterThan(80));

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-detached',
        messages: _scrollMessages('session-detached', 25),
      ),
    );
    await tester.pumpAndSettle();

    expect(_timelinePixels(tester), closeTo(offsetBeforeAppend, 1));
    expect(find.byTooltip('Jump to latest'), findsOneWidget);
  });

  testWidgets('jump to latest button restores bottom following', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-jump',
        messages: _scrollMessages('session-jump', 24),
      ),
    );
    await tester.pumpAndSettle();

    await tester.drag(find.byType(ListView), const Offset(0, 260));
    await tester.pumpAndSettle();
    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-jump',
        messages: _scrollMessages('session-jump', 25),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('Jump to latest'));
    await tester.pumpAndSettle();

    expect(_timelineExtentAfter(tester), lessThanOrEqualTo(80));
    expect(find.byTooltip('Jump to latest'), findsNothing);
  });

  testWidgets('timeline follows streaming content growth near the bottom', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-streaming',
        messages: _scrollMessages('session-streaming', 12),
      ),
    );
    await tester.pumpAndSettle();

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-streaming',
        messages: _scrollMessages('session-streaming', 12, expandedLast: true),
      ),
    );
    await tester.pumpAndSettle();

    expect(_timelineExtentAfter(tester), lessThanOrEqualTo(80));
  });

  testWidgets('timeline keeps scroll state isolated per session', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(980, 520);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-a',
        messages: _scrollMessages('session-a', 24),
      ),
    );
    await tester.pumpAndSettle();
    await tester.drag(find.byType(ListView), const Offset(0, 260));
    await tester.pumpAndSettle();
    final sessionAOffset = _timelinePixels(tester);
    expect(_timelineExtentAfter(tester), greaterThan(80));

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-b',
        messages: _scrollMessages('session-b', 20),
      ),
    );
    await tester.pumpAndSettle();
    expect(_timelineExtentAfter(tester), lessThanOrEqualTo(80));

    await tester.pumpWidget(
      _timelineHarness(
        sessionId: 'session-a',
        messages: _scrollMessages('session-a', 24),
      ),
    );
    await tester.pumpAndSettle();

    expect(_timelinePixels(tester), closeTo(sessionAOffset, 1));
    expect(find.byTooltip('Jump to latest'), findsOneWidget);
  });

  testWidgets('sidebar session actions call Studio API', (tester) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_emptyState());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('New session'));
    await tester.pump();
    expect(api.createSessionCount, 1);

    await tester.tap(find.byTooltip('Archive session'));
    await tester.pump();
    expect(api.archivedSessionId, 'session-1');
  });

  testWidgets('project close buttons respect current session busy state', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(
      _twoProjectState(
        selectedProjectId: 'project-a',
        turnPhase: TurnPhase.streaming,
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pump();
    await tester.pump();

    final closeProjectButtons = find.widgetWithIcon(IconButton, Icons.close);
    final closeButtons = tester
        .widgetList<IconButton>(closeProjectButtons)
        .toList();
    expect(closeButtons.length, 2);
    expect(closeButtons.first.onPressed, isNull);
    expect(closeButtons.last.onPressed, isNotNull);

    await tester.tap(closeProjectButtons.last);
    await tester.pump();
    await tester.pump();
    expect(api.archivedProjectId, 'project-b');
  });

  testWidgets('status bar exposes session mode and planner model controls', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(
      _stateWithPlannerModels().copyWith(
        runtime: const SessionRuntimeView(
          model: 'planner/local',
          contextTokens: 42,
          contextWindow: 100,
          totalTokens: 128,
          costLabel: 'CNY 0.16',
          activeSkills: ['flutter-ui'],
          activeMcpServers: ['dart'],
          activeLspServers: ['rust-analyzer'],
          agentCount: 2,
        ),
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byTooltip('Session mode'), findsOneWidget);
    expect(find.byTooltip('Planner model'), findsOneWidget);
    expect(find.byTooltip('Reasoning effort'), findsOneWidget);
    expect(find.bySemanticsLabel('Context'), findsOneWidget);
    expect(find.text('42/100'), findsNothing);
    expect(find.text('CNY 0.16'), findsOneWidget);
    expect(find.text('1 skill · 1 MCP · 1 LSP · 2 agents'), findsOneWidget);

    final gesture = await tester.createGesture(kind: PointerDeviceKind.mouse);
    addTearDown(gesture.removePointer);
    await gesture.addPointer();
    await gesture.moveTo(tester.getCenter(find.bySemanticsLabel('Context')));
    await tester.pumpAndSettle();
    expect(find.text('42 / 100'), findsOneWidget);
    expect(find.text('128'), findsOneWidget);
    expect(find.text('planner/local'), findsOneWidget);
    await gesture.moveTo(Offset.zero);
    await tester.pumpAndSettle();
    await gesture.removePointer();

    final capabilityCenter = tester.getCenter(
      find.text('1 skill · 1 MCP · 1 LSP · 2 agents'),
    );
    final capabilityRect = tester.getRect(
      find.text('1 skill · 1 MCP · 1 LSP · 2 agents'),
    );
    await tester.tapAt(Offset(capabilityRect.left + 8, capabilityCenter.dy));
    await tester.pumpAndSettle();
    expect(find.text('ACTIVE CAPABILITIES'), findsOneWidget);
    expect(find.textContaining('Skills · flutter-ui'), findsOneWidget);
    expect(find.textContaining('MCP · dart'), findsOneWidget);
    expect(find.textContaining('LSP · rust-analyzer'), findsOneWidget);
    expect(find.textContaining('Subagents · 2 agents'), findsOneWidget);
    await gesture.moveTo(Offset.zero);
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('Session mode'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Plan').last);
    await tester.pumpAndSettle();
    expect(api.sessionModeUpdate, CompileMode.plan);
    api.emitGlobal(
      StudioBridgeEvent(
        kindType: 'sessionListChanged',
        payload: {
          'projectId': 'project-1',
          'sessions': [
            {
              'id': 'session-1',
              'projectId': 'project-1',
              'title': 'Session',
              'mode': 'plan',
              'updatedAt': 1,
            },
          ],
        },
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Plan'), findsOneWidget);

    await tester.tap(find.byTooltip('Planner model'));
    await tester.pumpAndSettle();
    await tester.tap(find.textContaining('Reasoner').last);
    await tester.pumpAndSettle();
    expect(api.roleUpdate?.roleKey, 'planner');
    expect(api.roleUpdate?.providerId, 'deepseek');
    expect(api.roleUpdate?.model, 'deepseek-reasoner');

    await tester.tap(find.byTooltip('Reasoning effort'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('max').last);
    await tester.pumpAndSettle();
    expect(api.roleUpdate?.roleKey, 'planner');
    expect(api.roleUpdate?.providerId, 'deepseek');
    expect(api.roleUpdate?.model, 'deepseek-reasoner');
    expect(api.roleUpdate?.effort, 'max');
  });

  testWidgets('select menus open upward and stay clear of their triggers', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await _expectMenuOpensAboveTrigger(
      tester: tester,
      triggerTooltip: 'Session mode',
      menuText: 'Plan',
    );
    await _expectMenuOpensAboveTrigger(
      tester: tester,
      triggerTooltip: 'Planner model',
      menuText: 'DeepSeek / DeepSeek Reasoner',
    );
    await _expectMenuOpensAboveTrigger(
      tester: tester,
      triggerTooltip: 'Reasoning effort',
      menuText: 'max',
    );
    await _expectMenuOpensAboveTrigger(
      tester: tester,
      triggerTooltip: 'Permission mode',
      menuText: 'Full',
    );
  });

  testWidgets('provider settings can add provider through typed save', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Available balance'), findsOneWidget);

    await tester.tap(find.widgetWithText(FilledButton, 'Add provider'));
    await tester.pumpAndSettle();
    expect(find.text('New provider'), findsOneWidget);
    expect(find.text('Search providers'), findsNothing);

    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pumpAndSettle();

    final settings = api.savedProviderSettings;
    expect(settings, isNotNull);
    final providers = settings!['providers'] as List<Object?>;
    expect(providers.length, 2);
    expect((providers.last! as Map<String, Object?>)['id'], 'deepseek-2');
    expect(settings['defaultProviderId'], 'deepseek-2');
  });

  testWidgets('provider editor cancel does not save local draft', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.widgetWithText(FilledButton, 'Add provider'));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.widgetWithText(TextFormField, 'Display name'),
      'Changed Provider',
    );
    await tester.pumpAndSettle();
    expect(api.savedProviderSettings, isNull);

    await tester.tap(find.widgetWithText(OutlinedButton, 'Cancel'));
    await tester.pumpAndSettle();

    expect(api.savedProviderSettings, isNull);
    expect(find.text('Search providers'), findsOneWidget);
  });

  testWidgets('provider search empty state explains active filter', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.enterText(find.byType(TextField).first, 'no-such-provider');
    await tester.pumpAndSettle();

    expect(find.text('No providers match this filter'), findsOneWidget);
    expect(
      find.text('Add a provider to configure credentials and models.'),
      findsNothing,
    );
  });

  testWidgets('editing non-default provider keeps current default provider', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(
      _stateWithPlannerModels().copyWith(
        defaultProviderId: 'deepseek',
        providers: [
          ..._stateWithPlannerModels().providers,
          const ProviderSettingsView(
            id: 'openai',
            templateKind: 'openai',
            name: 'OpenAI',
            subtitle: 'OpenAI Platform',
            baseUrl: 'https://api.openai.com/v1',
            defaultModel: 'gpt-5.5',
            models: [
              ProviderModelView(
                slug: 'gpt-5.5',
                displayName: 'GPT-5.5',
                reasoningEfforts: ['medium'],
              ),
            ],
            status: 'ready',
            usageLabel: '1 models',
            modelCount: '1',
            providerKind: 'open_ai',
          ),
        ],
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('Edit provider').last);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pumpAndSettle();

    expect(api.savedProviderSettings?['defaultProviderId'], 'deepseek');
  });

  testWidgets(
    'settings ordinary controls save immediately without draft buttons',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final api = _FakeStudioApi(
        _stateWithPlannerModels().copyWith(
          runtime: _stateWithPlannerModels().runtime.copyWith(
            activeSkills: ['flutter-ui-polish'],
          ),
          skills: const SkillsSettingsView(disabled: []),
          mcpServers: const [
            McpServerSettingsView(
              id: 'local',
              transport: 'stdio',
              endpoint: 'npx',
              enabled: true,
              status: 'enabled',
            ),
          ],
        ),
      );
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const SettingsPage()),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text('Save draft'), findsNothing);

      await tester.tap(find.text('Roles'));
      await tester.pumpAndSettle();
      await tester.tap(find.byType(DropdownButtonFormField<String>).first);
      await tester.pumpAndSettle();
      await tester.tap(find.textContaining('DeepSeek Reasoner').last);
      await tester.pumpAndSettle();
      expect(api.roleUpdate?.roleKey, 'explorer');

      await tester.tap(find.text('Skills'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('flutter-ui-polish'));
      await tester.pumpAndSettle();
      expect(
        api.savedSkillsSettings?['disabled'],
        contains('flutter-ui-polish'),
      );

      await tester.tap(find.text('MCP'));
      await tester.pumpAndSettle();
      await tester.tap(find.byType(Switch).first);
      await tester.pumpAndSettle();
      final servers = api.savedMcpSettings?['servers'] as List<Object?>?;
      expect((servers?.single as Map<String, Object?>?)?['enabled'], isFalse);

      await tester.tap(find.text('General'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Compact timeline'));
      await tester.pumpAndSettle();
      expect(api.savedGeneralSettings?['compactTimeline'], isTrue);

      await tester.tap(find.text('Security'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Full'));
      await tester.pumpAndSettle();
      expect(api.savedPermissionMode, PermissionMode.fullAccess);
    },
  );

  testWidgets('instructions text saves after debounce', (tester) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('Instructions'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).first, 'new base');
    await tester.pump(const Duration(milliseconds: 500));
    expect(api.savedInstructionsSettings, isNull);
    await tester.pump(const Duration(milliseconds: 200));

    expect(api.savedInstructionsSettings?['baseOverride'], 'new base');
  });

  testWidgets(
    'zh Hans locale localizes shell while config names pass through',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 900);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      final api = _FakeStudioApi(_stateWithPlannerModels());
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(
            locale: const Locale.fromSubtags(
              languageCode: 'zh',
              scriptCode: 'Hans',
            ),
            home: const SettingsPage(),
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text('服务'), findsWidgets);
      expect(find.text('添加 provider'), findsOneWidget);
      expect(find.text('DeepSeek'), findsOneWidget);
      expect(find.text('deepseek-reasoner'), findsWidgets);

      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(
            locale: const Locale.fromSubtags(
              languageCode: 'zh',
              scriptCode: 'Hans',
            ),
            home: const StudioShell(),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('描述你的需求...'), findsOneWidget);
      expect(find.text('deepseek-v4-flash'), findsOneWidget);
      expect(find.text('high'), findsOneWidget);
    },
  );

  testWidgets('user input interaction accepts freeform fallback answers', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _emptyState().copyWith(
      pendingInteractions: const [
        PendingInteraction(
          id: 'interaction-1',
          sessionId: 'session-1',
          kind: InteractionKind.userInput,
          title: 'Need input',
          body: 'Tell me which branch to use',
        ),
      ],
      turnPhase: TurnPhase.waitingForInteraction,
    );
    final api = _FakeStudioApi(state);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pump(const Duration(milliseconds: 50));

    expect(find.text('Tell me which branch to use'), findsWidgets);
    expect(find.widgetWithText(FilledButton, 'Answer'), findsOneWidget);
    await tester.enterText(find.byType(TextField).last, 'use main');
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.widgetWithText(FilledButton, 'Answer'));
    await tester.pump(const Duration(milliseconds: 50));

    expect(api.resolvedInteractionId, 'interaction-1');
    expect(api.resolvedInteraction?['type'], 'userInput');
    expect(api.resolvedInteraction?['answers'], {
      'answer': {
        'answers': ['use main'],
      },
    });
  });

  testWidgets('user input interaction submits paged multi-question answers', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _emptyState().copyWith(
      pendingInteractions: const [
        PendingInteraction(
          id: 'interaction-questions',
          sessionId: 'session-1',
          kind: InteractionKind.userInput,
          title: 'Need input',
          body: 'Choose implementation details',
          payload: {
            'questions': [
              {
                'id': 'scope',
                'header': 'Scope',
                'question': 'Pick the areas to update',
                'options': [
                  {'label': 'UI', 'description': 'Polish the dock'},
                  {'label': 'Tests', 'description': 'Add widget coverage'},
                ],
              },
              {
                'id': 'notes',
                'header': 'Notes',
                'question': 'Anything else?',
                'isOther': true,
                'options': [
                  {'label': 'Docs', 'description': 'Update design notes'},
                ],
              },
              {
                'id': 'secret',
                'header': 'Secret',
                'question': 'Secret value?',
                'isSecret': true,
              },
            ],
          },
        ),
      ],
      turnPhase: TurnPhase.waitingForInteraction,
    );
    final api = _FakeStudioApi(state);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pump(const Duration(milliseconds: 50));

    expect(find.text('A few questions'), findsOneWidget);
    expect(find.text('Question 1 / 3'), findsOneWidget);
    await tester.tap(find.text('UI'));
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.text('Tests'));
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.widgetWithText(FilledButton, 'Next'));
    await tester.pump(const Duration(milliseconds: 50));

    expect(find.text('Question 2 / 3'), findsOneWidget);
    await tester.tap(find.text('Docs'));
    await tester.pump(const Duration(milliseconds: 50));
    await tester.enterText(find.byType(TextField).last, 'also mention risk');
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.widgetWithText(FilledButton, 'Next'));
    await tester.pump(const Duration(milliseconds: 50));

    expect(find.text('Question 3 / 3'), findsOneWidget);
    await tester.enterText(find.byType(TextField).last, 'secret-value');
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.widgetWithText(FilledButton, 'Submit answers'));
    await tester.pump(const Duration(milliseconds: 50));

    expect(api.resolvedInteractionId, 'interaction-questions');
    expect(api.resolvedInteraction?['type'], 'userInput');
    expect(api.resolvedInteraction?['answers'], {
      'scope': {
        'answers': ['UI', 'Tests'],
      },
      'notes': {
        'answers': ['Docs', 'also mention risk'],
      },
      'secret': {
        'answers': ['secret-value'],
      },
    });
  });

  testWidgets('plan confirmation implement keeps plan content in timeline', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _emptyState().copyWith(
      sessions: [
        StudioSession(
          id: 'session-1',
          projectId: 'project-1',
          title: 'Session',
          mode: CompileMode.plan,
          updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
        ),
      ],
      pendingInteractions: const [
        PendingInteraction(
          id: 'interaction-plan',
          sessionId: 'session-1',
          kind: InteractionKind.planConfirmation,
          title: 'Confirm plan',
          body: '## Plan\n- Implement',
          payload: {
            'type': 'planConfirmation',
            'content': '## Plan\n- Implement',
          },
        ),
      ],
      turnPhase: TurnPhase.completed,
    );
    final api = _FakeStudioApi(state);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pump(const Duration(milliseconds: 50));

    expect(find.text('Implement this plan?'), findsOneWidget);
    expect(find.text('Implement this plan'), findsOneWidget);
    expect(find.text('Plan content'), findsNothing);
    expect(find.text('Plan'), findsOneWidget);
    await tester.tap(find.widgetWithText(FilledButton, 'Implement this plan'));
    await tester.pump(const Duration(milliseconds: 50));

    expect(api.resolvedInteractionId, 'interaction-plan');
    expect(api.resolvedInteraction?['type'], 'planConfirmation');
    expect(api.resolvedInteraction?['decision'], 'implementFreshContext');
    expect(api.resolvedInteraction?.containsKey('content'), isFalse);
    expect(find.text('Auto'), findsOneWidget);
    expect(find.text('Plan'), findsNothing);
  });

  testWidgets('plan confirmation adjustment submits only user instruction', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _emptyState().copyWith(
      pendingInteractions: const [
        PendingInteraction(
          id: 'interaction-plan-adjust',
          sessionId: 'session-1',
          kind: InteractionKind.planConfirmation,
          title: 'Confirm plan',
          body: '## Plan\n- Implement',
          payload: {
            'type': 'planConfirmation',
            'content': '## Plan\n- Implement',
          },
        ),
      ],
      turnPhase: TurnPhase.completed,
    );
    final api = _FakeStudioApi(state);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pump(const Duration(milliseconds: 50));

    await tester.tap(find.text('Tell Pure how to adjust'));
    await tester.pump(const Duration(milliseconds: 50));
    await tester.enterText(find.byType(TextField).last, 'add tests first');
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.widgetWithText(FilledButton, 'Submit adjustment'));
    await tester.pump(const Duration(milliseconds: 50));

    expect(api.resolvedInteractionId, 'interaction-plan-adjust');
    expect(api.resolvedInteraction, {
      'type': 'planConfirmation',
      'decision': 'continuePlanning',
      'content': 'add tests first',
      'reason': 'continue planning',
    });
  });

  testWidgets('skills discover loads project skill catalog', (tester) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(_stateWithPlannerModels());
    api.discoveredSkills = const ['flutter-ui-polish', 'runtime-review'];
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('Skills'));
    await tester.pumpAndSettle();
    expect(find.text('flutter-ui-polish'), findsNothing);

    await tester.tap(find.widgetWithText(FilledButton, 'Discover'));
    await tester.pumpAndSettle();

    expect(api.discoverProjectId, 'project-1');
    expect(find.text('flutter-ui-polish'), findsOneWidget);
    expect(find.text('runtime-review'), findsOneWidget);
  });
}

Widget _timelineHarness({
  required String sessionId,
  required List<TimelineMessage> messages,
}) {
  return _localizedApp(
    home: Scaffold(
      body: SizedBox(
        width: 980,
        height: 520,
        child: TimelineView(sessionId: sessionId, messages: messages),
      ),
    ),
  );
}

Widget _localizedApp({
  required Widget home,
  Locale locale = const Locale('en'),
}) {
  return MaterialApp(
    locale: locale,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: home,
  );
}

Future<void> _expectMenuOpensAboveTrigger({
  required WidgetTester tester,
  required String triggerTooltip,
  required String menuText,
}) async {
  final trigger = find.byTooltip(triggerTooltip);
  expect(trigger, findsOneWidget);
  final triggerRect = tester.getRect(trigger);

  await tester.tap(trigger);
  await tester.pumpAndSettle();

  final menuItem = find.text(menuText).last;
  expect(menuItem, findsOneWidget);
  final menuItemRect = tester.getRect(menuItem);
  expect(menuItemRect.bottom, lessThanOrEqualTo(triggerRect.top - 4));

  await tester.tapAt(const Offset(4, 4));
  await tester.pumpAndSettle();
}

List<TimelineMessage> _scrollMessages(
  String sessionId,
  int count, {
  bool expandedLast = false,
}) {
  final now = DateTime.fromMillisecondsSinceEpoch(0);
  return [
    for (var index = 0; index < count; index++)
      TimelineMessage(
        id: '$sessionId-message-$index',
        sessionId: sessionId,
        role: index.isEven ? 'assistant' : 'user',
        createdAt: now.add(Duration(seconds: index)),
        parts: [
          TimelinePart(
            id: '$sessionId-part-$index',
            messageId: '$sessionId-message-$index',
            type: TimelinePartType.text,
            status: index == count - 1 && expandedLast
                ? 'streaming'
                : 'completed',
            text:
                'message $index for $sessionId\n\n'
                '${expandedLast && index == count - 1 ? _streamingGrowthText : _singleBlockText}',
          ),
        ],
      ),
  ];
}

const _singleBlockText =
    'This timeline row has enough text to create a '
    'stable scroll extent without depending on exact font metrics.';

const _streamingGrowthText = '''
streaming line 1
streaming line 2
streaming line 3
streaming line 4
streaming line 5
streaming line 6
streaming line 7
streaming line 8
streaming line 9
streaming line 10
streaming line 11
streaming line 12
streaming line 13
streaming line 14
''';

ScrollPosition _timelinePosition(WidgetTester tester) {
  final listView = tester.widget<ListView>(
    find.byKey(const ValueKey('timeline-scrollable')),
  );
  return listView.controller!.position;
}

double _timelineExtentAfter(WidgetTester tester) {
  return _timelinePosition(tester).extentAfter;
}

double _timelinePixels(WidgetTester tester) {
  return _timelinePosition(tester).pixels;
}

StudioState _emptyState() {
  const project = StudioProject(id: 'project-1', name: 'project', path: '.');
  final session = StudioSession(
    id: 'session-1',
    projectId: project.id,
    title: 'Session',
    mode: CompileMode.auto,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
  );
  return StudioState(
    projects: const [project],
    sessions: [session],
    messagesBySession: const {'session-1': []},
    providers: const [],
    roles: const [],
    mcpServers: const [],
    selectedProjectId: project.id,
    selectedSessionId: session.id,
    permissionMode: PermissionMode.requestApproval,
    turnPhase: TurnPhase.idle,
    runtime: const SessionRuntimeView(
      model: '',
      contextTokens: 0,
      contextWindow: 0,
      totalTokens: 0,
      costLabel: '',
      activeSkills: [],
      activeMcpServers: [],
      activeLspServers: [],
      agentCount: 0,
    ),
    pendingInteractions: const [],
  );
}

StudioState _noProjectState() {
  return const StudioState(
    projects: [],
    sessions: [],
    messagesBySession: {},
    providers: [],
    roles: [],
    mcpServers: [],
    selectedProjectId: null,
    selectedSessionId: null,
    permissionMode: PermissionMode.requestApproval,
    turnPhase: TurnPhase.idle,
    runtime: SessionRuntimeView(
      model: '',
      contextTokens: 0,
      contextWindow: 0,
      totalTokens: 0,
      costLabel: '',
      activeSkills: [],
      activeMcpServers: [],
      activeLspServers: [],
      agentCount: 0,
    ),
    pendingInteractions: [],
  );
}

StudioState _twoProjectState({
  required String selectedProjectId,
  List<StudioProject> projects = const [
    StudioProject(id: 'project-a', name: 'Project A', path: 'a'),
    StudioProject(id: 'project-b', name: 'Project B', path: 'b'),
  ],
  TurnPhase turnPhase = TurnPhase.idle,
}) {
  final sessions = [
    if (projects.any((project) => project.id == 'project-a'))
      StudioSession(
        id: 'session-a',
        projectId: 'project-a',
        title: 'Session A',
        mode: CompileMode.auto,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
      ),
    if (projects.any((project) => project.id == 'project-b'))
      StudioSession(
        id: 'session-b',
        projectId: 'project-b',
        title: 'Session B',
        mode: CompileMode.plan,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
      ),
  ];
  final selectedSessionId = selectedProjectId == 'project-b'
      ? 'session-b'
      : 'session-a';
  return _emptyState().copyWith(
    projects: projects,
    sessions: sessions,
    messagesBySession: {for (final session in sessions) session.id: const []},
    selectedProjectId: selectedProjectId,
    selectedSessionId: selectedSessionId,
    turnPhase: turnPhase,
  );
}

StudioState _sessionHistoryState({
  required String projectId,
  required String sessionId,
  required String text,
}) {
  final session = StudioSession(
    id: sessionId,
    projectId: projectId,
    title: 'Loaded $sessionId',
    mode: CompileMode.auto,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(1),
  );
  return _emptyState().copyWith(
    projects: [StudioProject(id: projectId, name: projectId, path: projectId)],
    sessions: [session],
    selectedProjectId: projectId,
    selectedSessionId: sessionId,
    messagesBySession: {
      sessionId: [
        TimelineMessage(
          id: '$sessionId-message-history',
          sessionId: sessionId,
          role: 'assistant',
          createdAt: DateTime.fromMillisecondsSinceEpoch(1),
          parts: [
            TimelinePart(
              id: '$sessionId-part-history',
              messageId: '$sessionId-message-history',
              type: TimelinePartType.text,
              text: text,
              status: 'completed',
            ),
          ],
        ),
      ],
    },
    eventCursorsBySession: {sessionId: 42},
  );
}

StudioState _stateWithPlannerModels() {
  final state = _emptyState();
  return state.copyWith(
    providers: const [
      ProviderSettingsView(
        id: 'deepseek',
        name: 'DeepSeek',
        baseUrl: 'https://api.deepseek.com',
        defaultModel: 'deepseek-v4-flash',
        models: [
          ProviderModelView(
            slug: 'deepseek-v4-flash',
            displayName: 'DeepSeek V4 Flash',
            reasoningEfforts: ['high', 'max'],
          ),
          ProviderModelView(
            slug: 'deepseek-reasoner',
            displayName: 'DeepSeek Reasoner',
            reasoningEfforts: ['high', 'max'],
          ),
        ],
        status: 'ready',
        usageLabel: '2 models',
      ),
    ],
    roles: const [
      RoleSettingsView(
        key: 'planner',
        providerId: 'deepseek',
        model: 'deepseek-v4-flash',
        effort: 'high',
      ),
    ],
    runtime: state.runtime.copyWith(model: 'deepseek-v4-flash'),
  );
}

class _FakeStudioApi implements StudioApi {
  _FakeStudioApi(this.initialState);

  final StudioState initialState;
  final _global = StreamController<Object>.broadcast();
  final _session = StreamController<Object>.broadcast();
  final Map<String, StudioState> sessionStates = {};
  final Map<String, StudioState> selectProjectStates = {};
  final Map<String, StudioState> archiveProjectStates = {};
  final List<String> loadedSessionIds = [];
  int createSessionCount = 0;
  String? archivedProjectId;
  String? archiveSelectedProjectId;
  String? archivedSessionId;
  CompileMode? sessionModeUpdate;
  _RoleUpdate? roleUpdate;
  Map<String, Object?>? savedProviderSettings;
  Map<String, Object?>? savedInstructionsSettings;
  Map<String, Object?>? savedSkillsSettings;
  Map<String, Object?>? savedMcpSettings;
  Map<String, Object?>? savedGeneralSettings;
  PermissionMode? savedPermissionMode;
  String? resolvedInteractionId;
  Map<String, Object?>? resolvedInteraction;
  String? discoverProjectId;
  List<String> discoveredSkills = const [];
  int loadProviderUsagesCount = 0;

  void emitGlobal(StudioBridgeEvent event) => _global.add(event);

  void emitSession(StudioBridgeEvent event) => _session.add(event);

  @override
  Future<StudioState> bootstrap() async => initialState;

  @override
  Future<StudioState> loadSessionState(String sessionId) async {
    loadedSessionIds.add(sessionId);
    return sessionStates[sessionId] ?? initialState;
  }

  @override
  Future<StudioState> openProject(String path) async => initialState;

  @override
  Future<StudioState> selectProject(String projectId) async =>
      selectProjectStates[projectId] ?? initialState;

  @override
  Future<StudioState> archiveProject(
    String projectId, {
    String? selectedProjectId,
  }) async {
    archivedProjectId = projectId;
    archiveSelectedProjectId = selectedProjectId;
    return archiveProjectStates[projectId] ?? initialState;
  }

  @override
  Future<StudioState> createSession(String projectId, {String? title}) async {
    createSessionCount += 1;
    return initialState;
  }

  @override
  Future<StudioState> archiveSession(
    String sessionId, {
    String? selectedSessionId,
  }) async {
    archivedSessionId = sessionId;
    return initialState;
  }

  @override
  Future<StudioState> setSessionMode(String sessionId, CompileMode mode) async {
    sessionModeUpdate = mode;
    return initialState.copyWith(
      sessions: [
        for (final session in initialState.sessions)
          session.id == sessionId ? session.copyWith(mode: mode) : session,
      ],
    );
  }

  @override
  Future<StudioState> setModelRole({
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
    String? selectedSessionId,
  }) async {
    roleUpdate = _RoleUpdate(
      roleKey: roleKey,
      providerId: providerId,
      model: model,
      effort: effort,
    );
    return initialState.copyWith(
      roles: [
        for (final role in initialState.roles)
          role.key == roleKey
              ? RoleSettingsView(
                  key: role.key,
                  providerId: providerId,
                  model: model,
                  effort: effort ?? role.effort,
                )
              : role,
      ],
    );
  }

  @override
  Future<List<StudioBridgeEvent>> loadStudioEvents(
    String sessionId, {
    int? afterSequence,
    int limit = 500,
  }) async => const [];

  @override
  Future<void> resolveInteraction(
    String interactionId,
    Map<String, Object?> resolution,
  ) async {
    jsonEncode(resolution);
    resolvedInteractionId = interactionId;
    resolvedInteraction = resolution;
  }

  @override
  Future<void> stopPrompt(String sessionId) async {}

  @override
  Stream<Object> subscribeGlobalEvents() => _global.stream;

  @override
  Stream<Object> subscribeSessionEvents(String sessionId) => _session.stream;

  @override
  Future<void> submitPrompt(
    String sessionId,
    String prompt,
    List<String> attachmentIds,
  ) async {}

  @override
  Future<void> saveRuntimePermissionMode(PermissionMode mode) async {
    savedPermissionMode = mode;
  }

  @override
  Future<List<String>> listDiscoveredSkills(String projectId) async {
    discoverProjectId = projectId;
    return discoveredSkills;
  }

  @override
  Future<StudioState> saveProviderSettings(
    Map<String, Object?> settings,
  ) async {
    jsonEncode(settings);
    savedProviderSettings = settings;
    return initialState.copyWith(
      defaultProviderId: settings['defaultProviderId'] as String?,
      providers: [
        for (final value in settings['providers'] as List<Object?>)
          _providerFromSettings(value),
      ],
    );
  }

  @override
  Future<StudioState> saveInstructionsSettings(
    Map<String, Object?> settings,
  ) async {
    jsonEncode(settings);
    savedInstructionsSettings = settings;
    return initialState.copyWith(
      instructions: InstructionsSettingsView(
        baseOverride: settings['baseOverride'] as String? ?? '',
        developer: settings['developer'] as String? ?? '',
        user: settings['user'] as String? ?? '',
        projectDocMaxBytes: settings['projectDocMaxBytes'] as int? ?? 65536,
        projectDocFallbackFilenames: [
          for (final value
              in settings['projectDocFallbackFilenames'] as List<Object?>? ??
                  const <Object?>[])
            value.toString(),
        ],
      ),
    );
  }

  @override
  Future<StudioState> saveSkillsSettings(Map<String, Object?> settings) async {
    jsonEncode(settings);
    savedSkillsSettings = settings;
    return initialState.copyWith(
      skills: initialState.skills.copyWith(
        disabled: [
          for (final value
              in settings['disabled'] as List<Object?>? ?? const <Object?>[])
            value.toString(),
        ],
      ),
    );
  }

  @override
  Future<StudioState> saveMcpSettings(Map<String, Object?> settings) async {
    jsonEncode(settings);
    savedMcpSettings = settings;
    return initialState;
  }

  @override
  Future<StudioState> saveGeneralSettings(Map<String, Object?> settings) async {
    jsonEncode(settings);
    savedGeneralSettings = settings;
    return initialState.copyWith(
      general: GeneralSettingsView(
        followSystemTheme: settings['followSystemTheme'] as bool? ?? true,
        followActiveTurn: settings['followActiveTurn'] as bool? ?? true,
        compactTimeline: settings['compactTimeline'] as bool? ?? false,
      ),
    );
  }

  @override
  Future<List<ProviderUsageView>> loadProviderUsages() async {
    loadProviderUsagesCount += 1;
    return const [
      ProviderUsageView(
        providerId: 'deepseek',
        updatedAt: 1,
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
    ];
  }

  @override
  Future<void> saveStudioSettingsDraft(
    String section,
    Map<String, Object?> draft,
  ) async {}
}

ProviderSettingsView _providerFromSettings(Object? value) {
  final json = value as Map<String, Object?>;
  final defaultModel = json['defaultModel'] as String? ?? '';
  return ProviderSettingsView(
    id: json['id'] as String? ?? '',
    templateKind: json['templateKind'] as String? ?? 'openai',
    name: json['name'] as String? ?? '',
    baseUrl: json['baseUrl'] as String? ?? '',
    bearerToken: '',
    hasBearerToken: (json['bearerToken'] as String? ?? '').isNotEmpty,
    defaultModel: defaultModel,
    models: [
      ProviderModelView(
        slug: defaultModel,
        displayName: defaultModel,
        reasoningEfforts: const ['high'],
      ),
    ],
    status: 'ready',
    usageLabel: '1 models',
  );
}

class _RoleUpdate {
  const _RoleUpdate({
    required this.roleKey,
    required this.providerId,
    required this.model,
    required this.effort,
  });

  final String roleKey;
  final String providerId;
  final String model;
  final String? effort;
}
