import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../shared/studio_chrome.dart';

part 'settings_provider_tab.dart';
part 'settings_provider_list.dart';
part 'settings_provider_editor.dart';
part 'settings_common.dart';
part 'settings_provider_drafts.dart';
part 'settings_tabs.dart';

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
          appBar: AppBar(
            leading: IconButton(
              tooltip: 'Back',
              icon: const Icon(Icons.arrow_back),
              onPressed: () => context.go('/'),
            ),
            title: const Text('Settings'),
          ),
          body: _SettingsScaffold(state: state),
        ),
      ),
    );
  }
}

const _settingsTabs = [
  _SettingsTabInfo(Icons.cloud_outlined, 'Providers'),
  _SettingsTabInfo(Icons.notes_outlined, 'Instructions'),
  _SettingsTabInfo(Icons.extension_outlined, 'Skills'),
  _SettingsTabInfo(Icons.badge_outlined, 'Roles'),
  _SettingsTabInfo(Icons.hub_outlined, 'MCP'),
  _SettingsTabInfo(Icons.security_outlined, 'Security'),
  _SettingsTabInfo(Icons.tune_outlined, 'General'),
];

class _SettingsTabInfo {
  const _SettingsTabInfo(this.icon, this.label);

  final IconData icon;
  final String label;
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
      _GeneralTab(settings: state.general),
    ];
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 820;
        if (compact) {
          return Column(
            children: [
              _SettingsNav(compact: true),
              Divider(height: 1, color: context.studioLine),
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
        final children = [
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
            child: SizedBox(
              height: 56,
              child: ListView(
                scrollDirection: Axis.horizontal,
                padding: const EdgeInsets.symmetric(
                  horizontal: 10,
                  vertical: 8,
                ),
                children: children,
              ),
            ),
          );
        }
        return Material(
          color: context.studioPaper2,
          child: SizedBox(
            width: 232,
            child: ListView(
              padding: const EdgeInsets.fromLTRB(12, 14, 12, 16),
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(4, 2, 4, 14),
                  child: Row(
                    children: [
                      const StudioIconBadge(
                        icon: Icons.tune_outlined,
                        size: 32,
                      ),
                      const SizedBox(width: 10),
                      Expanded(
                        child: Text(
                          'Studio Settings',
                          overflow: TextOverflow.ellipsis,
                          style: context.text.titleSmall?.copyWith(
                            color: context.studioInk,
                            fontWeight: FontWeight.w700,
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
                ...children,
              ],
            ),
          ),
        );
      },
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
    return Padding(
      padding: EdgeInsets.only(right: compact ? 8 : 0, bottom: compact ? 0 : 6),
      child: Material(
        color: selected ? StudioColors.claySoft : Colors.transparent,
        borderRadius: BorderRadius.circular(StudioRadii.sm),
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
                const SizedBox(width: 9),
                Text(
                  tab.label,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.labelMedium?.copyWith(
                    color: foreground,
                    fontWeight: selected ? FontWeight.w700 : FontWeight.w500,
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
          padding: const EdgeInsets.fromLTRB(24, 22, 24, 28),
          children: children,
        ),
      ),
    );
  }
}
