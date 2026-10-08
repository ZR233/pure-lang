part of 'studio_shell.dart';

/// 侧栏项目行：折叠箭头、文件夹、项目名、真实本地/SSH 标识、常驻新建
/// 与更多菜单。完整路径不再常驻占用辅助行，悬停或聚焦项目名时在浮层
/// 详情中查看与复制；存在 recovery issue 时诊断提示优先。
class _ProjectTile extends ConsumerWidget {
  const _ProjectTile({
    required this.project,
    required this.selected,
    required this.expanded,
    required this.onToggle,
    this.onNavigate,
    required this.recoveryIssue,
    this.touchMode = false,
  });
  final StudioProject project;
  final bool selected;
  final bool expanded;
  final VoidCallback onToggle;
  final VoidCallback? onNavigate;
  final StudioRecoveryIssue? recoveryIssue;

  /// 触控输入模式：名称命中区按触控密度放大，与 `_SidebarState` 的
  /// 全侧栏输入模式一致。
  final bool touchMode;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final controller = ref.read(studioControllerProvider.notifier);
    final issue = recoveryIssue;
    if (issue != null) {
      // 诊断提示优先：存在 recovery issue 时只保留名称区 Tooltip，不展示详情卡。
      return _buildRow(context, ref, controller, nameTooltip: issue.detail);
    }
    // 详情宿主锚定**整行**：卡片贴在完整项目行边界外侧，不再覆盖同项目的
    // 新建/更多按钮和相邻会话行的状态/操作槽；名称 InkWell 复用宿主传入的
    // 同一 focusNode，仍是唯一键盘选择入口，不新增 Focus/Tab 层。
    return _SidebarHoverDetail(
      detailBuilder: (_) => _projectDetailCard(context),
      childBuilder: (context, nameFocus, onMenuOpenChanged, onMenuBeforeOpen) =>
          _buildRow(
            context,
            ref,
            controller,
            nameFocus: nameFocus,
            onMenuOpenChanged: onMenuOpenChanged,
            onMenuBeforeOpen: onMenuBeforeOpen,
          ),
    );
  }

  /// 构建整行（折叠箭头、名称、常驻新建与更多菜单）。详情宿主/诊断 Tooltip
  /// 由调用方决定，本方法只返回原有的快捷键、`skipTraversal` 焦点容器与
  /// `projectRow` 键，保持全部行内语义不变。
  Widget _buildRow(
    BuildContext context,
    WidgetRef ref,
    StudioController controller, {
    FocusNode? nameFocus,
    ValueChanged<bool>? onMenuOpenChanged,
    VoidCallback? onMenuBeforeOpen,
    String? nameTooltip,
  }) {
    final nameArea = _buildProjectNameArea(
      context,
      controller,
      focusNode: nameFocus,
    );
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.arrowRight): () {
          if (!expanded) onToggle();
        },
        const SingleActivator(LogicalKeyboardKey.arrowLeft): () {
          if (expanded) onToggle();
        },
      },
      child: Focus(
        skipTraversal: true,
        child: KeyedSubtree(
          key: StudioDriverKeys.projectRow(project.id),
          child: Row(
            children: [
              _tileIconButton(
                key: StudioDriverKeys.projectExpand(project.id),
                tooltip: context.l10n.sidebarProjects,
                icon: expanded ? Icons.expand_more : Icons.chevron_right,
                iconSize: 18,
                minTarget: touchMode ? 44 : 32,
                onPressed: onToggle,
              ),
              Expanded(
                child: nameTooltip == null
                    ? nameArea
                    : Tooltip(message: nameTooltip, child: nameArea),
              ),
              _tileIconButton(
                key: selected
                    ? StudioDriverKeys.newSession
                    : ValueKey('project-new-session-${project.id}'),
                tooltip: '${context.l10n.sidebarNewSession} · ${project.name}',
                icon: Icons.add,
                iconSize: 18,
                minTarget: touchMode ? 44 : 32,
                onPressed: recoveryIssue == null
                    ? () async {
                        await controller.selectProject(project.id);
                        if (!context.mounted) return;
                        await controller.beginNewThread();
                        onNavigate?.call();
                      }
                    : null,
              ),
              SizedBox.square(
                dimension: touchMode ? 44 : 32,
                child: StudioIconMenu<String>(
                  key: StudioDriverKeys.projectMenu(project.id),
                  tooltip: context.l10n.sidebarProjects,
                  icon: const Icon(Icons.more_horiz, size: 18),
                  padding: EdgeInsets.zero,
                  visualDensity: VisualDensity.standard,
                  // 菜单建立前同步收起详情宿主，避免子菜单被详情卡遮挡；
                  // onOpen/onClose 只维护暂停重开 flag，不再在 onOpen 里关父面。
                  onBeforeOpen: onMenuBeforeOpen,
                  onOpen: () => onMenuOpenChanged?.call(true),
                  onClose: () => onMenuOpenChanged?.call(false),
                  onSelected: (action) async {
                    if (action == 'pin') {
                      await _saveSidebarPreferences(ref, projectId: project.id);
                    }
                    if (action == 'copy') {
                      await Clipboard.setData(
                        ClipboardData(text: project.path),
                      );
                    }
                    if (action == 'close') {
                      await controller.archiveProject(project.id);
                    }
                  },
                  itemBuilder: (_) => [
                    StudioMenuItem<String>(
                      value: 'pin',
                      child: Text(
                        ref
                                    .watch(studioControllerProvider)
                                    .value
                                    ?.general
                                    .pinnedProjectIds
                                    .contains(project.id) ==
                                true
                            ? context.l10n.sidebarUnpin
                            : context.l10n.sidebarPin,
                      ),
                    ),
                    StudioMenuItem<String>(
                      value: 'copy',
                      child: Text(context.l10n.sidebarCopyPath),
                    ),
                    StudioMenuItem<String>(
                      value: 'close',
                      itemKey: ValueKey('project-close-${project.id}'),
                      child: Text(context.l10n.sidebarCloseProject),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildProjectNameArea(
    BuildContext context,
    StudioController controller, {
    FocusNode? focusNode,
  }) {
    final issue = recoveryIssue;
    return InkWell(
      focusNode: focusNode,
      onTap: issue == null
          ? () async {
              await controller.selectProject(project.id);
              onNavigate?.call();
            }
          : null,
      child: Padding(
        padding: EdgeInsets.symmetric(vertical: touchMode ? 12 : 7),
        child: Row(
          children: [
            Icon(
              issue == null ? Icons.folder_outlined : Icons.error_outline,
              size: 17,
              color: issue == null ? null : context.colors.error,
            ),
            const SizedBox(width: 6),
            Flexible(
              child: Text(
                project.name,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: context.text.labelLarge?.copyWith(
                  fontWeight: selected ? FontWeight.w700 : FontWeight.w600,
                ),
              ),
            ),
            const SizedBox(width: 6),
            Text(
              project.sshAlias == null ? context.l10n.sidebarLocal : 'SSH',
              style: context.text.labelSmall,
            ),
          ],
        ),
      ),
    );
  }

  Widget _projectDetailCard(BuildContext context) {
    final alias = project.sshAlias;
    final environment = alias == null
        ? context.l10n.sidebarLocal
        : 'SSH · $alias';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        // 详情卡内部可滚动，完整展示 canonical 项目名，不设行数上限；
        // 截断只发生在行内单行渲染。
        Text(
          project.name,
          style: context.text.labelLarge?.copyWith(fontWeight: FontWeight.w600),
        ),
        const SizedBox(height: 8),
        _SidebarDetailRow(
          leading: Icon(
            alias == null ? Icons.computer_outlined : Icons.dns_outlined,
            size: 15,
            color: context.colors.onSurfaceVariant,
          ),
          value: environment,
        ),
        _SidebarDetailRow(
          leading: Icon(
            Icons.folder_open_outlined,
            size: 15,
            color: context.colors.onSurfaceVariant,
          ),
          value: project.path,
          trailing: _detailCopyButton(
            context,
            project.path,
            touchMode ? 44 : 32,
          ),
        ),
      ],
    );
  }
}

/// 侧栏会话行：紧凑单行标题，右侧固定状态区（会话工作树标识与行级
/// canonical 运行状态），悬停或聚焦时出现置顶、归档快捷操作与更多
/// 菜单，并展示完整标题、项目、环境、会话工作区路径与状态时间的悬浮
/// 详情。每行的状态只读自己的 [StudioThread.status]。
class _ThreadTile extends ConsumerWidget {
  const _ThreadTile({
    required this.thread,
    required this.project,
    required this.selected,
    required this.recoveryIssue,
    this.onNavigate,
    this.touchMode = false,
  });

  final StudioThread thread;
  final StudioProject project;
  final bool selected;
  final StudioRecoveryIssue? recoveryIssue;
  final VoidCallback? onNavigate;

  /// 触控输入模式：快捷操作常显，命中区放大到 44px；由 `_SidebarState`
  /// 按最近指针输入对整个侧栏统一判定。
  final bool touchMode;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final issue = recoveryIssue;
    final blocked = issue?.blocksAccess ?? false;
    final pinned = ref.watch(
      studioControllerProvider.select(
        (state) =>
            state.value?.general.pinnedThreadIds.contains(thread.id) ?? false,
      ),
    );
    final vsCodeAvailable =
        ref.watch(vsCodeAvailabilityProvider).value ?? false;
    final zedAvailable = ref.watch(zedAvailabilityProvider).value ?? false;
    final terminalAvailable =
        ref.watch(terminalAvailabilityProvider).value ?? false;
    // 状态区使用固定宽度槽位（工作区标识槽 + 状态槽），工作树有无、
    // 空闲与运行之间的切换都不改变标题可用宽度。
    final statusArea = Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        SizedBox.square(
          dimension: 16,
          child: Center(
            child: issue != null
                ? Icon(
                    Icons.error_outline,
                    size: 15,
                    color: context.colors.error,
                  )
                : thread.workspaceMode.isWorktree
                ? Tooltip(
                    message: context.l10n.sidebarSessionWorktree,
                    child: Icon(
                      Icons.account_tree_outlined,
                      key: StudioDriverKeys.threadWorkspaceMode(thread.id),
                      size: 14,
                    ),
                  )
                : null,
          ),
        ),
        SizedBox.square(
          dimension: 16,
          child: Center(child: _ThreadStatusBadge(status: thread.status)),
        ),
      ],
    );
    Widget tile({
      FocusNode? focusNode,
      ValueChanged<bool>? onMenuOpenChanged,
      VoidCallback? onMenuBeforeOpen,
    }) => _SidebarTile(
      selected: selected,
      title: thread.title,
      onTap: !blocked
          ? () async {
              await ref
                  .read(studioControllerProvider.notifier)
                  .selectThread(thread.id);
              onNavigate?.call();
            }
          : null,
      trailing: statusArea,
      touchMode: touchMode,
      focusNode: focusNode,
      actions: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          _tileIconButton(
            key: StudioDriverKeys.pinThread(thread.id),
            tooltip: pinned
                ? context.l10n.sidebarUnpin
                : context.l10n.sidebarPin,
            icon: pinned ? Icons.push_pin : Icons.push_pin_outlined,
            minTarget: touchMode ? 44 : 32,
            onPressed: () async {
              await _saveSidebarPreferences(ref, threadId: thread.id);
            },
          ),
          _tileIconButton(
            key: StudioDriverKeys.archiveThread(thread.id),
            tooltip: context.l10n.sidebarArchiveSession,
            icon: Icons.archive_outlined,
            minTarget: touchMode ? 44 : 32,
            onPressed: !blocked
                ? () => _archiveThreadFromSidebar(context, ref, thread.id)
                : null,
          ),
          _threadMenu(
            context,
            ref,
            issue,
            pinned: pinned,
            blocked: blocked,
            vsCodeAvailable: vsCodeAvailable,
            zedAvailable: zedAvailable,
            terminalAvailable: terminalAvailable,
            minTarget: touchMode ? 44 : 32,
            onOpenChanged: onMenuOpenChanged,
            onBeforeOpen: onMenuBeforeOpen,
          ),
        ],
      ),
    );
    return KeyedSubtree(
      key: StudioDriverKeys.threadRow(thread.id),
      child: issue == null
          ? _SidebarHoverDetail(
              detailBuilder: (detailContext) =>
                  _threadDetailCard(detailContext),
              childBuilder:
                  (context, focusNode, onMenuOpenChanged, onMenuBeforeOpen) =>
                      tile(
                        focusNode: focusNode,
                        onMenuOpenChanged: onMenuOpenChanged,
                        onMenuBeforeOpen: onMenuBeforeOpen,
                      ),
            )
          : Tooltip(message: issue.detail, child: tile()),
    );
  }

  /// 会话更多菜单：置顶、重命名、归档、复制会话工作区路径，以及会话级
  /// 打开入口（VS Code、Zed、终端；不可用时禁用并说明原因），并保留
  /// recovery issue 的重试入口。复用统一的 [StudioMenu] 锚定与键盘/焦点
  /// 规则，与页眉菜单共用同一 open 实现，不新增后端能力。
  Widget _threadMenu(
    BuildContext context,
    WidgetRef ref,
    StudioRecoveryIssue? issue, {
    required bool pinned,
    required bool blocked,
    required bool vsCodeAvailable,
    required bool zedAvailable,
    required bool terminalAvailable,
    required double minTarget,
    ValueChanged<bool>? onOpenChanged,
    VoidCallback? onBeforeOpen,
  }) {
    final alias = project.sshAlias;
    final targetDescription = alias == null
        ? thread.workspacePath
        : '$alias:${thread.workspacePath}';
    return SizedBox.square(
      dimension: minTarget,
      child: StudioIconMenu<String>(
        key: StudioDriverKeys.threadMenu(thread.id),
        icon: const Icon(Icons.more_horiz, size: 18),
        padding: EdgeInsets.zero,
        visualDensity: VisualDensity.standard,
        onBeforeOpen: onBeforeOpen,
        onOpen: () => onOpenChanged?.call(true),
        onClose: () => onOpenChanged?.call(false),
        onSelected: (action) async {
          if (action == 'retry') {
            await ref.read(studioControllerProvider.notifier).retryRecovery();
          }
          if (action == 'pin') {
            await _saveSidebarPreferences(ref, threadId: thread.id);
          }
          if (action == 'rename' && context.mounted) {
            await _renameThreadFromSidebar(context, ref, thread);
          }
          if (action == 'archive' && context.mounted) {
            await _archiveThreadFromSidebar(context, ref, thread.id);
          }
          if (action == 'copy-workspace') {
            await Clipboard.setData(ClipboardData(text: thread.workspacePath));
          }
          if (action == 'open-vscode' && context.mounted) {
            await _openThreadWorkspaceInVsCode(context, ref, project, thread);
          }
          if (action == 'open-zed' && context.mounted) {
            await _openThreadWorkspaceInZed(context, ref, project, thread);
          }
          if (action == 'open-terminal' && context.mounted) {
            await _openThreadWorkspaceInTerminal(context, ref, project, thread);
          }
        },
        itemBuilder: (_) => [
          if (issue?.canRetry == true)
            StudioMenuItem<String>(
              value: 'retry',
              itemKey: StudioDriverKeys.retryRecoveryIssue(issue!.id),
              child: Text(context.l10n.runtimeFatalRetry),
            ),
          StudioMenuItem<String>(
            value: 'pin',
            child: Text(
              pinned ? context.l10n.sidebarUnpin : context.l10n.sidebarPin,
            ),
          ),
          StudioMenuItem<String>(
            value: 'rename',
            itemKey: StudioDriverKeys.renameThread(thread.id),
            enabled: !blocked,
            child: Text(context.l10n.sidebarRenameSession),
          ),
          StudioMenuItem<String>(
            value: 'archive',
            // 菜单内的归档入口使用独立 literal key：行内常驻归档按钮复用
            // StudioDriverKeys.archiveThread（唯一随行存在的 Driver 标识），
            // 两者同时可见时不会让 Driver 唯一 finder 失效。
            itemKey: ValueKey('thread-menu-archive-${thread.id}'),
            enabled: !blocked,
            child: Text(context.l10n.sidebarArchiveSession),
          ),
          StudioMenuItem<String>(
            value: 'copy-workspace',
            itemKey: StudioDriverKeys.copyThreadWorkspace(thread.id),
            child: Text(context.l10n.sidebarCopyPath),
          ),
          StudioMenuItem<String>.header(
            itemKey: ValueKey('thread-menu-workspace-${thread.id}'),
            child: const Divider(height: 1),
          ),
          _workspaceMenuItem(
            context,
            key: StudioDriverKeys.openThreadVsCode(thread.id),
            value: 'open-vscode',
            icon: Icons.code,
            label: context.l10n.sessionOpenInVsCode,
            targetDescription: targetDescription,
            available: vsCodeAvailable,
            unavailableReason: context.l10n.sessionVsCodeUnavailable,
          ),
          _workspaceMenuItem(
            context,
            key: ValueKey('thread-menu-open-zed-${thread.id}'),
            value: 'open-zed',
            icon: Icons.code,
            label: context.l10n.sessionOpenInZed,
            targetDescription: targetDescription,
            available: zedAvailable,
            unavailableReason: context.l10n.sessionZedUnavailable,
          ),
          _workspaceMenuItem(
            context,
            key: StudioDriverKeys.openThreadTerminal(thread.id),
            value: 'open-terminal',
            icon: Icons.terminal,
            label: context.l10n.sessionOpenInTerminal,
            targetDescription: targetDescription,
            available: terminalAvailable,
            unavailableReason: _terminalUnavailableReason(context),
          ),
        ],
      ),
    );
  }

  Widget _threadDetailCard(BuildContext context) {
    final alias = project.sshAlias;
    final environment = alias == null
        ? context.l10n.sidebarLocal
        : 'SSH · $alias';
    final statusLabel = context.threadStatusLabel(thread.status);
    final updated = _relativeUpdatedLabel(context, thread.updatedAt);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        // 详情卡内部可滚动，完整展示 canonical 标题（含 80 字 CJK 长标题）；
        // 截断只发生在行内单行渲染。
        Text(
          thread.title,
          style: context.text.labelLarge?.copyWith(fontWeight: FontWeight.w600),
        ),
        const SizedBox(height: 8),
        _SidebarDetailRow(
          leading: Icon(
            Icons.folder_outlined,
            size: 15,
            color: context.colors.onSurfaceVariant,
          ),
          value: project.name,
        ),
        _SidebarDetailRow(
          leading: Icon(
            alias == null ? Icons.computer_outlined : Icons.dns_outlined,
            size: 15,
            color: context.colors.onSurfaceVariant,
          ),
          value: environment,
        ),
        _SidebarDetailRow(
          leading: Icon(
            Icons.folder_open_outlined,
            size: 15,
            color: context.colors.onSurfaceVariant,
          ),
          value: thread.workspacePath,
          trailing: _detailCopyButton(
            context,
            thread.workspacePath,
            touchMode ? 44 : 32,
          ),
        ),
        _SidebarDetailRow(
          leading: DecoratedBox(
            decoration: BoxDecoration(
              color: _statusDotColor(context),
              shape: BoxShape.circle,
            ),
            child: const SizedBox.square(dimension: 8),
          ),
          value: '$statusLabel · $updated',
        ),
      ],
    );
  }

  Color _statusDotColor(BuildContext context) => switch (thread.status) {
    ThreadStatusView.faulted => context.colors.error,
    ThreadStatusView.waitingInteraction => context.statusColors.warning,
    ThreadStatusView.queued ||
    ThreadStatusView.running ||
    ThreadStatusView.waitingTool ||
    ThreadStatusView.cancelling ||
    ThreadStatusView.closing => context.statusColors.activeIndicator,
    ThreadStatusView.idle || ThreadStatusView.closed =>
      context.colors.onSurfaceVariant.withValues(alpha: 0.6),
  };
}

/// 侧栏行内快捷操作按钮：桌面 32px、触控 44px 真实命中区。
///
/// 全局主题是 `VisualDensity.compact`，这里显式 standard 防止 constraints
/// 被每边减 4（compact 会把 44 缩到 36/40、32 缩到 24）；shrinkWrap 不
/// 膨胀到默认 48，保持单行密度与标题宽度稳定。
Widget _tileIconButton({
  Key? key,
  required String tooltip,
  required IconData icon,
  double iconSize = 16,
  double minTarget = 32,
  VoidCallback? onPressed,
}) {
  return IconButton(
    key: key,
    tooltip: tooltip,
    onPressed: onPressed,
    icon: Icon(icon, size: iconSize),
    visualDensity: VisualDensity.standard,
    style: const ButtonStyle(tapTargetSize: MaterialTapTargetSize.shrinkWrap),
    padding: EdgeInsets.zero,
    constraints: BoxConstraints(minWidth: minTarget, minHeight: minTarget),
  );
}

/// 详情卡中的复制路径按钮，与项目菜单的复制动作一致；桌面 32px、
/// 触控 44px 真实命中区，IconButton 自身可聚焦、Enter/Space 可触发。
Widget _detailCopyButton(BuildContext context, String value, double minTarget) {
  return IconButton(
    tooltip: context.l10n.sidebarCopyPath,
    icon: const Icon(Icons.copy, size: 16),
    visualDensity: VisualDensity.standard,
    style: const ButtonStyle(tapTargetSize: MaterialTapTargetSize.shrinkWrap),
    padding: EdgeInsets.zero,
    constraints: BoxConstraints(minWidth: minTarget, minHeight: minTarget),
    onPressed: () => Clipboard.setData(ClipboardData(text: value)),
  );
}

/// 侧栏会话菜单中的会话级打开条目；不可用时禁用并说明原因，
/// 悬停与读屏可获取完整打开目标。
StudioMenuItem<String> _workspaceMenuItem(
  BuildContext context, {
  required Key key,
  required String value,
  required IconData icon,
  required String label,
  required String targetDescription,
  required bool available,
  required String unavailableReason,
}) {
  final message = available
      ? '$label\n$targetDescription'
      : '$label\n$targetDescription\n$unavailableReason';
  return StudioMenuItem<String>(
    value: value,
    itemKey: key,
    enabled: available,
    tooltip: message,
    child: Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(icon, size: 18),
        const SizedBox(width: 10),
        Flexible(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(label, maxLines: 1, overflow: TextOverflow.ellipsis),
              Text(
                available ? targetDescription : unavailableReason,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: context.text.labelSmall?.copyWith(
                  color: available
                      ? context.colors.onSurfaceVariant
                      : context.colors.error,
                ),
              ),
            ],
          ),
        ),
      ],
    ),
  );
}

Future<void> _archiveThreadFromSidebar(
  BuildContext context,
  WidgetRef ref,
  String threadId,
) async {
  // 归档是破坏性动作：确认对话框是唯一入口，取消不产生任何副作用。
  final confirmed = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(context.l10n.sidebarArchiveSessionConfirmTitle),
      content: Text(context.l10n.sidebarArchiveSessionConfirmBody),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(false),
          child: Text(context.l10n.commonCancel),
        ),
        FilledButton(
          key: StudioDriverKeys.archiveThreadConfirm,
          onPressed: () => Navigator.of(context).pop(true),
          child: Text(context.l10n.sidebarArchiveSessionConfirmAction),
        ),
      ],
    ),
  );
  if (confirmed != true || !context.mounted) return;
  try {
    await ref.read(studioControllerProvider.notifier).archiveThread(threadId);
  } on Object catch (error) {
    if (!context.mounted) return;
    final message = switch (error) {
      StudioFailure(code: StudioFailureCode.busy) =>
        context.l10n.sidebarArchiveSessionBusy,
      StudioFailure(:final message, :final correlationId) =>
        context.l10n.sidebarArchiveSessionFailedReason(message, correlationId),
      _ => context.l10n.sidebarArchiveSessionFailed,
    };
    ScaffoldMessenger.of(context)
        .showSnackBar(SnackBar(content: Text(message)));
  }
}

Future<void> _renameThreadFromSidebar(
  BuildContext context,
  WidgetRef ref,
  StudioThread thread,
) async {
  final title = await showDialog<String>(
    context: context,
    builder: (context) => _RenameThreadDialog(thread: thread),
  );
  if (title == null || !context.mounted) return;
  try {
    await ref
        .read(studioControllerProvider.notifier)
        .renameThread(thread.id, title);
  } on Object {
    if (!context.mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(context.l10n.sidebarRenameSessionFailed)),
    );
  }
}

class _RenameThreadDialog extends StatefulWidget {
  const _RenameThreadDialog({required this.thread});

  final StudioThread thread;

  @override
  State<_RenameThreadDialog> createState() => _RenameThreadDialogState();
}

class _RenameThreadDialogState extends State<_RenameThreadDialog> {
  late final TextEditingController _controller = TextEditingController(
    text: widget.thread.title,
  );
  String? _error;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _submit() {
    final title = _controller.text.trim();
    if (title.isEmpty) {
      setState(() => _error = context.l10n.sidebarRenameSessionEmpty);
      return;
    }
    if (title.runes.length > 80) {
      setState(() => _error = context.l10n.sidebarRenameSessionTooLong);
      return;
    }
    Navigator.of(context).pop(title);
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      key: StudioDriverKeys.renameThreadDialog(widget.thread.id),
      title: Text(context.l10n.sidebarRenameSessionTitle),
      content: TextField(
        key: StudioDriverKeys.renameThreadInput(widget.thread.id),
        controller: _controller,
        autofocus: true,
        maxLength: 80,
        textInputAction: TextInputAction.done,
        onSubmitted: (_) => _submit(),
        decoration: InputDecoration(
          labelText: context.l10n.sidebarRenameSessionInput,
          errorText: _error,
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: Text(context.l10n.commonCancel),
        ),
        FilledButton(
          key: StudioDriverKeys.renameThreadSave(widget.thread.id),
          onPressed: _submit,
          child: Text(context.l10n.commonSave),
        ),
      ],
    );
  }
}
