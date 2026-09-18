part of '../widget_test.dart';

void registerProjectSidebarTests() {
  testWidgets(
    'add project wizard saves once, retries connection and adopts named remote project',
    (tester) async {
      _configureResponsiveView(tester, const Size(1440, 900));
      final api = _FakeStudioApi(_emptyState())
        ..testSshConnectionError = StateError('offline');
      api.selectProjectStates['remote-project'] = _remoteProjectAdoptedState(
        sshAlias: 'workstation',
      );
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.openProject));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('add-project-remote')));
      await tester.pump();
      await tester.tap(find.byKey(const ValueKey('add-project-continue')));
      await tester.pumpAndSettle();
      await tester.tap(
        find.byKey(const ValueKey('add-project-new-connection')),
      );
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(StudioDriverKeys.sshServerAliasInput),
        'workstation',
      );
      await tester.enterText(
        find.byKey(StudioDriverKeys.sshServerHostInput),
        'dev.test',
      );
      await tester.enterText(
        find.byKey(StudioDriverKeys.sshServerUsernameInput),
        'rui',
      );
      await tester.tap(find.byKey(StudioDriverKeys.sshServerSave));
      await tester.pumpAndSettle();
      expect(api.sshServers, hasLength(1));
      expect(find.textContaining('offline'), findsOneWidget);
      expect(
        tester
            .widget<TextField>(find.byKey(StudioDriverKeys.sshServerHostInput))
            .controller!
            .text,
        'dev.test',
      );
      api.testSshConnectionError = null;
      await tester.tap(find.byKey(StudioDriverKeys.sshServerSave));
      await tester.pumpAndSettle();
      expect(api.savedSshServer!.alias, isNotEmpty);
      expect(api.sshServers, hasLength(1));
      await tester.enterText(
        find.byKey(const ValueKey('add-project-name')),
        'Remote workspace',
      );
      await tester.pump();
      await tester.tap(find.byKey(StudioDriverKeys.sshOpenCurrentDirectory));
      await tester.pumpAndSettle();
      expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsNothing);
      final state = ProviderScope.containerOf(
        tester.element(find.byType(StudioShell)),
      ).read(studioControllerProvider).requireValue;
      expect(state.selectedProjectId, 'remote-project');
      expect(
        state.projects.firstWhere((p) => p.id == 'remote-project').name,
        'Remote workspace',
      );
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('project sidebar creates a draft in the clicked project', (
    tester,
  ) async {
    _configureResponsiveView(tester, const Size(1440, 900));
    final api = _FakeStudioApi(
      _twoProjectState(selectedProjectId: 'project-a'),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(
      find.byKey(const ValueKey('project-new-session-project-b')),
    );
    await tester.pumpAndSettle();
    final container = ProviderScope.containerOf(
      tester.element(find.byType(StudioShell)),
    );
    final state = container.read(studioControllerProvider).requireValue;
    expect(state.selectedProjectId, 'project-b');
    expect(state.selectedThreadId, isNull);
    expect(find.byKey(StudioDriverKeys.startPage), findsOneWidget);
    expect(tester.getSize(find.byKey(StudioDriverKeys.sidebar)).width, 336);
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'project sidebar narrow drawer preserves project and session names',
    (tester) async {
      _configureResponsiveView(tester, const Size(600, 800));
      final api = _FakeStudioApi(
        _twoProjectState(selectedProjectId: 'project-a'),
      );
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.byKey(StudioDriverKeys.sidebar).hitTestable(), findsNothing);
      await tester.tap(find.byKey(const ValueKey('sidebar-toggle')));
      await tester.pumpAndSettle();
      expect(find.text('Project A').hitTestable(), findsOneWidget);
      expect(
        find.byKey(StudioDriverKeys.threadRow('session-a')).hitTestable(),
        findsOneWidget,
      );
      await tester.tap(find.byKey(StudioDriverKeys.threadRow('session-a')));
      await tester.pumpAndSettle();
      expect(find.byKey(StudioDriverKeys.sidebar).hitTestable(), findsNothing);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'project sidebar search reaches a session outside the initial window',
    (tester) async {
      _configureResponsiveView(tester, const Size(1440, 900));
      final api = _FakeStudioApi(
        _twoProjectState(selectedProjectId: 'project-a'),
      );
      api.directoryPages['old'] = ThreadDirectoryPage(
        threads: [
          StudioThread(
            id: 'old-session',
            projectId: 'project-b',
            title: 'Find the ancient session',
            mode: ThreadModeId.simple,
            updatedAt: DateTime(2020),
          ),
          for (var index = 1; index < 25; index++)
            StudioThread(
              id: 'old-session-$index',
              projectId: 'project-b',
              title: 'Ancient continuation $index',
              mode: ThreadModeId.simple,
              updatedAt: DateTime(2020),
            ),
        ],
      );
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(const ValueKey('sidebar-search')),
        'ancient',
      );
      await tester.pump(const Duration(milliseconds: 250));
      await tester.pumpAndSettle();
      expect(find.text('Find the ancient session'), findsOneWidget);
      expect(
        find.byKey(StudioDriverKeys.projectRow('project-b')),
        findsOneWidget,
      );
      expect(find.byKey(StudioDriverKeys.threadRow('session-a')), findsNothing);
      final container = ProviderScope.containerOf(
        tester.element(find.byType(StudioShell)),
      );
      expect(
        container
            .read(studioControllerProvider)
            .requireValue
            .threads
            .any((thread) => thread.id == 'old-session'),
        isTrue,
      );
      final more = find.text('Show earlier sessions');
      await tester.ensureVisible(more);
      await tester.tap(more);
      await tester.pumpAndSettle();
      await tester.pump(const Duration(milliseconds: 300));
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.threadRow('old-session-24')),
        findsOneWidget,
      );
      await tester.ensureVisible(
        find.byKey(StudioDriverKeys.threadRow('old-session')),
      );
      await tester.tap(find.byKey(StudioDriverKeys.threadRow('old-session')));
      await tester.pump();
      await tester.pump();
      expect(
        find.byKey(const ValueKey('agent-workspace-loading')),
        findsOneWidget,
      );
      final selected = container.read(studioControllerProvider).requireValue;
      expect(selected.selectedThreadId, 'old-session');
      expect(selected.selectedProjectId, 'project-b');
    },
  );

  testWidgets(
    'add project wizard chooses an existing remote connection and keeps browse failures retryable',
    (tester) async {
      _configureResponsiveView(tester, const Size(1440, 900));
      final api = _FakeStudioApi(_emptyState())
        ..browseRemoteError = StateError('permission denied');
      api.sshServers = [
        const SshServer(
          alias: 'dev',
          hostName: 'dev.test',
          port: 22,
          username: 'rui',
          managed: true,
        ),
      ];
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.openProject));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('add-project-remote')));
      await tester.pump();
      await tester.tap(find.byKey(const ValueKey('add-project-continue')));
      await tester.pumpAndSettle();
      await tester.tap(
        find.byKey(const ValueKey('add-project-connection-dev')),
      );
      await tester.pump();
      await tester.tap(find.byKey(const ValueKey('add-project-continue')));
      await tester.pumpAndSettle();
      expect(api.testedSshServerAlias, 'dev');
      expect(find.textContaining('permission denied'), findsOneWidget);
      expect(api.openRemoteProjectCallCount, 0);
      api.browseRemoteError = null;
      await tester.enterText(
        find.byKey(StudioDriverKeys.sshDirectoryPathInput),
        '/workspace',
      );
      await tester.tap(find.byKey(StudioDriverKeys.sshDirectoryGo));
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.sshDirectoryEntry('/workspace/project')),
        findsOneWidget,
      );
      expect(tester.takeException(), isNull);
    },
  );
}
