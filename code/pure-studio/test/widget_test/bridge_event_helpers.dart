part of '../widget_test.dart';

ThreadSnapshotFrame _threadSnapshotFrame(StudioState state, String threadId) {
  return ThreadSnapshotFrame(workspace: state.workspacesByThread[threadId]!);
}

ThreadNotificationFrame _threadItemFrame({
  required String threadId,
  required int workspaceRevision,
  required ThreadItemView item,
}) {
  return ThreadNotificationFrame(
    threadId: threadId,
    revision: workspaceRevision,
    update: ThreadItemUpsert(item),
  );
}

ThreadNotificationFrame _threadDeltaFrame({
  required String threadId,
  required int workspaceRevision,
  required String itemId,
  required int itemRevision,
  required String field,
  required String delta,
}) {
  return ThreadNotificationFrame(
    threadId: threadId,
    revision: workspaceRevision,
    update: ThreadItemDeltaUpdate(
      ThreadItemDeltaView(
        itemId: itemId,
        revision: itemRevision,
        state: switch (field) {
          'text' => ThreadTextDeltaView(delta),
          'reasoning.summary' => ThreadThinkingSummaryDeltaView(0, delta),
          'reasoning.content' => ThreadThinkingContentDeltaView(0, delta),
          'tool.arguments' => ThreadToolArgumentsDeltaView(delta),
          'tool.result' => ThreadToolResultDeltaView(delta),
          _ => throw ArgumentError.value(field, 'field', 'unknown delta'),
        },
      ),
    ),
  );
}

ThreadNotificationFrame _threadTurnFrame({
  required String threadId,
  required int workspaceRevision,
  required StudioTurnState state,
  String turnId = 'turn-1',
}) {
  return ThreadNotificationFrame(
    threadId: threadId,
    revision: workspaceRevision,
    update: ThreadTurnUpdate(
      _testTurn(threadId: threadId, state: state, turnId: turnId),
    ),
  );
}

ThreadNotificationFrame _threadRuntimeFrame({
  required String threadId,
  required int workspaceRevision,
  required ThreadRuntimeView runtime,
  TimelineTodoListUpdate? todo,
}) {
  return ThreadNotificationFrame(
    threadId: threadId,
    revision: workspaceRevision,
    update: ThreadRuntimeUpdate(runtime: runtime, todo: todo),
  );
}

DateTime _fixtureDate(int unixSeconds) =>
    DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000);

StudioBridgeEvent _threadDirectoryChangedEvent({
  required String? projectId,
  required List<StudioThread> threads,
  List<String> removed = const [],
}) {
  return StudioBridgeEvent(
    payload: ThreadDirectoryChangedPayload(upserted: threads, removed: removed),
  );
}

StudioBridgeEvent _settingsChangedEvent(SettingsStateSnapshot settings) {
  return StudioBridgeEvent(payload: SettingsStateChangedPayload(settings));
}
