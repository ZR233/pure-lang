part of '../widget_test.dart';

void registerProviderLifecycleTests() {
  test(
    'workspace family auto-disposes while Studio controller stays alive',
    () async {
      final api = _FakeStudioApi(
        _twoProjectState(selectedProjectId: 'project-a'),
      );
      final container = ProviderContainer.test(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );

      final controllerSubscription = container.listen(
        studioControllerProvider,
        (_, _) {},
      );
      await container.read(studioControllerProvider.future);

      final workspace = agentWorkspaceProvider('session-a');
      final workspaceSubscription = container.listen(workspace, (_, _) {});
      await container.pump();
      expect(container.exists(workspace), isTrue);

      workspaceSubscription.close();
      await container.pump();
      expect(container.exists(workspace), isFalse);

      controllerSubscription.close();
      await container.pump();
      expect(container.exists(studioControllerProvider), isTrue);
    },
  );

  test('fatal Studio bootstrap does not retry automatically', () async {
    final api = _FakeStudioApi(_emptyState())
      ..bootstrapError = StateError('database unavailable');
    final container = ProviderContainer.test(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    final subscription = container.listen(studioControllerProvider, (_, _) {});

    await expectLater(
      container.read(studioControllerProvider.future),
      throwsA(isA<StateError>()),
    );
    await Future<void>.delayed(const Duration(milliseconds: 300));
    await container.pump();

    expect(api.bootstrapCount, 1);
    subscription.close();
  });

  test('sidebar selector ignores workspace-only composer updates', () async {
    final api = _FakeStudioApi(
      _twoProjectState(selectedProjectId: 'project-a'),
    );
    final container = ProviderContainer.test(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    final controllerSubscription = container.listen(
      studioControllerProvider,
      (_, _) {},
    );
    await container.read(studioControllerProvider.future);

    var sidebarNotifications = 0;
    final sidebarSubscription = container.listen(
      sidebarProvider,
      (_, _) => sidebarNotifications += 1,
      fireImmediately: true,
    );
    await container.pump();
    expect(sidebarNotifications, 1);

    container
        .read(studioControllerProvider.notifier)
        .updateComposer('session-a', 'draft');
    await container.pump();

    expect(sidebarNotifications, 1);
    sidebarSubscription.close();
    controllerSubscription.close();
  });

  test('Item delta only notifies the selected timeline projection', () async {
    final initial = _emptyState();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer.test(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    final controllerSubscription = container.listen(
      studioControllerProvider,
      (_, _) {},
    );
    await container.read(studioControllerProvider.future);
    await container.pump();
    api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: initial.selectedWorkspace!.copyWith(revision: 1),
      ),
    );
    api.emitThreadFrame(
      _threadItemFrame(
        threadId: 'session-1',
        workspaceRevision: 2,
        item: _threadItemFixture(
          id: 'item-1',
          threadId: 'session-1',
          turnId: 'turn-1',
          ordinal: 0,
          status: 'streaming',
          text: '',
        ),
      ),
    );
    await pumpEventQueue();

    final notifications = <String, int>{
      'sidebar': 0,
      'header': 0,
      'settings': 0,
      'layout': 0,
      'status': 0,
      'timeline': 0,
    };
    final subscriptions = <ProviderSubscription<Object?>>[
      container.listen<AsyncValue<SidebarView>>(
        sidebarProvider,
        (_, _) => notifications['sidebar'] = notifications['sidebar']! + 1,
      ),
      container.listen<AsyncValue<HeaderView>>(
        studioHeaderProvider,
        (_, _) => notifications['header'] = notifications['header']! + 1,
      ),
      container.listen<AsyncValue<SettingsPageView>>(
        settingsPageProvider,
        (_, _) => notifications['settings'] = notifications['settings']! + 1,
      ),
      container.listen<AsyncValue<WorkspaceLayoutView?>>(
        selectedWorkspaceLayoutProvider,
        (_, _) => notifications['layout'] = notifications['layout']! + 1,
      ),
      container.listen<AsyncValue<StatusBarView?>>(
        statusBarProvider,
        (_, _) => notifications['status'] = notifications['status']! + 1,
      ),
      container.listen<AsyncValue<TimelinePaneView?>>(
        agentTimelineProvider('session-1'),
        (_, _) => notifications['timeline'] = notifications['timeline']! + 1,
      ),
    ];

    api.emitThreadFrame(
      _threadDeltaFrame(
        threadId: 'session-1',
        workspaceRevision: 3,
        itemId: 'item-1',
        itemRevision: 1,
        field: 'text',
        delta: 'partial',
      ),
    );
    await _pumpFrameBatch();

    expect(notifications, {
      'sidebar': 0,
      'header': 0,
      'settings': 0,
      'layout': 0,
      'status': 0,
      'timeline': 1,
    });

    for (final subscription in subscriptions) {
      subscription.close();
    }
    controllerSubscription.close();
  });
}
