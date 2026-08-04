part of '../widget_test.dart';

Widget _timelineHarness({
  required String sessionId,
  required List<_ProjectedMessageFixture> messages,
  StudioTurnState turnState = const StudioTurnState.completed(),
  VoidCallback? onLoadOlder,
  bool isLoadingOlder = false,
}) {
  return _timelineApp(
    home: Scaffold(
      body: SizedBox(
        width: 980,
        height: 520,
        child: TimelineView(
          sessionId: sessionId,
          rows: _rowsFromProjectedMessages(messages),
          turn: _testTurn(sessionId: sessionId, state: turnState),
          onLoadOlder: onLoadOlder,
          isLoadingOlder: isLoadingOlder,
        ),
      ),
    ),
  );
}

class _ProjectedMessageFixture {
  const _ProjectedMessageFixture({required this.message, required this.parts});

  final TimelineMessage message;
  final List<TimelinePart> parts;
}

TimelinePart _toolTimelinePart({
  required String id,
  required String messageId,
  required String turnId,
  required String name,
  String sessionId = 'session-1',
  int order = 0,
  int sequence = 0,
  String status = 'completed',
  String arguments = '{}',
  String? result,
  List<Object?> outputArtifacts = const [],
  String? workingDirectory,
  String? denialReason,
  int? exitCode,
  bool timedOut = false,
}) {
  final now = DateTime.fromMillisecondsSinceEpoch(0);
  return timelinePartFromSnapshot(
    TimelinePartSnapshot(
      id: id,
      messageId: messageId,
      sessionId: sessionId,
      turnId: turnId,
      type: TimelinePartType.tool,
      order: order,
      revision: 0,
      sequence: sequence,
      text: '',
      status: status,
      createdAt: now,
      updatedAt: now,
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
    ),
  );
}

List<TimelineRow> _rowsFromProjectedMessages(
  List<_ProjectedMessageFixture> fixtures,
) {
  return timelineRowsFromMessages(
    [for (final fixture in fixtures) fixture.message],
    parts: [for (final fixture in fixtures) ...fixture.parts],
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

Widget _timelineApp({
  required Widget home,
  Locale locale = const Locale('en'),
}) {
  return ProviderScope(
    child: _localizedApp(home: home, locale: locale),
  );
}

Future<void> _pumpFrameBatch() async {
  await pumpEventQueue();
  final binding = SchedulerBinding.instance;
  binding.handleBeginFrame(const Duration(milliseconds: 16));
  binding.handleDrawFrame();
  await pumpEventQueue();
}
