part of 'studio_shell.dart';

class _Sidebar extends ConsumerWidget {
  const _Sidebar({required this.state, required this.compact});

  final SidebarView state;
  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final width = compact
        ? StudioLayout.compactRailWidth
        : StudioLayout.sidebarWidth;
    return SizedBox(
      key: StudioDriverKeys.sidebar,
      width: width,
      child: Material(
        color: context.colors.surfaceContainer,
        child: Column(
          children: [
            SizedBox(
              height: 52,
              child: Center(
                child: compact
                    ? StudioIconBadge(
                        tone: StudioTone.brand,
                        filled: true,
                        icon: Icons.auto_awesome_motion,
                        size: 34,
                      )
                    : Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 20),
                        child: Row(
                          children: [
                            StudioIconBadge(
                              filled: true,
                              icon: Icons.auto_awesome_motion,
                              size: 34,
                            ),
                            const SizedBox(width: 10),
                            Expanded(
                              child: Text(
                                context.l10n.appTitle,
                                overflow: TextOverflow.ellipsis,
                                style: Theme.of(context).textTheme.titleMedium
                                    ?.copyWith(
                                      fontWeight: FontWeight.w700,
                                      color: context.colors.onSurface,
                                    ),
                              ),
                            ),
                          ],
                        ),
                      ),
              ),
            ),
            Padding(
              padding: EdgeInsets.symmetric(
                horizontal: compact ? 8 : 16,
                vertical: 12,
              ),
              child: SizedBox(
                width: double.infinity,
                child: FilledButton(
                  key: StudioDriverKeys.newSession,
                  onPressed:
                      state.selectedProjectId != null &&
                          !state.projectRecoveryIssues.containsKey(
                            state.selectedProjectId,
                          )
                      ? ref
                            .read(studioControllerProvider.notifier)
                            .beginNewThread
                      : null,
                  style: FilledButton.styleFrom(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 8,
                      vertical: 14,
                    ),
                  ),
                  child: compact
                      ? const Icon(Icons.add, size: 18)
                      : Row(
                          mainAxisAlignment: MainAxisAlignment.center,
                          children: [
                            const Icon(Icons.add, size: 18),
                            const SizedBox(width: 8),
                            Flexible(
                              child: Text(
                                context.l10n.sidebarNewSession,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                          ],
                        ),
                ),
              ),
            ),
            Expanded(
              child: _SidebarDirectoryList(state: state, compact: compact),
            ),
            Divider(height: 1, color: context.colors.outlineVariant),
            _SidebarActions(state: state, compact: compact),
          ],
        ),
      ),
    );
  }
}

/// 侧栏目录分页列表：项目区固定在顶部，会话区懒构建并触底加载下一页。
