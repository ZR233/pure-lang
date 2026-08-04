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

final class AgentDirectoryChangedPayload extends StudioBridgeEventPayload {
  const AgentDirectoryChangedPayload({
    required this.rootThreadId,
    required this.agent,
  });

  final String rootThreadId;
  final StudioAgentView agent;
}

final class ThreadDirectoryChangedPayload extends StudioBridgeEventPayload {
  const ThreadDirectoryChangedPayload({
    required this.projectId,
    required this.threads,
  });

  final String? projectId;
  final List<StudioThread> threads;
}

final class TaskChangedPayload extends StudioBridgeEventPayload {
  const TaskChangedPayload({required this.rootThreadId, this.task});

  final String rootThreadId;
  final TaskRuntimeView? task;
}

final class McpHealthChangedPayload extends StudioBridgeEventPayload {
  const McpHealthChangedPayload({
    required this.activeMcpServers,
    required this.servers,
  });

  final List<String> activeMcpServers;
  final List<McpServerSettingsView> servers;
}

final class LspHealthChangedPayload extends StudioBridgeEventPayload {
  const LspHealthChangedPayload({required this.activeLspServers});

  final List<String> activeLspServers;
}

final class StalePayload extends StudioBridgeEventPayload {
  const StalePayload({required this.laggedEvents});

  final int laggedEvents;
}
