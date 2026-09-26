part of 'studio_shell.dart';

class _Header extends StatelessWidget {
  const _Header({required this.state});

  final HeaderView state;

  @override
  Widget build(BuildContext context) {
    final thread = state.selectedRootThread;
    final project = state.selectedProject;
    final projectLabel = project?.name.trim() ?? '';
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 16),
      child: LayoutBuilder(
        builder: (context, constraints) {
          final title = Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                thread?.title ?? context.l10n.shellNoSession,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: context.text.titleLarge?.copyWith(
                  fontWeight: FontWeight.w600,
                ),
              ),
              if (projectLabel.isNotEmpty) ...[
                const SizedBox(height: 4),
                Tooltip(
                  message: project?.path ?? projectLabel,
                  child: Text(
                    projectLabel,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: context.text.bodySmall?.copyWith(
                      color: context.colors.onSurfaceVariant,
                    ),
                  ),
                ),
              ],
            ],
          );
          final actions = Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              _AgentSwitcher(state: state),
              const SizedBox(width: 8),
              _SessionCostChip(cost: state.sessionCost),
              const SizedBox(width: 8),
              _SessionOverflowMenu(state: state),
            ],
          );
          if (constraints.maxWidth < 520) {
            return Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [title, if (state.workspaceThreads.isNotEmpty) actions],
            );
          }
          return Row(
            children: [
              Expanded(child: title),
              if (state.workspaceThreads.isNotEmpty) ...[
                const SizedBox(width: 16),
                actions,
              ],
            ],
          );
        },
      ),
    );
  }
}

class _SessionCostChip extends StatelessWidget {
  const _SessionCostChip({required this.cost});

  final SessionCostView? cost;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: [
        context.l10n.sessionAllAgentsCostTooltip,
        for (final item in cost?.purposeCosts ?? const <PurposeCostView>[])
          '${_purposeLabel(context, item.purpose)}: ${item.label}${item.hasUnpricedUsage ? ' (${context.l10n.statusUnpricedUsageLabel})' : ''}',
      ].join('\n'),
      child: Padding(
        key: StudioDriverKeys.sessionCost,
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
        child: Text(cost?.label ?? '-', style: context.text.labelMedium),
      ),
    );
  }
}

String _purposeLabel(BuildContext context, String? purpose) =>
    switch (purpose) {
      'main' => context.l10n.costPurposeMain,
      'summary' => context.l10n.costPurposeSummary,
      'review' => context.l10n.costPurposeReview,
      'title' => context.l10n.costPurposeTitle,
      null => context.l10n.costPurposeUnknown,
      String value => value,
    };

/// 会话顶栏「...」更多菜单；当前唯一动作是「在 VS Code 中打开工作区」。
///
/// 未探测到 VS Code 安装或当前无所属项目时整个入口不渲染（菜单保持为空，
/// 不显示空占位）。远端项目经 Remote-SSH 打开，连接参数由 `~/.ssh/config`
/// 的 Host 别名解析。
class _SessionOverflowMenu extends ConsumerWidget {
  const _SessionOverflowMenu({required this.state});

  final HeaderView state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final project = state.selectedProject;
    final thread = state.selectedRootThread;
    final availability = ref.watch(vsCodeAvailabilityProvider);
    if (project == null ||
        thread == null ||
        !availability.hasValue ||
        availability.value != true) {
      return const SizedBox.shrink();
    }
    final controller = MenuController();
    return MenuAnchor(
      controller: controller,
      alignmentOffset: const Offset(0, 6),
      menuChildren: [
        MenuItemButton(
          key: StudioDriverKeys.sessionOpenInVsCode,
          leadingIcon: const Icon(Icons.code_outlined, size: 18),
          onPressed: () {
            controller.close();
            _openInVsCode(context, ref, project, thread);
          },
          child: _SessionOpenTarget(
            label: context.l10n.sessionOpenInVsCode,
            target: thread.workspacePath,
          ),
        ),
      ],
      builder: (context, controller, child) => IconButton(
        key: StudioDriverKeys.sessionOverflow,
        tooltip: context.l10n.sessionMoreActionsTooltip,
        icon: const Icon(Icons.more_horiz, size: 20),
        onPressed: () =>
            controller.isOpen ? controller.close() : controller.open(),
      ),
    );
  }

  Future<void> _openInVsCode(
    BuildContext context,
    WidgetRef ref,
    StudioProject project,
    StudioThread thread,
  ) async {
    final messenger = ScaffoldMessenger.maybeOf(context);
    final l10n = context.l10n;
    String? uri;
    String? failure;
    if (project.sshAlias case final alias?) {
      try {
        final servers = await ref.read(studioApiProvider).listSshServers();
        final server = servers
            .where((server) => server.alias == alias)
            .firstOrNull;
        if (server == null) {
          failure = l10n.sessionVsCodeServerMissing;
        } else {
          uri = buildRemoteVsCodeFolderUri(
            alias: server.alias,
            remotePath: thread.workspacePath,
          );
        }
      } on Object {
        failure = l10n.sessionVsCodeOpenFailed;
      }
    } else {
      uri = buildLocalVsCodeFolderUri(thread.workspacePath);
    }
    if (uri != null) {
      final launcher = ref.read(vsCodeLauncherProvider);
      try {
        await launcher(uri);
      } on Object {
        failure = l10n.sessionVsCodeOpenFailed;
      }
    }
    if (failure != null && messenger != null) {
      messenger.showSnackBar(SnackBar(content: Text(failure)));
    }
  }
}

/// 会话「在 VS Code 中打开工作区」菜单项：主标签下方以辅助行展示打开目标
/// （会话工作区地址）。长路径截断为单行，完整内容可由悬停与读屏获取，辅助行
/// 不改变打开目标语义。
class _SessionOpenTarget extends StatelessWidget {
  const _SessionOpenTarget({required this.label, required this.target});

  final String label;
  final String target;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: target,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 280),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(label),
            const SizedBox(height: 2),
            Text(
              target,
              key: StudioDriverKeys.sessionOpenTarget,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: context.text.bodySmall?.copyWith(
                color: context.colors.onSurfaceVariant,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _AgentSwitcher extends ConsumerStatefulWidget {
  const _AgentSwitcher({required this.state});

  final HeaderView state;

  @override
  ConsumerState<_AgentSwitcher> createState() => _AgentSwitcherState();
}

class _AgentSwitcherState extends ConsumerState<_AgentSwitcher> {
  final MenuController _menuController = MenuController();
  Timer? _hoverTimer;

  @override
  void dispose() {
    _hoverTimer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final threads = _orderedAgentThreads(widget.state.workspaceThreads);
    final aggregateColor = _aggregateAgentColor(context, widget.state, threads);
    final viewport = MediaQuery.sizeOf(context);
    final availableWidth = (viewport.width - 24)
        .clamp(0.0, double.infinity)
        .toDouble();
    final minimumWidth = availableWidth < 240 ? availableWidth : 240.0;
    final menuWidth = (viewport.width * 0.36)
        .clamp(minimumWidth, availableWidth)
        .toDouble();
    // Reserve the compact header and overlay insets so a long menu can stay
    // below its anchor and scroll instead of being flipped over the header.
    // 高度上限让菜单只占用有限纵向空间，其余条目滚动查看。
    final menuHeight = (viewport.height - 96)
        .clamp(0.0, _agentMenuMaxHeight)
        .toDouble();
    final contentWidth = (menuWidth - 116).clamp(140.0, 244.0).toDouble();
    return MenuAnchor(
      controller: _menuController,
      style: MenuStyle(
        alignment: AlignmentDirectional.bottomEnd,
        minimumSize: WidgetStatePropertyAll(Size(menuWidth, 0)),
        maximumSize: WidgetStatePropertyAll(Size(menuWidth, menuHeight)),
      ),
      alignmentOffset: const Offset(0, 6),
      menuChildren: [
        for (final thread in threads)
          MenuItemButton(
            key: StudioDriverKeys.agentRow(thread.id),
            leadingIcon: Padding(
              padding: EdgeInsets.only(left: _agentDepth(thread, threads) * 12),
              child: Icon(
                _agentForThread(widget.state, thread.id)?.error != null
                    ? Icons.error_outline
                    : Icons.circle,
                size: _agentForThread(widget.state, thread.id)?.error != null
                    ? 16
                    : 9,
                color: _agentForThread(widget.state, thread.id)?.error != null
                    ? Theme.of(context).colorScheme.error
                    : _statusColor(context, widget.state, thread),
              ),
            ),
            trailingIcon: thread.id == widget.state.selectedThreadId
                ? const Icon(Icons.check, size: 18)
                : null,
            onPressed: () {
              _menuController.close();
              ref
                  .read(studioControllerProvider.notifier)
                  .selectAgentThread(thread.id);
            },
            child: SizedBox(
              width: contentWidth,
              child: Row(
                children: [
                  Expanded(
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Tooltip(
                          message: _agentDisplayName(context, thread),
                          child: Text(
                            _agentDisplayName(context, thread),
                            key: StudioDriverKeys.agentTaskSummary(thread.id),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                        if (thread.role.trim().isNotEmpty)
                          Text(
                            thread.isRetiredAgent
                                ? context.l10n.agentRoleRetired
                                : context.roleLabel(thread.role),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: Theme.of(context).textTheme.labelSmall
                                ?.copyWith(
                                  color: context.colors.onSurfaceVariant,
                                ),
                          ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 12),
                  ConstrainedBox(
                    // 长状态或错误文本必须行内省略，不能撑破固定宽度的菜单行。
                    constraints: BoxConstraints(
                      maxWidth: contentWidth * _agentStatusWidthFraction,
                    ),
                    child: Text(
                      _agentForThread(widget.state, thread.id)?.error ??
                          _agentShortStatus(context, thread),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: Theme.of(context).textTheme.labelSmall
                          ?.copyWith(color: context.colors.onSurfaceVariant),
                    ),
                  ),
                ],
              ),
            ),
          ),
      ],
      builder: (context, controller, child) {
        return MouseRegion(
          onEnter: (_) {
            _hoverTimer?.cancel();
            _hoverTimer = Timer(const Duration(milliseconds: 250), () {
              if (mounted && !_menuController.isOpen) {
                _menuController.open();
              }
            });
          },
          onExit: (_) => _hoverTimer?.cancel(),
          child: Focus(
            onFocusChange: (focused) {
              if (focused && !_menuController.isOpen) {
                _menuController.open();
              }
            },
            child: TextButton(
              key: StudioDriverKeys.agentSwitcher,
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(Icons.circle, size: 6, color: aggregateColor),
                  const SizedBox(width: 8),
                  Text(context.l10n.statusAgentsCount(threads.length)),
                  const SizedBox(width: 4),
                  const Icon(Icons.keyboard_arrow_down, size: 14),
                ],
              ),
              onPressed: () => _menuController.isOpen
                  ? _menuController.close()
                  : _menuController.open(),
            ),
          ),
        );
      },
    );
  }
}

/// `n agents` 菜单最多占用的高度，超出部分滚动查看。
const double _agentMenuMaxHeight = 320;

/// 菜单行内状态标签最多占用的行宽比例，超出部分行内省略。
const double _agentStatusWidthFraction = 0.45;

/// 运行状态展示分组：执行中、失败、空闲、关闭中或已关闭。
int _agentStatusPriority(ThreadStatusView status) => switch (status) {
  // 执行中，与 ThreadStatusView.isActive 保持一致。
  ThreadStatusView.queued ||
  ThreadStatusView.running ||
  ThreadStatusView.waitingTool ||
  ThreadStatusView.waitingInteraction ||
  ThreadStatusView.cancelling => 0,
  ThreadStatusView.faulted => 1,
  ThreadStatusView.idle => 2,
  ThreadStatusView.closing || ThreadStatusView.closed => 3,
};

/// 按运行状态分组稳定排序，同组内保持 canonical 的 owner/父子顺序。
List<StudioThread> _orderedAgentThreads(List<StudioThread> threads) {
  final indexed = [
    for (var index = 0; index < threads.length; index++)
      (index, threads[index]),
  ];
  indexed.sort((left, right) {
    final byStatus = _agentStatusPriority(left.$2.status)
        .compareTo(_agentStatusPriority(right.$2.status));
    return byStatus != 0 ? byStatus : left.$1.compareTo(right.$1);
  });
  return [for (final entry in indexed) entry.$2];
}

StudioAgentView? _agentForThread(HeaderView state, String threadId) {
  for (final agent in state.agents) {
    if (agent.threadId == threadId) return agent;
  }
  return null;
}

int _agentDepth(StudioThread thread, List<StudioThread> threads) {
  var depth = 0;
  var parentId = thread.parentThreadId;
  final visited = <String>{thread.id};
  while (parentId != null && visited.add(parentId)) {
    final parent = threads
        .where((candidate) => candidate.id == parentId)
        .firstOrNull;
    if (parent == null) {
      break;
    }
    depth += 1;
    parentId = parent.parentThreadId;
  }
  return depth;
}

Color _aggregateAgentColor(
  BuildContext context,
  HeaderView state,
  List<StudioThread> threads,
) {
  if (threads.any((thread) => _isFaultedAgentStatus(thread.status))) {
    return context.colors.error;
  }
  if (state.pendingInteractions.any(
    (interaction) => threads.any((thread) => thread.id == interaction.threadId),
  )) {
    return context.statusColors.warning;
  }
  if (threads.any((thread) => _isRunningAgentStatus(thread.status))) {
    return context.statusColors.activeIndicator;
  }
  return context.statusColors.success;
}

Color _statusColor(
  BuildContext context,
  HeaderView state,
  StudioThread thread,
) {
  if (_isFaultedAgentStatus(thread.status)) {
    return context.colors.error;
  }
  if (state.pendingInteractions.any(
    (interaction) => interaction.threadId == thread.id,
  )) {
    return context.statusColors.warning;
  }
  if (_isRunningAgentStatus(thread.status)) {
    return context.statusColors.activeIndicator;
  }
  return context.statusColors.success;
}

bool _isRunningAgentStatus(ThreadStatusView status) => status.isActive;

bool _isFaultedAgentStatus(ThreadStatusView status) =>
    status == ThreadStatusView.faulted;

String _agentShortStatus(BuildContext context, StudioThread thread) =>
    context.threadStatusLabel(thread.status);

String _agentDisplayName(BuildContext context, StudioThread thread) {
  if (!thread.isRoot && thread.title.trim().isNotEmpty) {
    return thread.title.trim();
  }
  if (thread.isRetiredAgent) return thread.id;
  final role = thread.role.trim();
  if (role.isEmpty) {
    return thread.isRoot ? context.l10n.roleEmpty : thread.id;
  }
  return context.roleLabel(role);
}

String _threadSubtitle(
  BuildContext context,
  StudioThread thread,
  String? modeDisplayName,
) {
  final status = switch (thread.status) {
    ThreadStatusView.idle => context.l10n.settingsLspActivityIdle,
    ThreadStatusView.queued => context.l10n.agentDetailStatusQueued,
    ThreadStatusView.running ||
    ThreadStatusView.waitingTool => context.l10n.sidebarRunning,
    ThreadStatusView.waitingInteraction => context.l10n.sidebarAttention,
    ThreadStatusView.cancelling => context.l10n.agentDetailStatusInterrupted,
    ThreadStatusView.closing => context.l10n.agentDetailStatusClosing,
    ThreadStatusView.closed => context.l10n.agentDetailStatusShutdown,
    ThreadStatusView.faulted => context.l10n.agentDetailStatusErrored,
  };
  final hour = thread.updatedAt.hour.toString().padLeft(2, '0');
  final minute = thread.updatedAt.minute.toString().padLeft(2, '0');
  final date = '${thread.updatedAt.month}/${thread.updatedAt.day}';
  return '$status · $date $hour:$minute';
}

class _Footer extends StatelessWidget {
  const _Footer({
    required this.threadId,
    required this.showTodo,
    required this.todoExpanded,
    required this.onToggleTodo,
    required this.compact,
    this.contentKey,
  });

  final String threadId;
  final bool showTodo;
  final bool todoExpanded;
  final VoidCallback? onToggleTodo;

  /// 矮窗口紧凑布局：只收缩 composer 的留白与输入行数，不隐藏任何操作。
  final bool compact;

  /// footer 区域的可定位句柄（供驱动按区域定位）。
  final Key? contentKey;

  @override
  Widget build(BuildContext context) {
    final footer = DecoratedBox(
      decoration: BoxDecoration(color: context.colors.surface),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          // 固定在输入区上方：滚历史时依旧可见，且与 Timeline 解耦。
          //
          // 活动条是整条 footer 里**唯一**可伸缩的部分：`Flexible` 把输入区与状态栏
          // 布局之后的实际剩余高度交给它，因此展开详情只能压缩自己，不可能把输入区
          // 顶出窗口。输入区与状态栏保持自身固有高度，始终完整可见。
          //
          // footer 的总预算由 [AgentWorkspacePane] 按「窗口高度 − 时间线最小可视高度」
          // 给出；矮窗口时 [compact] 只收起 composer 留白与输入行数，让展开详情保留可读
          // 高度，同时不隐藏任何操作按钮。
          Flexible(
            fit: FlexFit.loose,
            child: _ConversationActivityHost(threadId: threadId),
          ),
          _ComposerHost(compact: compact),
          _StatusBarHost(
            showTodo: showTodo,
            todoExpanded: todoExpanded,
            onToggleTodo: onToggleTodo,
          ),
        ],
      ),
    );
    return contentKey == null
        ? footer
        : KeyedSubtree(key: contentKey, child: footer);
  }
}

/// 固定活动条的接线点。
///
/// 事实来源是后端 typed 活动投影（`conversationActivityProvider`）：
/// - 阶段/摘要/并行工具：`ThreadWorkspace.activity`（快照与 `ActivityChanged` 通知）；
/// - 等待审批/输入：当前选中会话的待处理交互；
/// - 保存故障/暂停：`ThreadWorkspace.storage`（typed，不从错误字符串解析）；
/// - 详情：用户展开时由 controller 按活动身份**按需**读取，独立于消息窗口。
class _ConversationActivityHost extends ConsumerWidget {
  const _ConversationActivityHost({required this.threadId});

  final String threadId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final view = ref.watch(conversationActivityProvider(threadId)).value;
    if (view == null) {
      StudioDriverState.publishConversationActivity(null);
      return const SizedBox.shrink();
    }
    StudioDriverState.publishConversationActivity(view);
    return ConversationActivityBar(
      view: view,
      onExpand: () => ref
          .read(studioControllerProvider.notifier)
          .expandActivityDetail(threadId),
      onCollapse: () => ref
          .read(studioControllerProvider.notifier)
          .collapseActivityDetail(threadId),
      // Driver 可观察的是“实际展开态”，不是 `expandable` 的“可展开能力”。
      onExpandedChanged: StudioDriverState.publishActivityExpanded,
    );
  }
}

class _StatusBarHost extends ConsumerWidget {
  const _StatusBarHost({
    required this.showTodo,
    required this.todoExpanded,
    required this.onToggleTodo,
  });

  final bool showTodo;
  final bool todoExpanded;
  final VoidCallback? onToggleTodo;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final asyncStatus = ref.watch(statusBarProvider);
    return asyncStatus.when(
      loading: () => const SizedBox.shrink(),
      error: (error, stackTrace) => const SizedBox.shrink(),
      data: (status) {
        if (status == null) {
          return const SizedBox.shrink();
        }
        return ThreadStatusBar(
          view: status,
          showTodo: showTodo,
          todoExpanded: todoExpanded,
          onToggleTodo: onToggleTodo,
        );
      },
    );
  }
}

class _ComposerHost extends ConsumerWidget {
  const _ComposerHost({required this.compact});

  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final asyncWorkspace = ref.watch(selectedWorkspaceControlsProvider);
    return asyncWorkspace.when(
      loading: () => const SizedBox.shrink(),
      error: (error, stackTrace) => const SizedBox.shrink(),
      data: (workspace) {
        if (workspace == null) {
          return const SizedBox.shrink();
        }
        return ComposerDock(workspace: workspace, compact: compact);
      },
    );
  }
}
