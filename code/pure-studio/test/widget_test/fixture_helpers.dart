part of '../widget_test.dart';

Widget _timelineHarness({
  required String threadId,
  required List<ThreadItemView> items,
  StudioTurnState turnState = const CompletedStudioTurnState(
    startedAt: null,
    completedAt: 2,
    completion: StudioTurnCompletion.normal,
  ),
  VoidCallback? onLoadOlder,
  bool isLoadingOlder = false,
  StudioApi? api,
  TimelineRemoteImageProviderFactory? remoteImageProviderFactory,
}) {
  return _timelineApp(
    api: api,
    remoteImageProviderFactory: remoteImageProviderFactory,
    home: Scaffold(
      body: SizedBox(
        width: 980,
        height: 520,
        child: TimelineView(
          threadId: threadId,
          rows: timelineRowsFromThreadItems(items),
          turn: _testTurn(threadId: threadId, state: turnState),
          onLoadOlder: onLoadOlder,
          isLoadingOlder: isLoadingOlder,
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
                : ThreadItemKind.agentMessage,
          TimelineEntryType.reasoning => ThreadItemKind.reasoning,
          TimelineEntryType.plan => ThreadItemKind.plan,
          TimelineEntryType.tool => ThreadItemKind.toolCall,
          TimelineEntryType.skill => ThreadItemKind.skill,
          TimelineEntryType.file => ThreadItemKind.file,
        },
        text: part.planContent ?? part.text,
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

bool _isLiftedDetailCard(Widget widget) {
  if (widget is! DecoratedBox) {
    return false;
  }
  final decoration = widget.decoration;
  return decoration is BoxDecoration &&
      decoration.border != null &&
      (decoration.boxShadow?.isNotEmpty ?? false);
}
