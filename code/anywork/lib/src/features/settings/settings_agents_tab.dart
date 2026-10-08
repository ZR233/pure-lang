import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../shared/recovery_check_status.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import 'agent_workspace_mode_label.dart';
import 'settings_agent_profile_dialog.dart';
import 'settings_agent_routes.dart';
import 'settings_common.dart';
import 'settings_scope.dart';
import 'settings_worktree_recovery.dart';

/// Agent Profile 设置页。系统 Profile 只读；用户 Profile 由各自 TOML 文件管理。
class AgentsTab extends ConsumerStatefulWidget {
  const AgentsTab({super.key, required this.tabIndex});

  /// 本页在设置壳中的 tab 索引，用于「仅在可见时持有租约」。
  final int tabIndex;

  @override
  ConsumerState<AgentsTab> createState() => _AgentsTabState();
}

class _AgentsTabState extends ConsumerState<AgentsTab> {
  String _query = '';

  /// 最近一次可用的 canonical Profiles；重置/失败只保留局部提示，不清空既有 cards。
  AgentProfilesStateView? _lastProfiles;

  Future<void> _setSystemEnabled(AgentProfileView profile, bool enabled) async {
    await ref
        .read(studioControllerProvider.notifier)
        .setSystemAgentEnabled(profileId: profile.id, enabled: enabled);
  }

  Future<void> _editProfile([AgentProfileView? profile]) async {
    final draft = await showDialog<AgentProfileDraft>(
      context: context,
      builder: (context) =>
          AgentProfileDialog(profile: profile, providers: _routeProviders()),
    );
    if (draft == null || !mounted) return;
    await ref
        .read(studioControllerProvider.notifier)
        .saveUserAgentProfile(draft);
  }

  List<ProviderSettingsView> _routeProviders() =>
      ref.read(studioControllerProvider).value?.providers ??
      const <ProviderSettingsView>[];

  @override
  Widget build(BuildContext context) {
    // 只在 Agents tab 可见时租用 Agent Profiles topic；隐藏页不因 keep-alive 持租约。
    final visible = ref.watch(settingsVisibleTabProvider) == widget.tabIndex;
    if (visible) {
      ref.watch(settingsAgentsScopeProvider);
    }
    final profilesAsync = ref.watch(settingsAgentProfilesProvider);
    final latest = profilesAsync.value;
    if (latest != null) _lastProfiles = latest;
    final profilesState = latest ?? _lastProfiles;
    final profiles = profilesState?.data.profiles ?? const <AgentProfileView>[];
    final providers = ref.watch(
      studioControllerProvider.select(
        (state) => state.value?.providers ?? const <ProviderSettingsView>[],
      ),
    );
    final roles = ref.watch(
      studioControllerProvider.select(
        (state) => state.value?.roles ?? const <RoleSettingsView>[],
      ),
    );
    final recoveryState = ref.watch(
      studioControllerProvider.select((state) => state.value?.recoveryState),
    );
    final worktreeIssues =
        recoveryState?.values
            .where((issue) => issue.worktree != null)
            .toList(growable: false) ??
        const <StudioRecoveryIssue>[];
    final filtered = profiles.where((profile) {
      if (profile.id == 'planner') return false;
      final name = profile.system
          ? context.roleLabel(profile.id)
          : profile.displayName;
      return '$name ${profile.id} ${profile.description}'
          .toLowerCase()
          .contains(_query);
    }).toList();
    return SettingsPageLayout(
      header: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SettingsHeader(
            title: context.l10n.settingsAgentsTitle,
            subtitle: context.l10n.settingsAgentsSubtitle,
            trailing: FilledButton.icon(
              key: const ValueKey('agent-profile-add'),
              onPressed: _editProfile,
              icon: const Icon(Icons.add, size: 18),
              label: Text(context.l10n.settingsAgentsAddUserProfile),
            ),
          ),
          if (visible && profilesAsync.hasError)
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: SettingsInlineError(
                message: profilesAsync.error.toString(),
              ),
            ),
          if (visible) SettingsTopicStatus(topic: const AgentProfilesTopic()),
        ],
      ),
      toolbar: SettingsSearchField(
        hintText: context.l10n.settingsFilterAgents,
        onChanged: (query) =>
            setState(() => _query = query.trim().toLowerCase()),
      ),
      child: ListView(
        key: const ValueKey('settings-pane-scroll'),
        children: [
          const RecoveryCheckStatus(),
          if (profilesAsync.isLoading && profilesState == null)
            const Padding(
              padding: EdgeInsets.symmetric(vertical: 16),
              child: Center(child: CircularProgressIndicator()),
            ),
          if (worktreeIssues.isNotEmpty)
            SettingsSectionPanel(
              title: context.l10n.settingsAgentsRecoveryTitle,
              children: [
                for (final issue in worktreeIssues)
                  WorktreeRecoverySection(issue: issue),
              ],
            ),
          for (final system in [true, false])
            SettingsSectionPanel(
              title: system
                  ? context.l10n.settingsSystemAgentsGroup
                  : context.l10n.settingsUserAgentsGroup,
              children: [
                for (final profile in filtered.where(
                  (profile) => profile.system == system,
                ))
                  SettingsResourceRow(
                    key: ValueKey('agent-profile-card-${profile.id}'),
                    icon: profile.system
                        ? Icons.lock_outline
                        : Icons.person_outline,
                    title: profile.system
                        ? context.roleLabel(profile.id)
                        : profile.displayName,
                    subtitle:
                        '${profile.id} · ${profile.system ? context.roleDescription(profile.id) : profile.description}',
                    status: Text(
                      agentWorkspaceModeLabel(context, profile.workspaceMode),
                      key: profile.system
                          ? ValueKey('system-agent-workspace-${profile.id}')
                          : null,
                      style: context.text.bodySmall?.copyWith(
                        color: context.colors.onSurfaceVariant,
                      ),
                    ),
                    actions: [
                      if (profile.system)
                        Switch(
                          key: ValueKey('system-agent-enabled-${profile.id}'),
                          value: profile.enabled,
                          onChanged: (enabled) =>
                              _setSystemEnabled(profile, enabled),
                        )
                      else
                        TextButton.icon(
                          key: ValueKey('agent-profile-edit-${profile.id}'),
                          onPressed: () => _editProfile(profile),
                          icon: const Icon(Icons.edit_outlined, size: 16),
                          label: Text(context.l10n.settingsAgentsEditTooltip),
                        ),
                    ],
                    children: [
                      if (profile.system)
                        AgentRouteControls(
                          role: profile.id,
                          providers: providers,
                          roles: roles,
                        ),
                      const Divider(height: 24),
                    ],
                  ),
              ],
            ),
        ],
      ),
    );
  }
}
