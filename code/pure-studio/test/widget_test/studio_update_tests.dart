part of '../widget_test.dart';

void registerStudioUpdateTests() {
  test('disabled builds never call the update service', () async {
    final api = _FakeStudioUpdateApi(_upToDateSnapshot());
    final container = ProviderContainer(
      overrides: [
        studioUpdateApiProvider.overrideWithValue(api),
        studioUpdateEnabledProvider.overrideWithValue(false),
        studioVersionProvider.overrideWithValue('1.0.0'),
      ],
    );
    addTearDown(container.dispose);

    final state = container.read(studioUpdateControllerProvider);
    await Future<void>.delayed(Duration.zero);

    expect(state.phase, StudioUpdatePhase.disabled);
    expect(api.checkCount, 0);
  });

  test('startup reads last-known state without checking the network', () async {
    final availableApi = _FakeStudioUpdateApi(_availableUpdateSnapshot());
    final available = _updateContainer(availableApi);
    addTearDown(available.dispose);
    available.read(studioUpdateControllerProvider);
    await _flushUpdateController();

    expect(
      available.read(studioUpdateControllerProvider).phase,
      StudioUpdatePhase.available,
    );
    expect(
      available.read(studioUpdateControllerProvider).update?.version,
      '1.1.0',
    );
    expect(availableApi.checkCount, 0);

    final currentApi = _FakeStudioUpdateApi(_upToDateSnapshot());
    final current = _updateContainer(currentApi);
    addTearDown(current.dispose);
    current.read(studioUpdateControllerProvider);
    await _flushUpdateController();

    expect(
      current.read(studioUpdateControllerProvider).phase,
      StudioUpdatePhase.idle,
    );
    expect(currentApi.checkCount, 0);
  });

  test('check errors are retryable', () async {
    final api = _FakeStudioUpdateApi(
      _upToDateSnapshot(),
      checkError: StateError('offline'),
    );
    final container = _updateContainer(api);
    addTearDown(container.dispose);
    container.read(studioUpdateControllerProvider);
    await container.read(studioUpdateControllerProvider.notifier).check();

    final failed = container.read(studioUpdateControllerProvider);
    expect(failed.phase, StudioUpdatePhase.failed);
    expect(failed.errorCode, 'checkFailed');

    api.checkError = null;
    await container.read(studioUpdateControllerProvider.notifier).check();
    expect(
      container.read(studioUpdateControllerProvider).phase,
      StudioUpdatePhase.upToDate,
    );
  });

  test('install maps download, verify, and installer events', () async {
    final api = _FakeStudioUpdateApi(_availableUpdateSnapshot());
    final container = _updateContainer(api);
    addTearDown(container.dispose);
    container.read(studioUpdateControllerProvider);
    await _flushUpdateController();

    final install = container
        .read(studioUpdateControllerProvider.notifier)
        .install();
    await api.installStarted.future;
    api.emit(
      const StudioUpdateInstallEvent(
        kind: StudioUpdateInstallEventKind.started,
        total: 100,
      ),
    );
    api.emit(
      const StudioUpdateInstallEvent(
        kind: StudioUpdateInstallEventKind.progress,
        downloaded: 25,
        total: 100,
      ),
    );
    await _flushUpdateController();
    expect(container.read(studioUpdateControllerProvider).progress, 0.25);

    api.emit(
      const StudioUpdateInstallEvent(
        kind: StudioUpdateInstallEventKind.verifying,
      ),
    );
    await _flushUpdateController();
    expect(
      container.read(studioUpdateControllerProvider).phase,
      StudioUpdatePhase.verifying,
    );

    api.emit(
      const StudioUpdateInstallEvent(
        kind: StudioUpdateInstallEventKind.installerLaunched,
      ),
    );
    await api.closeInstallEvents();
    await install;
    expect(
      container.read(studioUpdateControllerProvider).phase,
      StudioUpdatePhase.installerLaunched,
    );
  });

  test('busy guard refuses to enter the install stream', () async {
    final api = _FakeStudioUpdateApi(_availableUpdateSnapshot());
    final container = _updateContainer(api, runtimeBusy: true);
    addTearDown(container.dispose);
    container.read(studioUpdateControllerProvider);
    await _flushUpdateController();

    await container.read(studioUpdateControllerProvider.notifier).install();

    final state = container.read(studioUpdateControllerProvider);
    expect(state.phase, StudioUpdatePhase.failed);
    expect(state.errorCode, 'runtimeBusy');
    expect(api.installCount, 0);
  });

  test(
    'active update operation can be cancelled before installer launch',
    () async {
      final api = _FakeStudioUpdateApi(_availableUpdateSnapshot());
      final container = _updateContainer(api);
      addTearDown(container.dispose);
      container.read(studioUpdateControllerProvider);
      await _flushUpdateController();

      final install = container
          .read(studioUpdateControllerProvider.notifier)
          .install();
      await api.installStarted.future;
      await _flushUpdateController();
      await container
          .read(studioUpdateControllerProvider.notifier)
          .cancelInstall();

      expect(api.operation.cancelled, isTrue);
      expect(
        container.read(studioUpdateControllerProvider).errorCode,
        'cancelled',
      );
      await api.closeInstallEvents();
      await install;
    },
  );

  testWidgets('General shows update actions and disables install while busy', (
    tester,
  ) async {
    _configureSettingsTestView(tester);
    final updateApi = _FakeStudioUpdateApi(_availableUpdateSnapshot());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(
            _FakeStudioApi(
              _withSelectedTurn(
                _stateWithPlannerModels().copyWith(
                  updaterState: _availableUpdateSnapshot(),
                ),
                _testTurn(
                  threadId: 'session-1',
                  state: const StudioTurnState.inProgress(
                    StudioTurnActivity.responding,
                  ),
                ),
              ),
            ),
          ),
          studioUpdateApiProvider.overrideWithValue(updateApi),
          studioUpdateEnabledProvider.overrideWithValue(true),
          studioVersionProvider.overrideWithValue('1.0.0'),
          studioRuntimeBusyProvider.overrideWithValue(true),
        ],
        child: _localizedApp(home: const SettingsPage()),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('General'));
    await tester.pumpAndSettle();

    expect(find.text('Pure Studio update'), findsOneWidget);
    expect(find.textContaining('Version 1.1.0 is available'), findsOneWidget);
    expect(
      find.byKey(const ValueKey('studio-update-release-notes')),
      findsOneWidget,
    );
    final install = tester.widget<FilledButton>(
      find.byKey(const ValueKey('studio-update-install')),
    );
    expect(install.onPressed, isNull);
    expect(
      find.text('Finish the active turn or task before installing.'),
      findsOneWidget,
    );
  });

  testWidgets('sidebar settings button shows a quiet update indicator', (
    tester,
  ) async {
    final updateApi = _FakeStudioUpdateApi(_availableUpdateSnapshot());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(
            _FakeStudioApi(
              _stateWithPlannerModels().copyWith(
                updaterState: _availableUpdateSnapshot(),
              ),
            ),
          ),
          studioUpdateApiProvider.overrideWithValue(updateApi),
          studioUpdateEnabledProvider.overrideWithValue(true),
          studioVersionProvider.overrideWithValue('1.0.0'),
        ],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(
      find.byKey(const ValueKey('studio-update-indicator')),
      findsOneWidget,
    );
  });

  testWidgets('update row has zh Hans copy', (tester) async {
    _configureSettingsTestView(tester);
    final updateApi = _FakeStudioUpdateApi(_availableUpdateSnapshot());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(
            _FakeStudioApi(
              _stateWithPlannerModels().copyWith(
                updaterState: _availableUpdateSnapshot(),
              ),
            ),
          ),
          studioUpdateApiProvider.overrideWithValue(updateApi),
          studioUpdateEnabledProvider.overrideWithValue(true),
          studioVersionProvider.overrideWithValue('1.0.0'),
        ],
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
    await tester.tap(find.text('通用'));
    await tester.pumpAndSettle();

    expect(find.text('Pure Studio 更新'), findsOneWidget);
    expect(find.text('下载并安装'), findsOneWidget);
    expect(find.text('发行说明'), findsOneWidget);
  });
}

ProviderContainer _updateContainer(
  _FakeStudioUpdateApi api, {
  bool runtimeBusy = false,
}) {
  return ProviderContainer(
    overrides: [
      studioApiProvider.overrideWithValue(
        _FakeStudioApi(_emptyState().copyWith(updaterState: api.checkResult)),
      ),
      studioUpdateApiProvider.overrideWithValue(api),
      studioUpdateEnabledProvider.overrideWithValue(true),
      studioVersionProvider.overrideWithValue('1.0.0'),
      studioRuntimeBusyProvider.overrideWithValue(runtimeBusy),
    ],
  );
}

Future<void> _flushUpdateController() async {
  await Future<void>.delayed(Duration.zero);
  await Future<void>.delayed(Duration.zero);
}

class _FakeStudioUpdateApi implements StudioUpdateApi {
  _FakeStudioUpdateApi(this.checkResult, {this.checkError});

  UpdaterStateSnapshot checkResult;
  Object? checkError;
  final installStarted = Completer<void>();
  final _installEvents = StreamController<StudioUpdateInstallEvent>();
  int checkCount = 0;
  int installCount = 0;
  String? openedNotesUrl;
  late _FakeStudioUpdateOperation operation;

  @override
  Future<UpdaterStateSnapshot> check() async {
    checkCount += 1;
    if (checkError case final error?) throw error;
    return checkResult;
  }

  @override
  Future<StudioUpdateOperation> startInstall({
    required int expectedRevision,
    required String version,
  }) async {
    installCount += 1;
    if (!installStarted.isCompleted) installStarted.complete();
    return operation = _FakeStudioUpdateOperation(_installEvents);
  }

  @override
  Future<void> openReleaseNotes(String url) async {
    openedNotesUrl = url;
  }

  void emit(StudioUpdateInstallEvent event) => _installEvents.add(event);

  Future<void> closeInstallEvents() => _installEvents.close();
}

class _FakeStudioUpdateOperation implements StudioUpdateOperation {
  _FakeStudioUpdateOperation(this._events);

  final StreamController<StudioUpdateInstallEvent> _events;
  bool cancelled = false;

  @override
  Stream<StudioUpdateInstallEvent> get events => _events.stream;

  @override
  Future<void> cancel() async {
    cancelled = true;
  }

  @override
  void dispose() {}
}

UpdaterStateSnapshot _availableUpdateSnapshot() => UpdaterStateSnapshot(
  meta: ObservedStateMeta(
    revision: 1,
    phase: ObservedStatePhase.ready,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(1000),
    lastCheckedAt: DateTime.fromMillisecondsSinceEpoch(1000),
    stale: false,
  ),
  version: '1.1.0',
  publishedAt: DateTime.fromMillisecondsSinceEpoch(1000),
  notesUrl: 'https://github.com/ZR233/pure-lang/releases/tag/v1.1.0',
);

UpdaterStateSnapshot _upToDateSnapshot() => UpdaterStateSnapshot(
  meta: ObservedStateMeta(
    revision: 1,
    phase: ObservedStatePhase.ready,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(1000),
    lastCheckedAt: DateTime.fromMillisecondsSinceEpoch(1000),
    stale: false,
  ),
);
