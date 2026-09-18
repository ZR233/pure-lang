part of '../widget_test.dart';

void registerVsCodeLauncherTests() {
  test('local folder URIs normalize separators, drive letters and slashes', () {
    expect(
      buildLocalVsCodeFolderUri('/home/dev/project'),
      'vscode://file/home/dev/project/',
    );
    expect(
      buildLocalVsCodeFolderUri(r'C:\Users\dev\Project'),
      'vscode://file/c:/Users/dev/Project/',
    );
    expect(
      buildLocalVsCodeFolderUri('/home/dev/project/'),
      'vscode://file/home/dev/project/',
    );
    expect(
      buildLocalVsCodeFolderUri('/home/my project/代码'),
      'vscode://file/home/my%20project/%E4%BB%A3%E7%A0%81/',
    );
  });

  test('remote folder URIs encode the ssh alias and canonical path', () {
    expect(
      buildRemoteVsCodeFolderUri(alias: 'arm-dev', remotePath: '/srv/app'),
      'vscode://vscode-remote/ssh-remote+arm-dev/srv/app',
    );
    expect(
      buildRemoteVsCodeFolderUri(alias: 'ci box', remotePath: '/srv/a b'),
      'vscode://vscode-remote/ssh-remote+ci%20box/srv/a%20b',
    );
  });

  test('safeVsCodeUrl accepts only the two vscode folder forms', () {
    expect(safeVsCodeUrl('vscode://file/home/x/'), 'vscode://file/home/x/');
    expect(
      safeVsCodeUrl('vscode://vscode-remote/ssh-remote+a/b'),
      'vscode://vscode-remote/ssh-remote+a/b',
    );
    expect(safeVsCodeUrl('https://example.com'), isNull);
    expect(safeVsCodeUrl('vscode://edit/file.txt'), isNull);
    expect(safeVsCodeUrl('file:///home/x'), isNull);
    expect(safeVsCodeUrl('vscode://file/\u0001x'), 'vscode://file/x');
  });

  test('path scan finds candidates in any listed directory', () {
    final (entries, candidate) = Platform.isWindows
        ? (const ['C:\\Windows\\System32'], 'cmd.exe')
        : (const ['/bin'], 'sh');
    expect(pathEntriesContainExecutable(entries, [candidate]), isTrue);
    expect(
      pathEntriesContainExecutable(['/definitely-missing-dir'], [candidate]),
      isFalse,
    );
  });

  Future<void> pumpShellWithProject(
    WidgetTester tester, {
    required StudioState state,
    required List<String> opened,
    required bool vsCodeAvailable,
    _FakeStudioApi? api,
  }) async {
    _configureResponsiveView(tester, const Size(1440, 900));
    final effectiveApi =
        api ?? (_FakeStudioApi(state)..publishSnapshotOnSubscribe = true);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(effectiveApi),
          vsCodeAvailabilityProvider.overrideWith(
            (ref) async => vsCodeAvailable,
          ),
          vsCodeLauncherProvider.overrideWithValue(
            (url) async => opened.add(url),
          ),
        ],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('session overflow opens a local workspace in VS Code', (
    tester,
  ) async {
    final state = _emptyState().copyWith(
      projectDirectory: ProjectDirectoryState.fromState(
        state: _testReady([
          const StudioProject(
            id: 'project-1',
            name: 'project',
            path: '/home/dev/project',
          ),
        ]),
      ),
    );
    final opened = <String>[];
    await pumpShellWithProject(
      tester,
      state: state,
      opened: opened,
      vsCodeAvailable: true,
    );

    await tester.tap(find.byKey(StudioDriverKeys.sessionOverflow));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.sessionOpenInVsCode));
    await tester.pump();
    expect(opened, ['vscode://file/home/dev/project/']);
  });

  testWidgets('session overflow opens a remote workspace through the alias', (
    tester,
  ) async {
    final state = _emptyState().copyWith(
      projectDirectory: ProjectDirectoryState.fromState(
        state: _testReady([
          const StudioProject(
            id: 'project-1',
            name: 'project',
            path: '/srv/app',
            sshAlias: 'arm-dev',
          ),
        ]),
      ),
    );
    final api = _FakeStudioApi(state)
      ..sshServers = const [
        SshServer(
          alias: 'arm-dev',
          hostName: '192.168.100.12',
          port: 22,
          username: 'root',
          managed: true,
        ),
      ];
    final opened = <String>[];
    await pumpShellWithProject(
      tester,
      state: state,
      opened: opened,
      vsCodeAvailable: true,
      api: api,
    );

    await tester.tap(find.byKey(StudioDriverKeys.sessionOverflow));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.sessionOpenInVsCode));
    await tester.pump();
    expect(opened, ['vscode://vscode-remote/ssh-remote+arm-dev/srv/app']);
  });

  testWidgets('session overflow reports a missing ssh alias', (tester) async {
    final state = _emptyState().copyWith(
      projectDirectory: ProjectDirectoryState.fromState(
        state: _testReady([
          const StudioProject(
            id: 'project-1',
            name: 'project',
            path: '/srv/app',
            sshAlias: 'deleted-alias',
          ),
        ]),
      ),
    );
    final opened = <String>[];
    await pumpShellWithProject(
      tester,
      state: state,
      opened: opened,
      vsCodeAvailable: true,
    );

    await tester.tap(find.byKey(StudioDriverKeys.sessionOverflow));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.sessionOpenInVsCode));
    await tester.pumpAndSettle();
    expect(opened, isEmpty);
    expect(
      find.text('The SSH alias for this project is no longer in ~/.ssh/config'),
      findsOneWidget,
    );
  });

  testWidgets('session overflow stays hidden without VS Code installed', (
    tester,
  ) async {
    final opened = <String>[];
    await pumpShellWithProject(
      tester,
      state: _emptyState(),
      opened: opened,
      vsCodeAvailable: false,
    );
    expect(find.byKey(StudioDriverKeys.sessionOverflow), findsNothing);
  });
}
