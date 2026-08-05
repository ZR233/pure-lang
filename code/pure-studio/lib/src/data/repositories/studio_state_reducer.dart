import '../../domain/models/studio_models.dart';
import '../frb/studio_api.dart';

class StudioReduceResult {
  const StudioReduceResult(this.state, {this.resyncThreadId});

  final StudioState state;
  final String? resyncThreadId;
}

/// Product stream reducer.
///
/// Thread-local turn, item, interaction, and runtime facts are deliberately
/// excluded. They are applied only through [applyThreadSnapshot] and
/// [applyThreadUpdate].
StudioReduceResult reduceStudioEvent(
  StudioState current,
  StudioBridgeEvent event,
) {
  return switch (event.payload) {
    ThreadDirectoryChangedPayload(:final projectId, :final threads) =>
      StudioReduceResult(_replaceThreadDirectory(current, projectId, threads)),
    TaskChangedPayload(:final rootThreadId, :final task) => StudioReduceResult(
      _withTask(current, rootThreadId, task),
    ),
    AgentDirectoryChangedPayload(:final agent) => StudioReduceResult(
      _withThreadDirectoryEntry(current, agent),
    ),
    McpHealthChangedPayload(:final servers) => StudioReduceResult(
      servers.isEmpty ? current : current.copyWith(mcpServers: servers),
    ),
    LspHealthChangedPayload() || StalePayload() => StudioReduceResult(current),
  };
}

/// Authoritative snapshots replace the complete canonical workspace.
///
/// Subscription generation checks belong to the controller. Once a snapshot
/// reaches this reducer it wins over every locally accumulated delta.
StudioState applyThreadSnapshot(
  StudioState current,
  ThreadWorkspace workspace,
) {
  final threadId = workspace.thread.id;
  if (threadId.isEmpty) return current;
  final directoryThread = current.threads
      .where((thread) => thread.id == threadId)
      .firstOrNull;
  final workspaces =
      Map<String, ThreadWorkspace>.from(current.workspacesByThread)
        ..[threadId] = _sortedWorkspace(
          workspace.copyWith(thread: directoryThread ?? workspace.thread),
        );
  final workspaceUi = Map<String, WorkspaceUiState>.from(
    current.workspaceUiByThread,
  );
  workspaceUi[threadId] = (workspaceUi[threadId] ?? const WorkspaceUiState())
      .copyWith(syncState: AgentWorkspaceSyncState.ready);
  return current.copyWith(
    workspacesByThread: workspaces,
    workspaceUiByThread: workspaceUi,
  );
}

StudioReduceResult applyThreadUpdate(
  StudioState current, {
  required String threadId,
  required int revision,
  required ThreadWorkspaceUpdate update,
}) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null) {
    return StudioReduceResult(current, resyncThreadId: threadId);
  }
  if (revision <= workspace.revision) {
    return StudioReduceResult(current);
  }
  if (revision != workspace.revision + 1) {
    return StudioReduceResult(current, resyncThreadId: threadId);
  }

  final updated = switch (update) {
    ThreadTurnUpdate(:final turn) => workspace.copyWith(
      revision: revision,
      activeTurn: turn.state.isBusy ? turn : null,
    ),
    ThreadItemUpsert(:final item) => _upsertThreadItem(
      workspace,
      revision,
      item,
    ),
    ThreadItemDeltaUpdate(:final delta) => _appendThreadItemDelta(
      workspace,
      revision,
      delta,
    ),
    ThreadInteractionUpdate(:final interaction, :final pending) =>
      _updateThreadInteraction(workspace, revision, interaction, pending),
    ThreadRuntimeUpdate(:final runtime, :final todo) => workspace.copyWith(
      revision: revision,
      runtime: runtime,
      todo: todo,
    ),
  };
  if (updated == null) {
    return StudioReduceResult(current, resyncThreadId: threadId);
  }
  return StudioReduceResult(
    current.copyWith(
      workspacesByThread: {...current.workspacesByThread, threadId: updated},
    ),
  );
}

StudioState mergeThreadHistoryPage(
  StudioState current,
  String threadId,
  ThreadHistoryPage page,
) {
  final workspace = current.workspacesByThread[threadId];
  if (workspace == null || page.items.isEmpty) return current;
  final byId = <String, ThreadItemView>{
    for (final item in workspace.items) item.id: item,
  };
  for (final item in page.items) {
    if (item.threadId != threadId) continue;
    final existing = byId[item.id];
    if (existing == null || item.revision > existing.revision) {
      byId[item.id] = item;
    }
  }
  final items = byId.values.toList()..sort(_compareItems);
  return current.copyWith(
    workspacesByThread: {
      ...current.workspacesByThread,
      threadId: workspace.copyWith(items: items),
    },
  );
}

StudioState mergeStudioConfigState(StudioState current, StudioState next) {
  final nextProviders = next.providers.isEmpty
      ? current.providers
      : next.providers;
  return current.copyWith(
    projects: next.projects.isEmpty ? current.projects : next.projects,
    threads: next.threads.isEmpty ? current.threads : next.threads,
    providers: [
      for (final provider in nextProviders)
        providerWithCatalogMetadata(provider, current.providerCatalog),
    ],
    defaultProviderId: next.defaultProviderId,
    providerUsages: next.providerUsages.isEmpty
        ? current.providerUsages
        : next.providerUsages,
    roles: next.roles.isEmpty ? current.roles : next.roles,
    mcpServers: next.mcpServers.isEmpty ? current.mcpServers : next.mcpServers,
    instructions: next.instructions,
    skills: next.skills,
    general: next.general,
    webSearch: next.webSearch,
    permissionMode: next.permissionMode,
    recoveryIssues: next.recoveryIssues,
  );
}

StudioState mergeStudioThreadState(
  StudioState current,
  StudioState next,
  String threadId,
) {
  final thread = next.threads
      .where((candidate) => candidate.id == threadId)
      .firstOrNull;
  if (thread == null) return current;
  final workspace = current.workspacesByThread[threadId];
  return current.copyWith(
    threads: [
      for (final candidate in current.threads)
        candidate.id == threadId ? thread : candidate,
    ],
    workspacesByThread: workspace == null
        ? current.workspacesByThread
        : {
            ...current.workspacesByThread,
            threadId: workspace.copyWith(thread: thread),
          },
  );
}

String defaultEffortForModel(
  StudioState current,
  String providerId,
  String model,
) {
  for (final provider in current.providers) {
    if (provider.id != providerId) continue;
    for (final candidate in provider.models) {
      if (candidate.slug == model && candidate.reasoningEfforts.isNotEmpty) {
        return candidate.reasoningEfforts.first;
      }
    }
  }
  return current.role('planner')?.effort ?? 'high';
}

String planFollowUpPrompt(
  PendingInteraction interaction,
  InteractionResolutionCommand resolution,
) {
  final content = resolution is PlanConfirmationResolutionCommand
      ? resolution.content?.trim() ?? ''
      : '';
  if (content.isNotEmpty) return content;
  final reason = switch (resolution) {
    PlanConfirmationResolutionCommand(:final reason) => reason?.trim() ?? '',
    ToolApprovalResolutionCommand(:final reason) => reason?.trim() ?? '',
    UserInputResolutionCommand() => '',
  };
  if (reason.isNotEmpty) return reason;
  return switch (interaction.payload) {
    PlanConfirmationInteractionPayload(:final content)
        when content.trim().isNotEmpty =>
      content.trim(),
    _ => interaction.body.trim(),
  };
}

ThreadWorkspace _sortedWorkspace(ThreadWorkspace workspace) {
  final items = [...workspace.items]..sort(_compareItems);
  return workspace.copyWith(items: items);
}

ThreadWorkspace? _upsertThreadItem(
  ThreadWorkspace workspace,
  int workspaceRevision,
  ThreadItemView incoming,
) {
  if (incoming.threadId != workspace.thread.id || incoming.id.isEmpty) {
    return null;
  }
  final items = [...workspace.items];
  final index = items.indexWhere((item) => item.id == incoming.id);
  if (index >= 0) {
    final existing = items[index];
    if (!_sameItemIdentity(existing, incoming) ||
        incoming.revision < existing.revision) {
      return null;
    }
    items[index] = incoming;
  } else {
    items.add(incoming);
  }
  items.sort(_compareItems);
  return workspace.copyWith(revision: workspaceRevision, items: items);
}

ThreadWorkspace? _appendThreadItemDelta(
  ThreadWorkspace workspace,
  int workspaceRevision,
  ThreadItemDeltaView delta,
) {
  final items = [...workspace.items];
  final index = items.indexWhere((item) => item.id == delta.itemId);
  if (index < 0) return null;
  final item = items[index];
  if (_isTerminalItemStatus(item.status)) return null;
  if (delta.revision <= item.revision) {
    return workspace.copyWith(revision: workspaceRevision);
  }
  if (delta.revision != item.revision + 1) return null;
  items[index] = item.appendDelta(
    field: delta.field,
    delta: delta.delta,
    nextRevision: delta.revision,
  );
  return workspace.copyWith(revision: workspaceRevision, items: items);
}

ThreadWorkspace _updateThreadInteraction(
  ThreadWorkspace workspace,
  int revision,
  PendingInteraction interaction,
  bool pending,
) {
  final interactions = [...workspace.interactions];
  final index = interactions.indexWhere((item) => item.id == interaction.id);
  if (!pending) {
    if (index >= 0) interactions.removeAt(index);
  } else if (index >= 0) {
    interactions[index] = interaction;
  } else {
    interactions.add(interaction);
  }
  return workspace.copyWith(revision: revision, interactions: interactions);
}

StudioState _replaceThreadDirectory(
  StudioState current,
  String? projectId,
  List<StudioThread> incoming,
) {
  final affectedProjects = projectId == null
      ? incoming.map((thread) => thread.projectId).toSet()
      : {projectId};
  final threads = projectId == null && incoming.isEmpty
      ? <StudioThread>[]
      : [
          for (final thread in current.threads)
            if (!affectedProjects.contains(thread.projectId)) thread,
          ...incoming,
        ];
  final knownIds = threads.map((thread) => thread.id).toSet();
  final threadsById = {for (final thread in threads) thread.id: thread};
  var selectedThreadId = current.selectedThreadId;
  if (selectedThreadId != null && !knownIds.contains(selectedThreadId)) {
    selectedThreadId = threads
        .where(
          (thread) =>
              thread.isRoot &&
              (projectId == null || thread.projectId == projectId),
        )
        .firstOrNull
        ?.id;
  }
  final workspaces = {
    for (final entry in current.workspacesByThread.entries)
      if (knownIds.contains(entry.key))
        entry.key: entry.value.copyWith(thread: threadsById[entry.key]),
  };
  final workspaceUi = Map<String, WorkspaceUiState>.from(
    current.workspaceUiByThread,
  )..removeWhere((id, _) => !knownIds.contains(id));
  return current.copyWith(
    threads: threads,
    selectedThreadId: selectedThreadId,
    workspacesByThread: workspaces,
    workspaceUiByThread: workspaceUi,
  );
}

StudioState _withTask(
  StudioState current,
  String rootThreadId,
  TaskRuntimeView? task,
) {
  final tasks = Map<String, TaskRuntimeView>.from(current.tasksByRootThread);
  if (task == null) {
    tasks.remove(rootThreadId);
  } else {
    tasks[rootThreadId] = task;
  }
  return current.copyWith(tasksByRootThread: tasks);
}

StudioState _withThreadDirectoryEntry(
  StudioState current,
  StudioAgentView agent,
) {
  final index = current.threads.indexWhere(
    (thread) => thread.id == agent.threadId,
  );
  if (index < 0) return current;
  final threads = [...current.threads];
  threads[index] = threads[index].copyWith(
    agentPath: agent.path,
    role: agent.role,
    status: agent.status,
  );
  final canonical = threads[index];
  final workspace = current.workspacesByThread[canonical.id];
  return current.copyWith(
    threads: threads,
    workspacesByThread: workspace == null
        ? current.workspacesByThread
        : {
            ...current.workspacesByThread,
            canonical.id: workspace.copyWith(thread: canonical),
          },
  );
}

bool _sameItemIdentity(ThreadItemView left, ThreadItemView right) {
  return left.id == right.id &&
      left.threadId == right.threadId &&
      left.turnId == right.turnId &&
      left.ordinal == right.ordinal &&
      left.kind == right.kind &&
      left.createdAt == right.createdAt;
}

bool _isTerminalItemStatus(String status) {
  return const {
    'completed',
    'failed',
    'interrupted',
    'cancelled',
    'denied',
    'budgetLimited',
  }.contains(status);
}

int _compareItems(ThreadItemView left, ThreadItemView right) {
  final ordinal = left.ordinal.compareTo(right.ordinal);
  return ordinal != 0 ? ordinal : left.id.compareTo(right.id);
}
