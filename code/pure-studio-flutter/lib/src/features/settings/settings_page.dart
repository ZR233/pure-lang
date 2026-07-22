import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_chrome.dart';
import '../update/studio_update_controller.dart';

part 'settings_provider_tab.dart';
part 'settings_provider_list.dart';
part 'settings_provider_usage.dart';
part 'settings_provider_editor.dart';
part 'settings_common.dart';
part 'settings_provider_drafts.dart';
part 'settings_tabs.dart';
part 'settings_system_tabs.dart';
part 'settings_web_search.dart';
part 'settings_update_row.dart';

class SettingsPage extends ConsumerWidget {
  const SettingsPage({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final asyncState = ref.watch(studioControllerProvider);
    return asyncState.when(
      loading: () =>
          const Scaffold(body: Center(child: CircularProgressIndicator())),
      error: (error, stackTrace) =>
          Scaffold(body: Center(child: Text(error.toString()))),
      data: (state) => DefaultTabController(
        length: _settingsTabs.length,
        child: Scaffold(
          backgroundColor: context.studioPaper,
          body: _SettingsScaffold(state: state),
        ),
      ),
    );
  }
}

const _settingsTabs = [
  _SettingsTabInfo(Icons.cloud_outlined, _SettingsTab.providers),
  _SettingsTabInfo(Icons.notes_outlined, _SettingsTab.instructions),
  _SettingsTabInfo(Icons.extension_outlined, _SettingsTab.skills),
  _SettingsTabInfo(Icons.badge_outlined, _SettingsTab.roles),
  _SettingsTabInfo(Icons.hub_outlined, _SettingsTab.mcp),
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
      _SettingsTab.roles => context.l10n.settingsRolesTab,
      _SettingsTab.mcp => context.l10n.settingsMcpTab,
      _SettingsTab.security => context.l10n.settingsSecurityTab,
      _SettingsTab.general => context.l10n.settingsGeneralTab,
    };
  }
}

enum _SettingsTab {
  providers,
  instructions,
  skills,
  roles,
  mcp,
  security,
  general,
}

class _SettingsScaffold extends StatelessWidget {
  const _SettingsScaffold({required this.state});

  final StudioState state;

  @override
  Widget build(BuildContext context) {
    final views = [
      _ProvidersTab(providers: state.providers, roles: state.roles),
      _InstructionsTab(settings: state.instructions),
      _SkillsTab(
        skills: {
          ...state.runtime.activeSkills,
          ...state.skills.disabled,
        }.toList(),
        settings: state.skills,
        projectId: state.selectedProjectId,
      ),
      _RolesTab(providers: state.providers, roles: state.roles),
      _McpTab(servers: state.mcpServers),
      _SecurityTab(mode: state.permissionMode),
      _GeneralTab(
        settings: state.general,
        webSearch: state.webSearch,
        runtimeBusy: state.isBusy || state.runtime.hasActiveTask,
      ),
    ];
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 820;
        if (compact) {
          return Column(
            children: [
              _SettingsNav(compact: true),
              Expanded(child: TabBarView(children: views)),
            ],
          );
        }
        return Row(
          children: [
            const _SettingsNav(compact: false),
            VerticalDivider(width: 1, color: context.studioLine),
            Expanded(child: TabBarView(children: views)),
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
            color: context.studioPaper2,
            child: DecoratedBox(
              decoration: BoxDecoration(
                border: Border(bottom: BorderSide(color: context.studioLine)),
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
          color: context.studioPaper2,
          child: SizedBox(
            width: StudioLayout.settingsNavigationWidth,
            child: ListView(
              padding: const EdgeInsets.fromLTRB(12, 18, 12, 16),
              children: [
                const _SettingsBackTile(compact: false),
                _SettingsNavGroupLabel(context.l10n.settingsWorkspaceGroup),
                ...navItems.take(5),
                _SettingsNavGroupLabel(context.l10n.settingsSystemGroup),
                ...navItems.skip(5),
              ],
            ),
          ),
        );
      },
    );
  }
}

class _SettingsBackTile extends StatelessWidget {
  const _SettingsBackTile({required this.compact});

  final bool compact;

  @override
  Widget build(BuildContext context) {
    final content = Row(
      mainAxisSize: compact ? MainAxisSize.min : MainAxisSize.max,
      children: [
        Icon(Icons.arrow_back, size: 16, color: context.studioInkSoft),
        if (!compact) ...[
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              context.l10n.settingsBackToChat,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: context.text.labelMedium?.copyWith(
                color: context.studioInkSoft,
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
          borderRadius: BorderRadius.circular(StudioRadii.sm),
          onTap: () => context.go('/'),
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
          color: context.studioInkSoft.withValues(alpha: 0.64),
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
    final foreground = selected ? StudioColors.clayDeep : context.studioInkSoft;
    final label = tab.label(context);
    return Padding(
      padding: EdgeInsets.only(right: compact ? 8 : 0, bottom: compact ? 0 : 6),
      child: Material(
        color: selected ? context.studioPaper : Colors.transparent,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(StudioRadii.sm),
          side: BorderSide(
            color: selected ? context.studioLine2 : Colors.transparent,
          ),
        ),
        child: InkWell(
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
                      fontWeight: selected ? FontWeight.w600 : FontWeight.w500,
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
    );
  }
}

class _SettingsPane extends StatelessWidget {
  const _SettingsPane({required this.children, this.maxWidth = 980});

  final List<Widget> children;
  final double maxWidth;

  @override
  Widget build(BuildContext context) {
    return Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: BoxConstraints(maxWidth: maxWidth),
        child: ListView(
          padding: const EdgeInsets.fromLTRB(28, 22, 28, 30),
          children: children,
        ),
      ),
    );
  }
}
