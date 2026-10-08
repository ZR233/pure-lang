import 'thread_models.dart';

/// Timeline 的唯一 UI 命令入口。
///
/// 滚动、分页、正文补齐和阅读锚点都以同一组 typed 命令进入 data 层，避免
/// Widget 同时持有多个相互独立的业务回调。
sealed class TimelineCommand {
  const TimelineCommand();
}

/// 会话窗口向界面交付的事件。事件只属于当前打开的会话，界面不轮询数据源。
sealed class TimelineEvent {
  const TimelineEvent(this.threadId);
  final String threadId;
}

final class TimelineWindowReset extends TimelineEvent {
  const TimelineWindowReset(super.threadId, this.version);
  final int version;
}

final class TimelineWindowPatch extends TimelineEvent {
  const TimelineWindowPatch(super.threadId, this.fromVersion, this.version);
  final int fromVersion;
  final int version;
}

final class TimelinePagingStarted extends TimelineEvent {
  const TimelinePagingStarted(super.threadId, this.direction);
  final TimelineDirection direction;
}

final class TimelinePagingCompleted extends TimelineEvent {
  const TimelinePagingCompleted(super.threadId, this.direction, this.version);
  final TimelineDirection direction;
  final int version;
}

final class TimelinePagingFailed extends TimelineEvent {
  const TimelinePagingFailed(super.threadId, this.direction, this.error);
  final TimelineDirection direction;
  final Object error;
}

final class TimelineBodyLoadCompleted extends TimelineEvent {
  const TimelineBodyLoadCompleted(super.threadId, this.itemId, this.version);
  final String itemId;
  final int version;
}

final class TimelineRuntimeChanged extends TimelineEvent {
  const TimelineRuntimeChanged(super.threadId);
}

final class TimelineStreamLagged extends TimelineEvent {
  const TimelineStreamLagged(super.threadId, this.version);
  final int version;
}

final class TimelineSessionClosed extends TimelineEvent {
  const TimelineSessionClosed(super.threadId);
}

/// 当前打开会话的唯一命令/事件边界。具体状态由宿主 reducer 投影。
abstract interface class TimelineSession {
  Stream<TimelineEvent> get events;
  Future<void> dispatch(TimelineCommand command);
  Future<void> close();
}

final class TimelineLoadOlder extends TimelineCommand {
  const TimelineLoadOlder();
}

final class TimelineLoadNewer extends TimelineCommand {
  const TimelineLoadNewer();
}

final class TimelineExtendLatest extends TimelineCommand {
  const TimelineExtendLatest();
}

final class TimelineJumpToLatest extends TimelineCommand {
  const TimelineJumpToLatest();
}

final class TimelineExpandBody extends TimelineCommand {
  const TimelineExpandBody(this.itemId);
  final String itemId;
}

final class TimelineVisibleBodies extends TimelineCommand {
  const TimelineVisibleBodies(this.itemIds);
  final List<String> itemIds;
}

final class TimelineAnchorChanged extends TimelineCommand {
  const TimelineAnchorChanged(this.anchor);
  final TimelineAnchor anchor;
}
