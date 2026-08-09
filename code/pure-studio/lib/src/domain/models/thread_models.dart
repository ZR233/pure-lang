import 'composer_models.dart';
import 'agent_workspace_view.dart';
import 'interaction_models.dart';
import 'runtime_models.dart';
import 'studio_enums.dart';
import 'thread_directory_models.dart';
import 'timeline_models.dart';
import 'turn_models.dart';

enum ThreadItemKind {
  userMessage,
  agentMessage,
  reasoning,
  plan,
  toolCall,
  file,
}

enum AgentMessageChannel { commentary, finalAnswer }

class ThreadAttachmentView {
  const ThreadAttachmentView({
    required this.id,
    required this.mediaType,
    required this.byteSize,
    this.filename,
    this.width,
    this.height,
    this.dataUrl,
  });

  final String id;
  final String mediaType;
  final String? filename;
  final int? width;
  final int? height;
  final int byteSize;
  final String? dataUrl;
}

class ThreadItemView {
  const ThreadItemView({
    required this.id,
    required this.threadId,
    required this.turnId,
    required this.ordinal,
    required this.revision,
    required this.status,
    required this.createdAt,
    required this.updatedAt,
    required this.kind,
    this.completedAt,
    this.error,
    this.text = '',
    this.channel,
    this.attachments = const [],
    this.reasoningSummary = const [],
    this.reasoningContent = const [],
    this.tool,
    this.filePath,
    this.mediaType,
    this.contextDisposition = ThreadContextDisposition.active,
  });

  final String id;
  final String threadId;
  final String turnId;
  final int ordinal;
  final int revision;
  final String status;
  final DateTime createdAt;
  final DateTime updatedAt;
  final DateTime? completedAt;
  final String? error;
  final ThreadItemKind kind;
  final String text;
  final AgentMessageChannel? channel;
  final List<ThreadAttachmentView> attachments;
  final List<String> reasoningSummary;
  final List<String> reasoningContent;
  final TimelineToolPart? tool;
  final String? filePath;
  final String? mediaType;
  final ThreadContextDisposition contextDisposition;

  ThreadItemView appendDelta({
    required String field,
    required String delta,
    required int nextRevision,
  }) {
    if (nextRevision <= revision) {
      return this;
    }
    return copyWith(
      revision: nextRevision,
      text: switch (field) {
        'text' || 'planContent' => '$text$delta',
        _ => text,
      },
      reasoningSummary: field == 'reasoning.summary'
          ? _appendChunk(reasoningSummary, delta)
          : reasoningSummary,
      reasoningContent: field == 'reasoning.content'
          ? _appendChunk(reasoningContent, delta)
          : reasoningContent,
      tool: switch ((field, tool)) {
        ('tool.arguments', final current?) => current.copyWith(
          arguments: '${current.arguments}$delta',
        ),
        ('tool.result', final current?) => current.copyWith(
          result: '${current.result ?? ''}$delta',
        ),
        _ => tool,
      },
    );
  }

  ThreadItemView copyWith({
    int? revision,
    String? status,
    DateTime? updatedAt,
    DateTime? completedAt,
    String? error,
    String? text,
    List<String>? reasoningSummary,
    List<String>? reasoningContent,
    TimelineToolPart? tool,
    ThreadContextDisposition? contextDisposition,
  }) {
    return ThreadItemView(
      id: id,
      threadId: threadId,
      turnId: turnId,
      ordinal: ordinal,
      revision: revision ?? this.revision,
      status: status ?? this.status,
      createdAt: createdAt,
      updatedAt: updatedAt ?? this.updatedAt,
      completedAt: completedAt ?? this.completedAt,
      error: error ?? this.error,
      kind: kind,
      text: text ?? this.text,
      channel: channel,
      attachments: attachments,
      reasoningSummary: reasoningSummary ?? this.reasoningSummary,
      reasoningContent: reasoningContent ?? this.reasoningContent,
      tool: tool ?? this.tool,
      filePath: filePath,
      mediaType: mediaType,
      contextDisposition: contextDisposition ?? this.contextDisposition,
    );
  }
}

class ThreadWorkspace {
  const ThreadWorkspace({
    required this.thread,
    required this.revision,
    required this.items,
    required this.interactions,
    required this.runtime,
    this.activeTurn,
    this.todo,
  });

  final StudioThread thread;
  final int revision;
  final List<ThreadItemView> items;
  final List<PendingInteraction> interactions;
  final ThreadRuntimeView runtime;
  final StudioTurnView? activeTurn;
  final TimelineTodoListUpdate? todo;

  ThreadWorkspace copyWith({
    StudioThread? thread,
    int? revision,
    List<ThreadItemView>? items,
    List<PendingInteraction>? interactions,
    ThreadRuntimeView? runtime,
    Object? activeTurn = _workspaceUnset,
    Object? todo = _workspaceUnset,
  }) {
    return ThreadWorkspace(
      thread: thread ?? this.thread,
      revision: revision ?? this.revision,
      items: items ?? this.items,
      interactions: interactions ?? this.interactions,
      runtime: runtime ?? this.runtime,
      activeTurn: identical(activeTurn, _workspaceUnset)
          ? this.activeTurn
          : activeTurn as StudioTurnView?,
      todo: identical(todo, _workspaceUnset)
          ? this.todo
          : todo as TimelineTodoListUpdate?,
    );
  }
}

class WorkspaceUiState {
  const WorkspaceUiState({
    this.composer = const ComposerThreadState.idle(),
    this.syncState = AgentWorkspaceSyncState.loading,
    this.subscriptionGeneration = 0,
    this.history = const ThreadHistoryPagingState.initial(),
  });

  final ComposerThreadState composer;
  final AgentWorkspaceSyncState syncState;
  final int subscriptionGeneration;
  final ThreadHistoryPagingState history;

  WorkspaceUiState copyWith({
    ComposerThreadState? composer,
    AgentWorkspaceSyncState? syncState,
    int? subscriptionGeneration,
    ThreadHistoryPagingState? history,
  }) {
    return WorkspaceUiState(
      composer: composer ?? this.composer,
      syncState: syncState ?? this.syncState,
      subscriptionGeneration:
          subscriptionGeneration ?? this.subscriptionGeneration,
      history: history ?? this.history,
    );
  }
}

class ThreadHistoryPagingState {
  const ThreadHistoryPagingState({
    required this.nextCursor,
    required this.hasMore,
    required this.isLoading,
    required this.isLoaded,
    this.errorMessage,
  });

  const ThreadHistoryPagingState.initial()
    : nextCursor = null,
      hasMore = true,
      isLoading = false,
      isLoaded = false,
      errorMessage = null;

  final String? nextCursor;
  final bool hasMore;
  final bool isLoading;
  final bool isLoaded;
  final String? errorMessage;
}

const _workspaceUnset = Object();

List<String> _appendChunk(List<String> current, String delta) {
  if (current.isEmpty) {
    return [delta];
  }
  return [...current.take(current.length - 1), '${current.last}$delta'];
}
