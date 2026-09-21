part of '../widget_test.dart';

/// 显式打开当前选中会话并等到首个权威帧后的历史窗口落地。
///
/// 生产代码的首屏只恢复“选择”，不打开会话（§6.1）；依赖订阅/实时帧/历史窗口的测试
/// 必须先显式打开，语义与用户点击“打开会话”完全一致。
Future<void> _openSelectedThread(ProviderContainer container) async {
  await container.read(studioControllerProvider.notifier).openSelectedThread();
  await pumpEventQueue();
}

Widget _timelineHarness({
  required String threadId,
  required List<ThreadItemView> items,
  StudioTurnState turnState = const CompletedStudioTurnState(
    startedAt: null,
    completedAt: 2,
    completion: StudioTurnCompletion.normal,
  ),
  Locale locale = const Locale('en'),
  VoidCallback? onLoadOlder,
  bool isLoadingOlder = false,
  StudioApi? api,
  TimelineRemoteImageProviderFactory? remoteImageProviderFactory,
  Set<String> previewedItemIds = const {},
  Set<String> loadingItemIds = const {},
  Map<String, String> itemBodyErrors = const {},
  Set<String> pendingItemBodyIds = const {},
  Set<String> unavailableItemIds = const {},
  ValueChanged<String>? onLoadItemBody,
  double height = 520,
}) {
  return _timelineApp(
    api: api,
    locale: locale,
    remoteImageProviderFactory: remoteImageProviderFactory,
    home: Scaffold(
      body: SizedBox(
        width: 980,
        height: height,
        child: TimelineView(
          threadId: threadId,
          rows: timelineRowsFromThreadItems(items),
          turn: _testTurn(threadId: threadId, state: turnState),
          onLoadOlder: onLoadOlder,
          isLoadingOlder: isLoadingOlder,
          previewedItemIds: previewedItemIds,
          loadingItemIds: loadingItemIds,
          itemBodyErrors: itemBodyErrors,
          pendingItemBodyIds: pendingItemBodyIds,
          unavailableItemIds: unavailableItemIds,
          onLoadItemBody: onLoadItemBody,
        ),
      ),
    ),
  );
}

TimelineEntry _toolTimelinePart({
  required String id,
  required String groupId,
  required String turnId,
  required String name,
  String threadId = 'session-1',
  int order = 0,
  int sequence = 0,
  String status = 'succeeded',
  String arguments = '{}',
  String? result,
  List<Object?> outputArtifacts = const [],
  String? workingDirectory,
  String? denialReason,
  int? exitCode,
  bool timedOut = false,
}) {
  return TimelineEntry(
    id: id,
    groupId: groupId,
    threadId: threadId,
    turnId: turnId,
    type: TimelineEntryType.tool,
    order: order,
    sequence: sequence,
    text: '',
    status: status,
    tool: TimelineToolPart(
      toolCallId: id,
      name: name,
      arguments: arguments,
      result: result,
      outputArtifacts: outputArtifacts,
      exitCode: exitCode,
      timedOut: timedOut,
      workingDirectory: workingDirectory,
      denialReason: denialReason,
    ),
  );
}

List<TimelineRow> timelineRowsFromFixtureParts(
  List<TimelineEntry> parts, {
  DateTime? createdAt,
}) {
  final items = <ThreadItemView>[];
  for (var index = 0; index < parts.length; index++) {
    final part = parts[index];
    final itemCreatedAt =
        part.createdAt ?? createdAt ?? _fixtureDate(index + 1);
    items.add(
      _threadItemFixture(
        id: part.id,
        threadId: part.threadId,
        turnId: part.turnId,
        ordinal: part.sequence == 0 ? index : part.sequence,
        revision: part.revision,
        status: part.status,
        createdAt: itemCreatedAt,
        updatedAt: part.updatedAt ?? itemCreatedAt,
        completedAt: part.completedAt,
        error: part.error,
        kind: switch (part.type) {
          TimelineEntryType.text =>
            part.textChannel == TimelineTextChannel.user
                ? ThreadItemKind.userMessage
                : part.textChannel == TimelineTextChannel.parentAgent
                ? ThreadItemKind.parentAgentMessage
                : ThreadItemKind.agentMessage,
          TimelineEntryType.reasoning => ThreadItemKind.reasoning,
          TimelineEntryType.tool => ThreadItemKind.toolCall,
          TimelineEntryType.skill => ThreadItemKind.skill,
          TimelineEntryType.file => ThreadItemKind.file,
        },
        text: part.text,
        channel: part.textChannel == TimelineTextChannel.commentary
            ? AgentMessageChannel.commentary
            : AgentMessageChannel.finalAnswer,
        reasoningSummary: part.reasoningSummary,
        reasoningContent: part.reasoningContent,
        tool: part.tool,
        skill: part.skill,
      ),
    );
  }
  return timelineRowsFromThreadItems(items);
}

Widget _localizedApp({
  required Widget home,
  Locale locale = const Locale('en'),
  bool disableAnimations = false,
}) {
  return MaterialApp(
    theme: pureStudioTheme(),
    themeMode: ThemeMode.light,
    locale: locale,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    builder: disableAnimations
        ? (context, child) => MediaQuery(
            data: MediaQuery.of(context).copyWith(disableAnimations: true),
            child: child!,
          )
        : null,
    home: home,
  );
}

Widget _timelineApp({
  required Widget home,
  Locale locale = const Locale('en'),
  StudioApi? api,
  ExternalUrlLauncher? externalUrlLauncher,
  TimelineRemoteImageProviderFactory? remoteImageProviderFactory,
}) {
  return ProviderScope(
    overrides: [
      if (api != null) studioApiProvider.overrideWithValue(api),
      if (externalUrlLauncher != null)
        externalUrlLauncherProvider.overrideWithValue(externalUrlLauncher),
      if (remoteImageProviderFactory != null)
        timelineRemoteImageProviderFactoryProvider.overrideWithValue(
          remoteImageProviderFactory,
        ),
    ],
    child: _localizedApp(home: home, locale: locale),
  );
}

void _configureResponsiveView(WidgetTester tester, Size size) {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
}
