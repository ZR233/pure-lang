import 'dart:convert';

import '../domain/models/studio_models.dart';

/// Read-only state exported through Flutter Driver in local acceptance builds.
abstract final class StudioDriverState {
  static StudioProject? _project;
  static AgentWorkspaceView? _workspace;
  static ThreadWorkspace? _timelineWorkspace;
  static ThreadHistoryWindow _history = const ThreadHistoryWindow();
  static Map<String, ThreadWorkspace> _workspacesByThread = const {};
  static final List<StudioShutdownProgress> _shutdownProgress = [];
  static List<String> _sidebarDirectoryIds = const [];
  static List<StudioThread> _currentRootThreads = const [];
  static bool _sidebarDirectoryHasMore = false;
  static String? _selectedProjectId;
  static String? _selectedThreadId;
  static ThreadModeId _newThreadMode = ThreadModeId.simple;
  static ThreadWorkspaceMode _newThreadWorkspaceMode =
      ThreadWorkspaceMode.local;
  static ComposerThreadState _newThreadComposer =
      const ComposerThreadState.idle();
  static int _settingsRevision = 0;
  static List<ModeModelRouteView> _modeModelRoutes = const [];
  static List<RoleSettingsView> _roles = const [];
  static List<ProviderSettingsView> _providers = const [];
  static PersistenceStateSnapshot _persistenceState =
      const PersistenceStateSnapshot.ready();
  static ConversationActivityView? _conversationActivity;
  static bool _activityExpanded = false;
  static TimelineScrollDiagnostic? _timelineScroll;

  /// 内容交付计数是否启用。
  ///
  /// **只在 Driver 构建启用**：编译期 `ANYWORK_DRIVER`（`cargo xtask run-gui --driver`
  /// 注入的 dart-define）为真时默认开启，Driver 入口也可用
  /// [enableContentDeliveryMetrics] 显式开启。普通运行与 release 恒为关闭，完全不
  /// 统计、也不为统计序列化正文。
  static bool _contentMetricsEnabled = const bool.fromEnvironment(
    'ANYWORK_DRIVER',
  );
  static int _contentResets = 0;
  static int _contentPatches = 0;
  static int _contentChanges = 0;
  static int _contentBodyBytes = 0;
  static int _contentMaxWindowItems = 0;

  /// Driver 入口启用内容交付计数。这是应用载荷计量，不是 FRB 物理 wire 字节。
  static void enableContentDeliveryMetrics() {
    _contentMetricsEnabled = true;
  }

  static void resetContentDeliveryMetrics() {
    _contentResets = 0;
    _contentPatches = 0;
    _contentChanges = 0;
    _contentBodyBytes = 0;
    _contentMaxWindowItems = 0;
  }

  /// 记录一次权威整窗交付（窗口条目数）。
  static void recordChatWindowReset(int windowItems) {
    if (!_contentMetricsEnabled) return;
    _contentResets++;
    if (windowItems > _contentMaxWindowItems) {
      _contentMaxWindowItems = windowItems;
    }
  }

  /// 记录一次差量交付；[windowItems] 是交付后的窗口条目数。
  static void recordChatWindowPatch(int windowItems) {
    if (!_contentMetricsEnabled) return;
    _contentPatches++;
    if (windowItems > _contentMaxWindowItems) {
      _contentMaxWindowItems = windowItems;
    }
  }

  /// 记录一次正文内容增量（按 UTF-8 字节计）。
  static void recordContentDelta(String delta) {
    if (!_contentMetricsEnabled) return;
    _contentChanges++;
    if (delta.isEmpty) return;
    _contentBodyBytes += _utf8Length(delta);
  }

  /// 记录一次整条/多条正文交付（按 UTF-8 字节计的是正文文本，不是 JSON 载荷）。
  static void recordContentBody(String text) {
    if (!_contentMetricsEnabled) return;
    if (text.isEmpty) return;
    _contentBodyBytes += _utf8Length(text);
  }

  /// 直接按 code unit 数出 UTF-8 字节长度。
  ///
  /// 正文可以很大，`utf8.encode` 会为每次交付复制一份字节；这里的计数是只读诊断，
  /// 因此按 code unit 就地累加，不额外物化全文副本。
  static int _utf8Length(String text) {
    var bytes = 0;
    for (var index = 0; index < text.length; index += 1) {
      final unit = text.codeUnitAt(index);
      if (unit < 0x80) {
        bytes += 1;
      } else if (unit < 0x800) {
        bytes += 2;
      } else if (unit >= 0xD800 && unit < 0xDC00 && index + 1 < text.length) {
        // 高位代理：与紧随的低位代理合成一个 4 字节字符。
        bytes += 4;
        index += 1;
      } else {
        bytes += 3;
      }
    }
    return bytes;
  }

  static void publishState(StudioState state) {
    _timelineWorkspace = state.selectedWorkspace;
    _history = state.selectedWorkspaceUi.history;
    _workspacesByThread = state.workspacesByThread;
    _selectedProjectId = state.selectedProjectId;
    _selectedThreadId = state.selectedThreadId;
    _currentRootThreads = List.unmodifiable(state.rootThreads);
    _newThreadMode = state.newThreadMode;
    _newThreadWorkspaceMode = state.newThreadWorkspaceMode;
    _newThreadComposer = state.newThreadComposer;
    _settingsRevision = state.settingsRevision;
    _modeModelRoutes = List.unmodifiable(state.modeModelRoutes);
    _providers = List.unmodifiable(state.providers);
    _roles = List.unmodifiable([
      for (final role in state.roles)
        RoleSettingsView(
          key: role.key,
          providerId: role.providerId,
          model: role.model,
          effort: role.effort,
        ),
    ]);
    _persistenceState = state.persistenceState;
    _project = state.projects
        .where((project) => project.id == state.selectedProjectId)
        .firstOrNull;
    final workspace = state.selectedAgentWorkspace;
    if (workspace == null) {
      _workspace = null;
    } else {
      publishWorkspace(workspace);
    }
    publishSidebarDirectory([
      for (final thread in state.rootThreads) thread.id,
    ], state.threadDirectory.hasMore);
  }

  static void publishProject(StudioProject? project) {
    _project = project;
  }

  static void publishWorkspace(AgentWorkspaceView workspace) {
    _workspace = workspace;
  }

  /// 固定活动条的当前投影；驱动据此断言阶段/摘要/详情而无需从渲染文本猜测。
  static void publishConversationActivity(ConversationActivityView? view) {
    _conversationActivity = view;
  }

  /// 固定活动条的**实际展开态**（用户此刻是否展开详情）。
  ///
  /// 与快照里的 `expandable`（该活动是否有可展开的完整内容）语义不同：`expandable` 是
  /// 能力，`expanded` 是当前状态。仅由活动条在展开/收起或身份变化时上报。
  static void publishActivityExpanded(bool expanded) {
    _activityExpanded = expanded;
  }

  /// Timeline 滚动几何诊断；只读投影，不是第二份业务状态。
  static void publishTimelineScroll(TimelineScrollDiagnostic? diagnostic) {
    _timelineScroll = diagnostic;
  }

  static void publishSidebarDirectory(List<String> threadIds, bool hasMore) {
    _sidebarDirectoryIds = List.unmodifiable(threadIds);
    _sidebarDirectoryHasMore = hasMore;
  }

  static void publishShutdownProgress(StudioShutdownProgress progress) {
    _shutdownProgress.add(progress);
  }

  static List<StudioShutdownProgress> get shutdownProgress =>
      List.unmodifiable(_shutdownProgress);

  static String snapshotJson() {
    final workspace = _workspace;
    final lastTurn = workspace?.lastTurn;
    return jsonEncode({
      'timelineWindow': {
        'itemIds':
            _timelineWorkspace?.items.map((item) => item.id).toList() ?? [],
        // 有界预览与显式回源身份：驱动据此按 canonical item id 定位大正文，
        // 无需从渲染文本里猜测。
        'previewedItemIds': [
          for (final item
              in _timelineWorkspace?.items ?? const <ThreadItemView>[])
            if (_history.previewedItemIds.contains(item.id)) item.id,
        ],
        // 窗口内正文完整的条目：窗口是唯一正文 owner，展开后省略量归零即视为已完整。
        'loadedItemIds': [
          for (final item
              in _timelineWorkspace?.items ?? const <ThreadItemView>[])
            if (!item.bodyPreviewed) item.id,
        ],
        // 回源已发出但完整正文尚未可取（例如历史事务尚未 durable）：入口仍可见可重试。
        'pendingItemBodyIds': [
          for (final item
              in _timelineWorkspace?.items ?? const <ThreadItemView>[])
            if (_history.pendingItemBodyIds.contains(item.id)) item.id,
        ],
        // ChatView 窗口条目数（唯一正文源）。
        'windowItemCount': _timelineWorkspace?.items.length ?? 0,
        'hasOlder': _history.hasOlder,
        'hasNewer': _history.hasNewer,
        'loading': _history.isLoading,
        'direction': _history.direction.name,
        'epoch': _history.epoch,
        'olderError': _history.errorMessage,
        'newerError': _history.newerError,
        'anchor': _history.anchor == null
            ? null
            : {
                'itemId': _history.anchor!.itemId,
                'offset': _history.anchor!.offset,
                'followingBottom': _history.anchor!.followingBottom,
              },
      },
      // 只读滚动几何：判断“末尾是否贴底、内容是否填满视口、上翻历史是否还在原锚点”
      // 不必从截图猜测（见 TimelineScrollDiagnostic）。
      'timelineScroll': _timelineScroll?.toJson(),
      'sidebarDirectory': {
        'count': _sidebarDirectoryIds.length,
        'hasMore': _sidebarDirectoryHasMore,
        'ids': _sidebarDirectoryIds,
        'titles': {
          for (final thread in _currentRootThreads) thread.id: thread.title,
        },
        'workspaceModes': {
          for (final thread in _currentRootThreads)
            thread.id: thread.workspaceMode.id,
        },
        'workspacePaths': {
          for (final thread in _currentRootThreads)
            thread.id: thread.workspacePath,
        },
      },
      'navigation': {
        'selectedProjectId': _selectedProjectId,
        'selectedThreadId': _selectedThreadId,
        'isStartPage': _selectedThreadId == null,
        'newThreadMode': _newThreadMode.name,
        'newThreadWorkspaceMode': _newThreadWorkspaceMode.id,
        'newThreadComposer': {
          'draft': _newThreadComposer.draft,
          'phase': switch (_newThreadComposer) {
            IdleComposerThreadState() => 'idle',
            SubmittingComposerThreadState() => 'submitting',
            FailedComposerThreadState() => 'failed',
          },
          'submissionRevision': _newThreadComposer.submissionRevision,
          'error': _newThreadComposer.error,
          'attachments': [
            for (final attachment in _newThreadComposer.attachments)
              {
                'id': attachment.id,
                'modality': attachment.modality.name,
                'filename': attachment.filename,
                'byteSize': attachment.byteSize,
              },
          ],
        },
      },
      'shutdownPhases': <String>[
        for (final progress in _shutdownProgress)
          '${progress.phase.name}:${switch (progress) {
            FlushingPersistenceProgress(:final pendingCommits) => pendingCommits,
            StoppingSubscriptionsProgress() || CancellingTurnsProgress() || StoppingAgentsProgress() || StoppingMcpProgress() || StoppingLspProgress() || StoppedProgress() => 0,
          }}',
      ],
      'settings': {
        'revision': _settingsRevision,
        'providers': [
          for (final provider in _providers)
            {
              'id': provider.id,
              'preset': provider.templateKind,
              'defaultModel': provider.defaultModel,
              'pricingEnabled': provider.pricingEnabled,
              'hasBearerToken': provider.hasBearerToken,
              'status': provider.status,
              'models': [for (final model in provider.allModels) model.slug],
            },
        ],
        'roles': [
          for (final role in _roles)
            {
              'key': role.key,
              'providerId': role.providerId,
              'model': role.model,
              'effort': role.effort,
            },
        ],
        'modeModelRoutes': [
          for (final route in _modeModelRoutes)
            {
              'modeId': route.modeId.id,
              'providerId': route.providerId,
              'model': route.model,
              'effort': route.effort,
            },
        ],
      },
      'persistence': {
        'revision': _persistenceState.revision,
        'kind': switch (_persistenceState.state) {
          ReadyPersistenceState() => 'ready',
          FlushingPersistenceState() => 'flushing',
          DegradedPersistenceState() => 'degraded',
          RecoveringPersistenceState() => 'recovering',
          BlockedPersistenceState() => 'blocked',
        },
        'pendingCommits': _persistenceState.state.pendingCommits,
        'oldestPendingRevision': _persistenceState.state.oldestPendingRevision,
        'firstFailedAt': _persistenceState.state.firstFailedAt,
        'needsAttention': _persistenceState.needsAttention,
        'errorCode': _persistenceState.state.error?.code,
      },
      'project': _project == null
          ? null
          : {'id': _project!.id, 'path': _project!.path},
      'workspace': workspace == null
          ? null
          : {
              'threadId': workspace.thread.id,
              'syncState': workspace.syncState.name,
              'loadError': workspace.loadError,
              'title': workspace.rootThread.title,
              'projectId': workspace.thread.projectId,
              'rootThreadId': workspace.rootThread.id,
              'threadMode': workspace.thread.mode.name,
              'threadStatus': workspace.thread.status.name,
              'isBusy': workspace.isBusy,
              'model': workspace.runtime.model,
              'modelRoute': workspace.runtime.modelRoute == null
                  ? null
                  : {
                      'providerId': workspace.runtime.modelRoute!.providerId,
                      'model': workspace.runtime.modelRoute!.model,
                      'effort': workspace.runtime.modelRoute!.effort,
                      'revision': workspace.runtime.modelRoute!.revision,
                      'available': workspace.runtime.modelRoute!.available,
                      'unavailableReason':
                          workspace.runtime.modelRoute!.unavailableReason,
                    },
              'usage': {
                'inputTokens': workspace.runtime.promptTokens,
                'outputTokens': workspace.runtime.completionTokens,
                'cacheReadTokens': workspace.runtime.cachedPromptTokens,
                'hasIncompleteUsage': workspace.runtime.hasIncompleteUsage,
              },
              'modelCapabilities': _modelCapabilities(workspace),
              'modelProvider': _modelProvider(workspace),
              'composer': {
                'mode': workspace.composerMode.name,
                'draft': workspace.composer.draft,
                'submissionPending': workspace.composer.isSubmissionPending,
                'lockedByInteraction': workspace.activeInteraction != null,
                'attachments': [
                  for (final attachment in workspace.composer.attachments)
                    {
                      'id': attachment.id,
                      'modality': _attachmentModalityName(attachment.modality),
                      'filename': attachment.filename,
                      'byteSize': attachment.byteSize,
                    },
                ],
              },
              'agents': [
                for (final agent in workspace.agents)
                  {
                    'id': agent.id,
                    'threadId': agent.threadId,
                    'rootThreadId': agent.rootThreadId,
                    'path': agent.path,
                    'parentPath': agent.parentPath,
                    'role': agent.role,
                    'task': agent.task,
                    'status': agent.status,
                    'error': agent.error,
                  },
              ],
              'historyAttachments': [
                for (final row in workspace.timelineRows)
                  for (final attachment in row.part?.attachments ?? const [])
                    {
                      'id': attachment.id,
                      'modality': _attachmentModalityName(attachment.modality),
                      'filename': attachment.filename,
                      'byteSize': attachment.byteSize,
                    },
                for (final row in workspace.timelineRows)
                  for (final item in row.toolGroup?.items ?? const [])
                    for (final attachment in item.tool?.attachments ?? const [])
                      {
                        'id': attachment.id,
                        'modality': _attachmentModalityName(
                          attachment.modality,
                        ),
                        'filename': attachment.filename,
                        'byteSize': attachment.byteSize,
                        'source': 'tool',
                      },
              ],
              'timeline': [
                for (final row in workspace.timelineRows)
                  {
                    'id': row.id,
                    'type': row.type.name,
                    'text': row.part?.text,
                    'tools': [
                      for (final item in row.toolGroup?.items ?? const [])
                        if (item.tool case final tool?)
                          {
                            'name': tool.name,
                            'callId': tool.callId ?? tool.toolCallId,
                            'status': item.status,
                            'arguments': tool.arguments,
                            'result': tool.result,
                            // 在途工具的只读采样字段（同一 typed 事实的派生视图，不是第二份
                            // 状态源、不做内容推断）：`output` 与 `result` 同源——运行中即窗口
                            // 已交付的 streamed 输出、终态即最终输出；`progressBytes` 是该输出
                            // 当前 UTF-8 字节数，逐次增长即在途真实产出；`itemRevision` 是承载
                            // 该工具的窗口条目 revision，供按版本采样。
                            'output': tool.result,
                            'progressBytes': _utf8Length(tool.result ?? ''),
                            'itemRevision': item.part.revision,
                            'denialReason': tool.denialReason,
                            'workingDirectory': tool.workingDirectory,
                            'exitCode': tool.exitCode,
                            'attachments': [
                              for (final attachment in tool.attachments)
                                {
                                  'id': attachment.id,
                                  'modality': _attachmentModalityName(
                                    attachment.modality,
                                  ),
                                  'filename': attachment.filename,
                                  'byteSize': attachment.byteSize,
                                },
                            ],
                          },
                    ],
                    'attachments': [
                      for (final attachment
                          in row.part?.attachments ?? const [])
                        {
                          'id': attachment.id,
                          'modality': _attachmentModalityName(
                            attachment.modality,
                          ),
                          'filename': attachment.filename,
                          'byteSize': attachment.byteSize,
                        },
                    ],
                  },
              ],
              'interactionCount': workspace.activeInteraction == null ? 0 : 1,
              'activeInteraction': workspace.activeInteraction == null
                  ? null
                  : {
                      'id': workspace.activeInteraction!.id,
                      'turnId': workspace.activeInteraction!.turnId,
                      'kind': workspace.activeInteraction!.kind.name,
                      'title': workspace.activeInteraction!.title,
                      'body': workspace.activeInteraction!.body,
                    },
              'turn': workspace.turn == null
                  ? null
                  : {
                      'id': workspace.turn!.turnId,
                      'status': workspace.turn!.state.status.name,
                      'activity': workspace.turn!.state.activity?.name,
                      'reason': workspace.turn!.state.reason,
                      'updatedAt': workspace.turn!.updatedAt
                          .toUtc()
                          .toIso8601String(),
                    },
              'lastTurn': lastTurn == null ? null : _turnJson(lastTurn),
              'timelineProgress': _timelineProgress(workspace),
            },
      // Workflow is the only lifecycle projection exposed to acceptance
      // drivers. Product-specific orchestration payloads are intentionally not
      // serialized.
      'workflow': workspace?.runtime.workflow == null
          ? null
          : _workflowJson(workspace!.runtime.workflow!),
      // 固定活动条投影：阶段/摘要/并行工具数/完整详情身份。
      'conversationActivity': _conversationActivity == null
          ? null
          : {
              'identity': _conversationActivity!.identity,
              'kind': _conversationActivity!.kind.name,
              'summary': _conversationActivity!.summary,
              'activeToolCount': _conversationActivity!.activeToolCount,
              'backgroundToolCount': _conversationActivity!.backgroundToolCount,
              'errorMessage': _conversationActivity!.errorMessage,
              'expandable': _conversationActivity!.expandable,
              // `expandable` 是能力；`expanded` 是用户此刻的实际展开态。
              'expanded': _activityExpanded,
              'detailsLoading': _conversationActivity!.detailsLoading,
              'detailsError': _conversationActivity!.detailsError,
              'details': [
                for (final detail in _conversationActivity!.details)
                  {'id': detail.id, 'title': detail.title, 'body': detail.body},
              ],
            },
      // 内容交付计数：**应用载荷指标**，不是 FRB 物理 wire 字节。仅 Driver 启用时统计。
      'contentDelivery': {
        'enabled': _contentMetricsEnabled,
        'resets': _contentResets,
        'patches': _contentPatches,
        'contentChanges': _contentChanges,
        'bodyUtf8Bytes': _contentBodyBytes,
        'maxWindowItems': _contentMaxWindowItems,
        'metric': 'application-payload',
      },
      // 保存故障/恢复的 typed 诊断（只读投影，不是第二份业务状态）。
      //
      // 每个已观测到存储事实的 Thread 一条：typed 故障类别、代数与水位、执行阶段、
      // 硬故障闩 `resumeRequired` 与后端核验的 `canResume`。fixture 用 `history-retry-<id>`
      // 与 `history-resume-<id>`（本块给出与其一致的事实）校验“重试保存 / 继续执行”分离。
      // `null` 一律表示未知，不折叠成 0/健康。
      'storageRecovery': [
        for (final entry in _workspacesByThread.entries)
          if (entry.value.storage case final storage?)
            {
              'threadId': entry.key,
              'title': entry.value.thread.title,
              'fault': storage.fault?.name,
              'faultGeneration': storage.faultGeneration,
              'acceptedSequence': storage.acceptedSequence,
              'durableSequence': storage.durableSequence,
              'execution': storage.execution.name,
              'pressurePaused': storage.pressurePaused,
              'resumeRequired': storage.resumeRequired,
              'canResume': storage.canResume,
              'blocksContinuation': storage.blocksContinuation,
              'lastError': storage.lastError,
            },
      ],
    });
  }

  static Map<String, Object?> _workflowJson(WorkflowRuntimeView workflow) {
    final run = workflow.currentRun;
    return {
      'revision': workflow.revision,
      'currentRun': run == null
          ? null
          : {
              'lineageId': run.lineageId,
              'runId': run.runId,
              'modeId': run.modeId,
              'graphRevision': run.graphRevision,
              'graphHash': run.graphHash,
              'currentStateId': run.currentStateId,
              'terminal': run.terminal,
            },
    };
  }

  static List<String> _modelCapabilities(AgentWorkspaceView workspace) {
    for (final provider in workspace.providers) {
      for (final model in provider.allModels) {
        if (model.slug == workspace.runtime.model) {
          return [
            for (final capability in model.inputCapabilities)
              capability.modality.name,
          ];
        }
      }
    }
    return const [];
  }

  static Map<String, Object?>? _modelProvider(AgentWorkspaceView workspace) {
    for (final provider in workspace.providers) {
      if (provider.allModels.any(
        (model) => model.slug == workspace.runtime.model,
      )) {
        return {
          'id': provider.id,
          'hasBearerToken': provider.hasBearerToken,
          'credentialEnv': provider.credentialEnv,
        };
      }
    }
    return null;
  }

  static Map<String, Object?> _timelineProgress(AgentWorkspaceView workspace) {
    final rows = workspace.timelineRows;
    return {
      'rowCount': rows.length,
      'lastSequence': rows.fold<int>(
        0,
        (latest, row) => row.sequence > latest ? row.sequence : latest,
      ),
      'renderVersion': Object.hashAll([
        for (final row in rows) row.id,
        for (final row in rows) row.sequence,
        for (final row in rows) row.renderVersion,
      ]),
      // Driver 顺序验收：完整行序列（id/type/text/sequence）。
      'rows': [
        for (final row in rows)
          {
            'id': row.id,
            'type': row.type.name,
            'text': row.part?.text,
            'sequence': row.sequence,
          },
      ],
    };
  }

  static Map<String, Object?> _turnJson(StudioTurnView turn) => {
    'id': turn.turnId,
    'inputId': turn.inputId,
    'threadId': turn.threadId,
    'revision': turn.revision,
    'status': turn.state.status.name,
    'activity': turn.state.activity?.name,
    'reason': turn.state.reason,
    'updatedAt': turn.updatedAt.toUtc().toIso8601String(),
  };

  static String _attachmentModalityName(AttachmentModalityView modality) =>
      switch (modality) {
        AttachmentModalityView.image => 'image',
        AttachmentModalityView.video => 'video',
        AttachmentModalityView.file => 'file',
      };
}

/// Timeline 的只读滚动几何诊断。
///
/// 它只描述**实际渲染出来的**滚动几何（`CustomScrollView` 的 center 拆分、跟随状态、
/// [ScrollPosition] 的 min/max/pixels、视口高度、贴底留白与当前锚点），供驱动快照与人工
/// 验收判断“末尾是否贴底、内容是否填满视口、上翻历史有没有被抢”：
///
/// - 贴底：`extentAfter ≈ 0`；
/// - 内容比视口短（整段靠底）：`maxScrollExtent == 0 && pixels == 0 && bottomSlack > 0`；
/// - 退化几何（内容贴顶 + 下方空白）：`centerId != null && pixels >= 0` 且 `centerId`
///   起正向区高度 < `viewportDimension`；
/// - 上翻历史：`centerId != null && pixels < 0`，`anchor` 与上一帧一致；
/// - 拖动是否被状态机吞掉：`userDragUpdates` 是真实拖动累计次数、`programmaticScroll` 是
///   当前是否把滚动当成程序化滚动忽略。两者一起读即可区分“拖动到达滚动视图但位置被复位”
///   与“拖动没有到达滚动视图”，不必从截图倒推（见 `timeline_view.dart` 的
///   `_restorePendingPosition` / `_handleScrollUpdate`）。
class TimelineScrollDiagnostic {
  const TimelineScrollDiagnostic({
    required this.threadId,
    required this.centerId,
    required this.centerIndex,
    required this.rowCount,
    required this.followingBottom,
    required this.detachedByUser,
    required this.pendingNewEvents,
    required this.pixels,
    required this.minScrollExtent,
    required this.maxScrollExtent,
    required this.viewportDimension,
    required this.extentAfter,
    required this.bottomSlack,
    required this.hasNewer,
    required this.showJumpToLatest,
    required this.programmaticScroll,
    required this.userDragUpdates,
    required this.anchor,
    required this.restorePending,
    required this.restoreAnchor,
  });

  final String? threadId;

  /// `CustomScrollView.center` 当前锚定的行；为空表示正向区独占整个消息列。
  final String? centerId;

  /// [centerId] 在窗口里的下标；[centerId] 为空时为 -1。
  final int centerIndex;

  final int rowCount;
  final bool followingBottom;
  final bool detachedByUser;
  final int pendingNewEvents;
  final double? pixels;
  final double? minScrollExtent;
  final double? maxScrollExtent;
  final double? viewportDimension;
  final double? extentAfter;

  /// 贴底 sliver 最近一次布局上报的前导留白（内容比视口长时为 0）。
  final double bottomSlack;

  final bool hasNewer;
  final bool showJumpToLatest;

  /// 时间线当前是否把滚动通知当成**程序化滚动**忽略。
  ///
  /// 正常状态恒为 false；如果一次真实上翻后仍为 true，说明时间线把用户滚动当成了自己的
  /// 程序化定位（例如恢复标记没有归零），此时 `detachedByUser` 永远不会变化——这与锚点/
  /// 贴底几何无关（见 `_restorePendingPosition`）。
  final bool programmaticScroll;

  /// 真实指针拖动累计产生的滚动更新次数（单调递增，`dragDetails != null`）。
  ///
  /// 与 [programmaticScroll] 一起读，用来区分「拖动到达了滚动视图、位置被状态机复位」和
  /// 「拖动根本没有到达滚动视图」：前者该计数在拖动后增长，后者不增长。
  final int userDragUpdates;

  /// 当前可见锚点（最靠上可见行的内容偏移）。
  final TimelineAnchor? anchor;

  /// 待恢复的阅读意图是否还没被当前布局表达出来。
  ///
  /// 为 `true` 时说明目标 offset 被当前 `maxScrollExtent`（正文仍是预览）钳位，界面正等
  /// 完整正文到位后按同一身份 + offset 重新落位；此时 [anchor] 只是钳位结果，[restoreAnchor]
  /// 才是读者的原始阅读位置。
  final bool restorePending;

  /// 尚未被布局表达的恢复目标（身份 + 原始 offset）；[restorePending] 为 false 时是本次
  /// 恢复的目标或空。
  final TimelineAnchor? restoreAnchor;

  Map<String, Object?> toJson() => {
    'threadId': threadId,
    'centerId': centerId,
    'centerIndex': centerIndex,
    'rowCount': rowCount,
    'followingBottom': followingBottom,
    'detachedByUser': detachedByUser,
    'pendingNewEvents': pendingNewEvents,
    'pixels': pixels,
    'minScrollExtent': minScrollExtent,
    'maxScrollExtent': maxScrollExtent,
    'viewportDimension': viewportDimension,
    'extentAfter': extentAfter,
    'bottomSlack': bottomSlack,
    'hasNewer': hasNewer,
    'showJumpToLatest': showJumpToLatest,
    'programmaticScroll': programmaticScroll,
    'userDragUpdates': userDragUpdates,
    'restorePending': restorePending,
    'restoreAnchor': restoreAnchor == null
        ? null
        : {
            'itemId': restoreAnchor!.itemId,
            'offset': restoreAnchor!.offset,
            'followingBottom': restoreAnchor!.followingBottom,
          },
    'anchor': anchor == null
        ? null
        : {
            'itemId': anchor!.itemId,
            'offset': anchor!.offset,
            'followingBottom': anchor!.followingBottom,
          },
  };
}
