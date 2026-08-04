part of '../widget_test.dart';

void registerStudioUpdateTests() {
  test('disabled builds never call the update service', () async {
    final api = _FakeStudioUpdateApi(const StudioUpdateUpToDate());
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

  test('startup check publishes available and up-to-date states', () async {
    final availableApi = _FakeStudioUpdateApi(
      const StudioUpdateAvailable(_testStudioUpdate),
    );
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

    final currentApi = _FakeStudioUpdateApi(const StudioUpdateUpToDate());
    final current = _updateContainer(currentApi);
    addTearDown(current.dispose);
    current.read(studioUpdateControllerProvider);
    await _flushUpdateController();

    expect(
      current.read(studioUpdateControllerProvider).phase,
      StudioUpdatePhase.upToDate,
    );
  });

  test('check errors are retryable', () async {
    final api = _FakeStudioUpdateApi(
      const StudioUpdateUpToDate(),
      checkError: StateError('offline'),
    );
    final container = _updateContainer(api);
    addTearDown(container.dispose);
    container.read(studioUpdateControllerProvider);
    await _flushUpdateController();

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
    final api = _FakeStudioUpdateApi(
      const StudioUpdateAvailable(_testStudioUpdate),
    );
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
    final api = _FakeStudioUpdateApi(
      const StudioUpdateAvailable(_testStudioUpdate),
    );
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
      final api = _FakeStudioUpdateApi(
        const StudioUpdateAvailable(_testStudioUpdate),
      );
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
    final updateApi = _FakeStudioUpdateApi(
      const StudioUpdateAvailable(_testStudioUpdate),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(
            _FakeStudioApi(
              _withSelectedTurn(
                _stateWithPlannerModels(),
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
    final updateApi = _FakeStudioUpdateApi(
      const StudioUpdateAvailable(_testStudioUpdate),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(
            _FakeStudioApi(_stateWithPlannerModels()),
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
    final updateApi = _FakeStudioUpdateApi(
      const StudioUpdateAvailable(_testStudioUpdate),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(
            _FakeStudioApi(_stateWithPlannerModels()),
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

  StudioUpdateCheckResult checkResult;
  Object? checkError;
  final installStarted = Completer<void>();
  final _installEvents = StreamController<StudioUpdateInstallEvent>();
  int checkCount = 0;
  int installCount = 0;
  String? openedNotesUrl;
  late _FakeStudioUpdateOperation operation;

  @override
  Future<StudioUpdateCheckResult> check(String currentVersion) async {
    checkCount += 1;
    if (checkError case final error?) throw error;
    return checkResult;
  }

  @override
  Future<StudioUpdateOperation> startInstall(StudioUpdateInfo update) async {
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

const _testStudioUpdate = StudioUpdateInfo(
  version: '1.1.0',
  publishedAt: 1,
  notesUrl: 'https://github.com/ZR233/pure-lang/releases/tag/v1.1.0',
  installerUrl:
      'https://github.com/ZR233/pure-lang/releases/download/v1.1.0/'
      'Pure-Studio-1.1.0-windows-x86_64-setup.exe',
  installerSize: 100,
  installerSha256:
      '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
  installerSignatureUrl:
      'https://github.com/ZR233/pure-lang/releases/download/v1.1.0/'
      'Pure-Studio-1.1.0-windows-x86_64-setup.exe.minisig',
);
