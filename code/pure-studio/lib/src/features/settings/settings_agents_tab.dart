import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import 'agent_workspace_mode_label.dart';
import 'settings_agent_profile_dialog.dart';
import 'settings_agent_routes.dart';
import 'settings_common.dart';
import 'settings_worktree_recovery.dart';

/// Agent Profile 设置页。系统 Profile 只读；用户 Profile 由各自 TOML 文件管理。
class AgentsTab extends ConsumerStatefulWidget {
  const AgentsTab({super.key, required this.providers, required this.roles});

  final List<ProviderSettingsView> providers;
  final List<RoleSettingsView> roles;

  @override
  ConsumerState<AgentsTab> createState() => _AgentsTabState();
}

class _AgentsTabState extends ConsumerState<AgentsTab> {
  late Future<List<AgentProfileView>> _profiles;
  String _query = '';

  @override
  void initState() {
    super.initState();
    _profiles = ref.read(studioApiProvider).readAgentProfiles();
  }

  Future<void> _setSystemEnabled(AgentProfileView profile, bool enabled) async {
    await ref
        .read(studioControllerProvider.notifier)
        .setSystemAgentEnabled(profileId: profile.id, enabled: enabled);
    if (mounted) {
      setState(
        () => _profiles = ref.read(studioApiProvider).readAgentProfiles(),
      );
    }
  }

  Future<void> _editProfile([AgentProfileView? profile]) async {
    final draft = await showDialog<AgentProfileDraft>(
      context: context,
      builder: (context) =>
          AgentProfileDialog(profile: profile, providers: widget.providers),
    );
    if (draft == null || !mounted) return;
    await ref
        .read(studioControllerProvider.notifier)
        .saveUserAgentProfile(draft);
    if (mounted) {
      setState(
        () => _profiles = ref.read(studioApiProvider).readAgentProfiles(),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final worktreeIssues =
        ref
            .watch(studioControllerProvider)
            .value
            ?.recoveryIssues
            .where((issue) => issue.worktree != null)
            .toList(growable: false) ??
        const <StudioRecoveryIssue>[];
    return FutureBuilder<List<AgentProfileView>>(
      future: _profiles,
      builder: (context, snapshot) {
        if (snapshot.connectionState != ConnectionState.done) {
          return const Center(child: CircularProgressIndicator());
        }
        if (snapshot.hasError) {
          return Center(child: Text(snapshot.error.toString()));
        }
        final profiles = (snapshot.data ?? const <AgentProfileView>[]).where((
          profile,
        ) {
          final name = profile.system
              ? context.roleLabel(profile.id)
              : profile.displayName;
          return '$name ${profile.id} ${profile.description}'
              .toLowerCase()
              .contains(_query);
        }).toList();
        return SettingsPageLayout(
          header: SettingsHeader(
            title: context.l10n.settingsAgentsTitle,
            subtitle: context.l10n.settingsAgentsSubtitle,
            trailing: FilledButton.icon(
              key: const ValueKey('agent-profile-add'),
              onPressed: _editProfile,
              icon: const Icon(Icons.add, size: 18),
              label: Text(context.l10n.settingsAgentsAddUserProfile),
            ),
          ),
          toolbar: SettingsSearchField(
            hintText: context.l10n.settingsFilterAgents,
            onChanged: (query) =>
                setState(() => _query = query.trim().toLowerCase()),
          ),
          child: ListView(
            key: const ValueKey('settings-pane-scroll'),
            children: [
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
                    for (final profile in profiles.where(
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
                          agentWorkspaceModeLabel(
                            context,
                            profile.workspaceMode,
                          ),
                          key: profile.system
                              ? ValueKey('system-agent-workspace-${profile.id}')
                              : null,
                          style: context.text.bodySmall?.copyWith(
                            color: context.studioInkSoft,
                          ),
                        ),
                        actions: [
                          if (profile.system)
                            Switch(
                              key: ValueKey(
                                'system-agent-enabled-${profile.id}',
                              ),
                              value: profile.enabled,
                              onChanged: (enabled) =>
                                  _setSystemEnabled(profile, enabled),
                            )
                          else
                            TextButton.icon(
                              key: ValueKey('agent-profile-edit-${profile.id}'),
                              onPressed: () => _editProfile(profile),
                              icon: const Icon(Icons.edit_outlined, size: 16),
                              label: Text(
                                context.l10n.settingsAgentsEditTooltip,
                              ),
                            ),
                        ],
                        children: [
                          if (profile.system)
                            AgentRouteControls(
                              role: profile.id,
                              providers: widget.providers,
                              roles: widget.roles,
                            ),
                          const Divider(height: 24),
                        ],
                      ),
                  ],
                ),
            ],
          ),
        );
      },
    );
  }
}
