part of '../widget_test.dart';

void registerShellSettingsTests() {
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

    final updatedAt = DateTime.fromMillisecondsSinceEpoch(1);
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
        agentsBySession: {
          'session-1': {
            'agent-reviewer': StudioAgentView(
              id: 'agent-reviewer',
              sessionId: 'session-1',
              path: 'root/reviewer',
              role: 'reviewer',
              task: 'Audit timeline projection',
              status: 'running',
              summary: 'Checking status projection',
              updatedAt: updatedAt,
            ),
            'agent-worker': StudioAgentView(
              id: 'agent-worker',
              sessionId: 'session-1',
              path: 'root/worker',
              role: 'worker',
              task: 'Patch Flutter status bar',
              status: 'completed',
              depth: 1,
              updatedAt: updatedAt,
            ),
          },
        },
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
    expect(find.text('1 skill · 1 MCP · 1 LSP'), findsOneWidget);
    expect(find.text('2 agents · 1 running'), findsOneWidget);

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

    final agentsCenter = tester.getCenter(find.text('2 agents · 1 running'));
    final agentsRect = tester.getRect(find.text('2 agents · 1 running'));
    await tester.tapAt(Offset(agentsRect.left + 8, agentsCenter.dy));
    await tester.pumpAndSettle();
    expect(find.text('SUBAGENTS'), findsOneWidget);
    expect(find.text('reviewer'), findsOneWidget);
    expect(find.text('worker'), findsOneWidget);
    expect(find.text('Running'), findsOneWidget);
    await tester.tap(find.text('reviewer'));
    await tester.pumpAndSettle();
    expect(find.textContaining('Checking status projection'), findsOneWidget);
    await tester.tapAt(Offset.zero);
    await tester.pumpAndSettle();

    final capabilityCenter = tester.getCenter(
      find.text('1 skill · 1 MCP · 1 LSP'),
    );
    final capabilityRect = tester.getRect(find.text('1 skill · 1 MCP · 1 LSP'));
    await tester.tapAt(Offset(capabilityRect.left + 8, capabilityCenter.dy));
    await tester.pumpAndSettle();
    expect(find.text('ACTIVE CAPABILITIES'), findsOneWidget);
    expect(find.textContaining('Skills · flutter-ui'), findsOneWidget);
    expect(find.textContaining('MCP · dart'), findsOneWidget);
    expect(find.textContaining('LSP · rust-analyzer'), findsOneWidget);
    await tester.tapAt(Offset(capabilityRect.left + 8, capabilityCenter.dy));
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('Session mode'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Plan').last);
    await tester.pumpAndSettle();
    expect(api.sessionModeUpdate, CompileMode.plan);
    api.emitGlobal(
      _sessionListChangedEvent(
        projectId: 'project-1',
        sessions: [
          StudioSession(
            id: 'session-1',
            projectId: 'project-1',
            title: 'Session',
            mode: CompileMode.plan,
            updatedAt: DateTime.fromMillisecondsSinceEpoch(1000),
          ),
        ],
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

  testWidgets('zh Hans localizes session and permission mode labels', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final api = _FakeStudioApi(
      _stateWithPlannerModels().copyWith(
        sessions: [
          StudioSession(
            id: 'session-1',
            projectId: 'project-1',
            title: 'Session',
            mode: CompileMode.plan,
            updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
          ),
        ],
        permissionMode: PermissionMode.fullAccess,
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(
          locale: const Locale('zh', 'Hans'),
          home: const StudioShell(),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.textContaining('计划 · 更新于'), findsOneWidget);
    expect(find.text('计划'), findsWidgets);
    expect(find.text('完全'), findsOneWidget);
    expect(find.text('Plan'), findsNothing);
    expect(find.text('Full'), findsNothing);

    await tester.tap(find.byTooltip('会话模式'));
    await tester.pumpAndSettle();
    expect(find.text('自动'), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();

    await tester.tap(find.byTooltip('权限模式'));
    await tester.pumpAndSettle();
    expect(find.text('请求'), findsOneWidget);
    expect(find.text('审查'), findsOneWidget);
    expect(find.text('完全'), findsWidgets);
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

  test('provider settings save updates default provider in store', () async {
    final api = _FakeStudioApi(_stateWithPlannerModels());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await container
        .read(studioControllerProvider.notifier)
        .saveProviderSettings({
          'defaultProviderId': 'deepseek',
          'providers': [
            {
              'id': 'deepseek',
              'templateKind': 'deepseek',
              'name': 'DeepSeek',
              'baseUrl': 'https://api.deepseek.com',
              'defaultModel': 'deepseek-v4-flash',
              'models': [
                {'slug': 'deepseek-v4-flash'},
              ],
            },
          ],
        });

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.defaultProviderId, 'deepseek');
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
}
