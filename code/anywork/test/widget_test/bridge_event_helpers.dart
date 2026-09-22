part of '../widget_test.dart';

ThreadSnapshotFrame _threadSnapshotFrame(StudioState state, String threadId) {
  return ThreadSnapshotFrame(
    workspace: _currentStateOnly(state.workspacesByThread[threadId]!),
  );
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
      _testTurn(
        threadId: threadId,
        state: state,
        turnId: turnId,
        revision: workspaceRevision,
      ),
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

ThreadItemView _submittedInputItem({
  required String threadId,
  required String turnId,
  required String inputId,
}) {
  final at = _fixtureDate(1);
  return ThreadItemView(
    id: inputId,
    threadId: threadId,
    turnId: turnId,
    ordinal: 1,
    revision: 1,
    createdAt: at,
    updatedAt: at,
    state: ThreadTextItemStateView(
      channel: ThreadTextChannel.user,
      text: 'submitted input',
      attachments: const [],
      lifecycle: CompletedThreadContentView(at),
    ),
  );
}
