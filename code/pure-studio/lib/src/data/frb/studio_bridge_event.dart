part of 'studio_api.dart';

class StudioBridgeEvent {
  const StudioBridgeEvent({
    required this.payload,
    this.eventId,
    this.sequence,
    this.createdAt,
  });

  factory StudioBridgeEvent.fromProduct(frb.BridgeProductEventEnvelope event) {
    return StudioBridgeEvent(
      eventId: event.eventId,
      sequence: event.sequence,
      createdAt: _dateFromUnix(event.createdAt),
      payload: _productPayloadFromFrb(event.payload),
    );
  }

  final String? eventId;
  final BigInt? sequence;
  final DateTime? createdAt;
  final StudioBridgeEventPayload payload;
}

sealed class StudioBridgeEventPayload {
  const StudioBridgeEventPayload();
}

final class ProjectDirectoryChangedPayload extends StudioBridgeEventPayload {
  const ProjectDirectoryChangedPayload(this.state);
  final ProjectDirectoryState state;
}

/// Thread directory 增量：GUI 按身份合并进分页窗口，未加载条目的增量忽略。
final class ThreadDirectoryChangedPayload extends StudioBridgeEventPayload {
  const ThreadDirectoryChangedPayload({
    required this.upserted,
    required this.removed,
  });

  final List<StudioThread> upserted;
  final List<String> removed;
}

final class TaskDirectoryChangedPayload extends StudioBridgeEventPayload {
  const TaskDirectoryChangedPayload(this.state);
  final TaskDirectoryState state;
}

final class AgentDirectoryChangedPayload extends StudioBridgeEventPayload {
  const AgentDirectoryChangedPayload(this.state);
  final AgentDirectoryState state;
}

final class SettingsStateChangedPayload extends StudioBridgeEventPayload {
  const SettingsStateChangedPayload(this.state);
  final SettingsStateSnapshot state;
}

final class RecoveryStateChangedPayload extends StudioBridgeEventPayload {
  const RecoveryStateChangedPayload(this.state);
  final RecoveryStateSnapshot state;
}

final class McpStateChangedPayload extends StudioBridgeEventPayload {
  const McpStateChangedPayload(this.state);
  final McpStateSnapshot state;
}

final class LspStateChangedPayload extends StudioBridgeEventPayload {
  const LspStateChangedPayload(this.state);
  final LspStateSnapshot state;
}

final class SkillsStateChangedPayload extends StudioBridgeEventPayload {
  const SkillsStateChangedPayload(this.state);
  final SkillsStateSnapshot state;
}

final class ProviderUsageStateChangedPayload extends StudioBridgeEventPayload {
  const ProviderUsageStateChangedPayload(this.state);
  final ProviderUsageStateSnapshot state;
}

final class UpdaterStateChangedPayload extends StudioBridgeEventPayload {
  const UpdaterStateChangedPayload(this.state);
  final UpdaterStateSnapshot state;
}

final class StalePayload extends StudioBridgeEventPayload {
  const StalePayload({required this.laggedEvents});
  final int laggedEvents;
}
