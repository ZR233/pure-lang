import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import '../../shared/studio_badges.dart';
import 'settings_agents_tab.dart';
import 'settings_general_tab.dart';
import 'settings_instructions_tab.dart';
import 'settings_lsp_tab.dart';
import 'settings_mcp_tab.dart';
import 'settings_provider_tab.dart';
import 'settings_scope.dart';
import 'settings_security_tab.dart';
import 'settings_skills_tab.dart';
import 'settings_ssh_tab.dart';
import 'settings_statistics_tab.dart';

class SettingsPage extends ConsumerStatefulWidget {
  const SettingsPage({super.key});

  @override
  ConsumerState<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends ConsumerState<SettingsPage> {
  bool _popping = false;

  Future<void> _popAfterFlush() async {
    if (_popping) return;
    _popping = true;
    try {
      await ref.read(studioControllerProvider.notifier).flushPending();
      if (mounted && context.canPop()) context.pop();
    } finally {
      _popping = false;
    }
  }

  @override
  Widget build(BuildContext context) {
    return PopScope<void>(
      canPop: false,
      onPopInvokedWithResult: (didPop, result) {
        if (!didPop) unawaited(_popAfterFlush());
      },
      child: const _SettingsPageShell(),
    );
  }
}

/// Owns the tab controller independently from the reactive settings projection.
/// A model/provider save refreshes canonical content without recreating the
/// navigation controller or restarting the whole settings page animation.
class _SettingsPageShell extends StatelessWidget {
  const _SettingsPageShell();

  @override
  Widget build(BuildContext context) {
    return DefaultTabController(
      length: _settingsTabs.length,
      child: const _SettingsPageBody(),
    );
  }
}

class _SettingsPageBody extends ConsumerStatefulWidget {
  const _SettingsPageBody();

  @override
  ConsumerState<_SettingsPageBody> createState() => _SettingsPageBodyState();
}

class _SettingsPageBodyState extends ConsumerState<_SettingsPageBody> {
  @override
  Widget build(BuildContext context) {
    // Only the readiness edge belongs to the page shell.  Once the app-level
    // repository has a canonical snapshot, field updates must rebuild their
    // own tab projection instead of replacing the retained TabBarView.
    final readiness = ref.watch(
      studioControllerProvider.select(
        (state) => (ready: state.value != null, error: state.error),
      ),
    );
    if (!readiness.ready) {
      // 仅在首帧尚无任何 canonical 设置视图时占位。
      return Scaffold(
        body: Center(
          child: readiness.error == null
              ? const CircularProgressIndicator()
              : Text(readiness.error.toString()),
        ),
      );
    }
    return Scaffold(
      backgroundColor: context.colors.surface,
      body: KeyedSubtree(
        key: StudioDriverKeys.settingsPage,
        child: _SettingsTabVisibilitySync(child: const _SettingsScaffold()),
      ),
    );
  }
}

/// 把设置壳的 [DefaultTabController] 当前索引同步到 [settingsVisibleTabProvider]。
///
/// 设置壳保活已访问过的 tab，但可见性变化必须让各数据页建立/释放自己的 scope 租约。
/// provider 写入延到微任务，避免在 build/通知阶段直接改 provider。
class _SettingsTabVisibilitySync extends ConsumerStatefulWidget {
  const _SettingsTabVisibilitySync({required this.child});

  final Widget child;

  @override
  ConsumerState<_SettingsTabVisibilitySync> createState() =>
      _SettingsTabVisibilitySyncState();
}

class _SettingsTabVisibilitySyncState
    extends ConsumerState<_SettingsTabVisibilitySync> {
  TabController? _controller;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final controller = DefaultTabController.of(context);
    if (identical(controller, _controller)) return;
    _controller?.removeListener(_sync);
    _controller = controller;
    controller.addListener(_sync);
    _sync();
  }

  void _sync() {
    final controller = _controller;
    if (controller == null) return;
    final index = controller.index;
    Future<void>.microtask(() {
      if (!mounted) return;
      ref.read(settingsVisibleTabProvider.notifier).select(index);
    });
  }

  @override
  void dispose() {
    _controller?.removeListener(_sync);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.child;
}

/// Each settings tab subscribes to the smallest projection it can render.
/// The retained settings shell therefore survives catalog/config events and a
/// model click only rebuilds the provider tab that displays that catalog.
class _ProvidersSettingsTab extends ConsumerWidget {
  const _ProvidersSettingsTab();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final providers = ref.watch(
      studioControllerProvider.select(
        (state) => state.value?.providers ?? const <ProviderSettingsView>[],
      ),
    );
    final catalog = ref.watch(
      studioControllerProvider.select(
        (state) =>
            state.value?.providerCatalog ?? const ProviderCatalogView.empty(),
      ),
    );
    final defaultProviderId = ref.watch(
      studioControllerProvider.select(
        (state) => state.value?.defaultProviderId,
      ),
    );
    return ProvidersTab(
      providers: providers,
      providerCatalog: catalog,
      defaultProviderId: defaultProviderId,
      tabIndex: _SettingsTab.providers.index,
    );
  }
}

class _InstructionsSettingsTab extends ConsumerWidget {
  const _InstructionsSettingsTab();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.watch(
      studioControllerProvider.select(
        (state) =>
            state.value?.instructions ?? const InstructionsSettingsView(),
      ),
    );
    return InstructionsTab(settings: settings);
  }
}

class _SecuritySettingsTab extends ConsumerWidget {
  const _SecuritySettingsTab();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final mode = ref.watch(
      studioControllerProvider.select(
        (state) =>
            state.value?.permissionMode ?? PermissionMode.requestApproval,
      ),
    );
    return SecurityTab(mode: mode);
  }
}

class _GeneralSettingsTab extends ConsumerWidget {
  const _GeneralSettingsTab();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.watch(
      studioControllerProvider.select(
        (state) => state.value?.general ?? const GeneralSettingsView(),
      ),
    );
    final webSearch = ref.watch(
      studioControllerProvider.select(
        (state) => state.value?.webSearch ?? const WebSearchSettingsView(),
      ),
    );
    final deepSeekWebSearch = ref.watch(
      studioControllerProvider.select(
        (state) =>
            state.value?.deepSeekWebSearch ??
            const DeepSeekWebSearchSettingsView(),
      ),
    );
    final runtimeBusy = ref.watch(
      studioControllerProvider.select(
        (state) =>
            state.value?.isBusy == true ||
            state.value?.runtime.hasActiveWorkflow == true,
      ),
    );
    return GeneralTab(
      settings: settings,
      webSearch: webSearch,
      deepSeekWebSearch: deepSeekWebSearch,
      runtimeBusy: runtimeBusy,
    );
  }
}

const _settingsTabs = [
  _SettingsTabInfo(Icons.cloud_outlined, _SettingsTab.providers),
  _SettingsTabInfo(Icons.notes_outlined, _SettingsTab.instructions),
  _SettingsTabInfo(Icons.extension_outlined, _SettingsTab.skills),
  _SettingsTabInfo(Icons.groups_outlined, _SettingsTab.agents),
  _SettingsTabInfo(Icons.hub_outlined, _SettingsTab.mcp),
  _SettingsTabInfo(Icons.code_outlined, _SettingsTab.lsp),
  _SettingsTabInfo(Icons.dns_outlined, _SettingsTab.ssh),
  _SettingsTabInfo(Icons.query_stats_outlined, _SettingsTab.statistics),
  _SettingsTabInfo(Icons.security_outlined, _SettingsTab.security),
  _SettingsTabInfo(Icons.tune_outlined, _SettingsTab.general),
];

class _SettingsTabInfo {
  const _SettingsTabInfo(this.icon, this.tab);

  final IconData icon;
  final _SettingsTab tab;

  String label(BuildContext context) {
    return switch (tab) {
      _SettingsTab.providers => context.l10n.settingsProvidersTab,
      _SettingsTab.instructions => context.l10n.settingsInstructionsTab,
      _SettingsTab.skills => context.l10n.settingsSkillsTab,
      _SettingsTab.agents => context.l10n.settingsAgentsTab,
      _SettingsTab.mcp => context.l10n.settingsMcpTab,
      _SettingsTab.lsp => context.l10n.settingsLspTab,
      _SettingsTab.ssh => context.l10n.settingsSshTab,
      _SettingsTab.statistics => context.l10n.settingsStatisticsTab,
      _SettingsTab.security => context.l10n.settingsSecurityTab,
      _SettingsTab.general => context.l10n.settingsGeneralTab,
    };
  }
}

enum _SettingsTab {
  providers,
  instructions,
  skills,
  agents,
  mcp,
  lsp,
  ssh,
  statistics,
  security,
  general,
}

class _SettingsScaffold extends StatelessWidget {
  const _SettingsScaffold();

  @override
  Widget build(BuildContext context) {
    final views = [
      const _ProvidersSettingsTab(),
      const _InstructionsSettingsTab(),
      SkillsTab(tabIndex: _SettingsTab.skills.index),
      AgentsTab(tabIndex: _SettingsTab.agents.index),
      McpTab(tabIndex: _SettingsTab.mcp.index),
      LspTab(tabIndex: _SettingsTab.lsp.index),
      const SshTab(),
      StatisticsTab(tabIndex: _SettingsTab.statistics.index),
      const _SecuritySettingsTab(),
      const _GeneralSettingsTab(),
    ];
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 820;
        if (compact) {
          return Column(
            children: [
              _SettingsNav(compact: true),
              Expanded(
                child: TabBarView(
                  children: [
                    for (final view in views) _RetainedSettingsTab(child: view),
                  ],
                ),
              ),
            ],
          );
        }
        return Row(
          children: [
            const _SettingsNav(compact: false),
            VerticalDivider(width: 1, color: context.colors.outlineVariant),
            Expanded(
              child: TabBarView(
                children: [
                  for (final view in views) _RetainedSettingsTab(child: view),
                ],
              ),
            ),
          ],
        );
      },
    );
  }
}

class _SettingsNav extends StatelessWidget {
  const _SettingsNav({required this.compact});

  final bool compact;

  @override
  Widget build(BuildContext context) {
    final controller = DefaultTabController.of(context);
    return AnimatedBuilder(
      animation: controller,
      builder: (context, _) {
        final selected = controller.index;
        final navItems = [
          for (var index = 0; index < _settingsTabs.length; index++)
            _SettingsNavItem(
              tab: _settingsTabs[index],
              selected: selected == index,
              compact: compact,
              onTap: () => controller.animateTo(index),
            ),
        ];
        if (compact) {
          return Material(
            color: context.colors.surfaceContainer,
            child: DecoratedBox(
              decoration: BoxDecoration(
                border: Border(
                  bottom: BorderSide(color: context.colors.outlineVariant),
                ),
              ),
              child: SizedBox(
                height: 56,
                child: ListView(
                  scrollDirection: Axis.horizontal,
                  padding: const EdgeInsets.symmetric(
                    horizontal: 10,
                    vertical: 8,
                  ),
                  children: [
                    _SettingsBackTile(compact: true),
                    const SizedBox(width: 8),
                    ...navItems,
                  ],
                ),
              ),
            ),
          );
        }
        return Material(
          color: context.colors.surfaceContainer,
          child: SizedBox(
            width: StudioLayout.settingsNavigationWidth,
            child: ListView(
              padding: const EdgeInsets.fromLTRB(12, 18, 12, 16),
              children: [
                const _SettingsBackTile(compact: false),
                _SettingsNavGroupLabel(context.l10n.settingsModelsGroup),
                for (final index in [0, 3, 1]) navItems[index],
                _SettingsNavGroupLabel(context.l10n.settingsExtensionsGroup),
                for (final index in [2, 4, 5, 6]) navItems[index],
                _SettingsNavGroupLabel(context.l10n.settingsPreferencesGroup),
                for (final index in [7, 8, 9]) navItems[index],
              ],
            ),
          ),
        );
      },
    );
  }
}

class _SettingsBackTile extends ConsumerWidget {
  const _SettingsBackTile({required this.compact});

  final bool compact;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final content = Row(
      mainAxisSize: compact ? MainAxisSize.min : MainAxisSize.max,
      children: [
        Icon(
          Icons.arrow_back,
          size: 16,
          color: context.colors.onSurfaceVariant,
        ),
        if (!compact) ...[
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              context.l10n.settingsBackToChat,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: context.text.labelMedium?.copyWith(
                color: context.colors.onSurfaceVariant,
                fontWeight: FontWeight.w500,
              ),
            ),
          ),
        ],
      ],
    );
    return Tooltip(
      message: context.l10n.settingsBack,
      child: Material(
        color: Colors.transparent,
        borderRadius: BorderRadius.circular(StudioRadii.sm),
        child: InkWell(
          key: StudioDriverKeys.settingsBack,
          borderRadius: BorderRadius.circular(StudioRadii.sm),
          onTap: () => unawaited(_popAfterFlush(context, ref)),
          child: Padding(
            padding: EdgeInsets.symmetric(
              horizontal: compact ? 10 : 10,
              vertical: compact ? 9 : 8,
            ),
            child: content,
          ),
        ),
      ),
    );
  }

  Future<void> _popAfterFlush(BuildContext context, WidgetRef ref) async {
    // Settings is a pushed child of the session shell. Reconcile in-flight
    // writes first, with a bounded deadline, then pop the existing shell so
    // its selectors observe the repository's canonical state immediately.
    await ref.read(studioControllerProvider.notifier).flushPending();
    if (context.mounted) context.pop();
  }
}

class _SettingsNavGroupLabel extends StatelessWidget {
  const _SettingsNavGroupLabel(this.label);

  final String label;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(10, 14, 10, 6),
      child: Text(
        label,
        style: context.text.labelSmall?.copyWith(
          color: context.colors.onSurfaceVariant,
          fontWeight: FontWeight.w700,
          letterSpacing: 0,
        ),
      ),
    );
  }
}

class _SettingsNavItem extends StatelessWidget {
  const _SettingsNavItem({
    required this.tab,
    required this.selected,
    required this.compact,
    required this.onTap,
  });

  final _SettingsTabInfo tab;
  final bool selected;
  final bool compact;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final foreground = selected
        ? context.colors.onPrimaryContainer
        : context.colors.onSurfaceVariant;
    final label = tab.label(context);
    return Padding(
      padding: EdgeInsets.only(right: compact ? 8 : 0, bottom: compact ? 0 : 6),
      child: Material(
        color: selected
            ? context.colors.surfaceContainerHigh
            : Colors.transparent,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(StudioRadii.sm),
        ),
        child: StudioSelectionMarker(
          selected: selected,
          child: InkWell(
            key: StudioDriverKeys.settingsTab(tab.tab.name),
            borderRadius: BorderRadius.circular(StudioRadii.sm),
            onTap: onTap,
            child: Padding(
              padding: EdgeInsets.symmetric(
                horizontal: compact ? 12 : 10,
                vertical: compact ? 8 : 10,
              ),
              child: Row(
                mainAxisSize: compact ? MainAxisSize.min : MainAxisSize.max,
                children: [
                  Icon(tab.icon, size: 18, color: foreground),
                  const SizedBox(width: 10),
                  if (compact)
                    Text(
                      label,
                      overflow: TextOverflow.ellipsis,
                      style: context.text.labelMedium?.copyWith(
                        color: foreground,
                        fontWeight: selected
                            ? FontWeight.w600
                            : FontWeight.w500,
                      ),
                    )
                  else
                    Expanded(
                      child: Text(
                        label,
                        overflow: TextOverflow.ellipsis,
                        style: context.text.labelMedium?.copyWith(
                          color: foreground,
                          fontWeight: selected
                              ? FontWeight.w600
                              : FontWeight.w500,
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// Keep an already visited settings page alive so changing tabs does not discard
/// editor text, search results or scroll position. Unvisited tabs remain lazy.
class _RetainedSettingsTab extends StatefulWidget {
  const _RetainedSettingsTab({required this.child});
  final Widget child;
  @override
  State<_RetainedSettingsTab> createState() => _RetainedSettingsTabState();
}

class _RetainedSettingsTabState extends State<_RetainedSettingsTab>
    with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;
  @override
  Widget build(BuildContext context) {
    super.build(context);
    return widget.child;
  }
}
