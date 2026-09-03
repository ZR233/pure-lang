import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import 'settings_common.dart';
import 'settings_system_tabs.dart';

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
          _AgentProfileDialog(profile: profile, providers: widget.providers),
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
        final profiles = snapshot.data ?? const <AgentProfileView>[];
        return SettingsPane(
          children: [
            SettingsHeader(
              title: context.l10n.settingsAgentsTitle,
              subtitle: context.l10n.settingsAgentsSubtitle,
            ),
            if (worktreeIssues.isNotEmpty) ...[
              Text(
                context.l10n.settingsAgentsRecoveryTitle,
                style: context.text.titleMedium?.copyWith(
                  color: context.studioInk,
                  fontWeight: FontWeight.w700,
                ),
              ),
              const SizedBox(height: 8),
              for (final issue in worktreeIssues)
                _WorktreeRecoveryCard(issue: issue),
              const SizedBox(height: 16),
            ],
            Align(
              alignment: Alignment.centerLeft,
              child: FilledButton.icon(
                key: const ValueKey('agent-profile-add'),
                onPressed: _editProfile,
                icon: const Icon(Icons.add),
                label: Text(context.l10n.settingsAgentsAddUserProfile),
              ),
            ),
            const SizedBox(height: 16),
            for (final profile in profiles) ...[
              Card(
                color: context.studioPaper2,
                child: Column(
                  children: [
                    ListTile(
                      leading: Icon(
                        profile.system
                            ? Icons.lock_outline
                            : Icons.person_outline,
                      ),
                      title: Text(profile.displayName),
                      subtitle: Text(
                        '${profile.id} · ${profile.description}\n${_workspaceModeLabel(profile.workspaceMode)}',
                      ),
                      isThreeLine: true,
                      trailing: profile.system
                          ? Row(
                              mainAxisSize: MainAxisSize.min,
                              children: [
                                Chip(
                                  key: ValueKey(
                                    'system-agent-workspace-${profile.id}',
                                  ),
                                  label: Text(
                                    _workspaceModeLabel(profile.workspaceMode),
                                  ),
                                ),
                                const SizedBox(width: 8),
                                Switch(
                                  key: ValueKey(
                                    'system-agent-enabled-${profile.id}',
                                  ),
                                  value: profile.enabled,
                                  onChanged: (enabled) =>
                                      _setSystemEnabled(profile, enabled),
                                ),
                              ],
                            )
                          : Row(
                              mainAxisSize: MainAxisSize.min,
                              children: [
                                const Chip(label: Text('TOML')),
                                IconButton(
                                  key: ValueKey(
                                    'agent-profile-edit-${profile.id}',
                                  ),
                                  tooltip:
                                      context.l10n.settingsAgentsEditTooltip,
                                  onPressed: () => _editProfile(profile),
                                  icon: const Icon(Icons.edit_outlined),
                                ),
                              ],
                            ),
                    ),
                    if (profile.system)
                      Padding(
                        padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
                        child: AgentRouteControls(
                          role: profile.id,
                          providers: widget.providers,
                          roles: widget.roles,
                        ),
                      ),
                  ],
                ),
              ),
              const SizedBox(height: 8),
            ],
          ],
        );
      },
    );
  }
}

class _WorktreeRecoveryCard extends ConsumerStatefulWidget {
  const _WorktreeRecoveryCard({required this.issue});

  final StudioRecoveryIssue issue;

  @override
  ConsumerState<_WorktreeRecoveryCard> createState() =>
      _WorktreeRecoveryCardState();
}

class _WorktreeRecoveryCardState extends ConsumerState<_WorktreeRecoveryCard> {
  bool _cleaning = false;

  @override
  Widget build(BuildContext context) {
    final worktree = widget.issue.worktree!;
    return Card(
      key: ValueKey('worktree-recovery-${worktree.childId}'),
      color: context.studioPaper2,
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Wrap(
              spacing: 8,
              runSpacing: 8,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                Text(
                  worktree.branch,
                  style: context.text.titleSmall?.copyWith(
                    fontWeight: FontWeight.w700,
                  ),
                ),
                Chip(label: Text(worktree.state)),
                if (worktree.dirty) const Chip(label: Text('dirty')),
              ],
            ),
            const SizedBox(height: 8),
            SelectableText(
              'base ${worktree.baseCommit}\n'
              'head ${worktree.headCommit ?? context.l10n.settingsWorktreeHeadUnavailable}\n'
              '${worktree.path}',
            ),
            if (worktree.changedFiles.isNotEmpty) ...[
              const SizedBox(height: 8),
              Text(
                context.l10n.settingsWorktreeChangedFiles(
                  worktree.changedFiles.join(', '),
                ),
              ),
            ],
            const SizedBox(height: 12),
            FilledButton.tonalIcon(
              key: ValueKey('worktree-cleanup-${worktree.childId}'),
              onPressed: _cleaning ? null : () => _cleanup(worktree),
              icon: _cleaning
                  ? const SizedBox.square(
                      dimension: 16,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.delete_sweep_outlined),
              label: Text(context.l10n.settingsWorktreeCleanup),
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _cleanup(WorktreeRecoveryPreview worktree) async {
    setState(() => _cleaning = true);
    try {
      await ref
          .read(studioControllerProvider.notifier)
          .cleanupPreservedWorktree(worktree);
    } finally {
      if (mounted) setState(() => _cleaning = false);
    }
  }
}

class _AgentProfileDialog extends StatefulWidget {
  const _AgentProfileDialog({this.profile, required this.providers});

  final AgentProfileView? profile;
  final List<ProviderSettingsView> providers;

  @override
  State<_AgentProfileDialog> createState() => _AgentProfileDialogState();
}

class _AgentProfileDialogState extends State<_AgentProfileDialog> {
  final _formKey = GlobalKey<FormState>();
  late final TextEditingController _id;
  late final TextEditingController _displayName;
  late final TextEditingController _description;
  late final TextEditingController _whenToUse;
  late final TextEditingController _instructions;
  late String _providerId;
  late String _model;
  String? _effort;
  late bool _enabled;
  late AgentWorkspaceMode _workspaceMode;

  @override
  void initState() {
    super.initState();
    final profile = widget.profile;
    _id = TextEditingController(text: profile?.id);
    _displayName = TextEditingController(text: profile?.displayName);
    _description = TextEditingController(text: profile?.description);
    _whenToUse = TextEditingController(text: profile?.whenToUse);
    _instructions = TextEditingController(text: profile?.systemInstructions);
    _providerId =
        widget.providers
            .where((provider) => provider.id == profile?.providerId)
            .map((provider) => provider.id)
            .firstOrNull ??
        widget.providers.firstOrNull?.id ??
        '';
    final models = _modelsFor(_providerId);
    _model = models.any((model) => model.slug == profile?.model)
        ? profile!.model
        : models.firstOrNull?.slug ?? '';
    _effort = _canonicalEffort(profile?.effort);
    _enabled = profile?.enabled ?? true;
    _workspaceMode = profile?.workspaceMode ?? AgentWorkspaceMode.directory;
  }

  @override
  void dispose() {
    for (final controller in [
      _id,
      _displayName,
      _description,
      _whenToUse,
      _instructions,
    ]) {
      controller.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return AlertDialog(
      title: Text(
        widget.profile == null
            ? l10n.settingsAgentProfileAddTitle
            : l10n.settingsAgentProfileEditTitle,
      ),
      content: SizedBox(
        width: 560,
        child: Form(
          key: _formKey,
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                _field(
                  _id,
                  l10n.settingsAgentProfileIdField,
                  enabled: widget.profile == null,
                ),
                _field(_displayName, l10n.settingsAgentProfileDisplayNameField),
                _field(_description, l10n.settingsAgentProfileDescriptionField),
                _field(_whenToUse, l10n.settingsAgentProfileWhenToUseField),
                _field(
                  _instructions,
                  l10n.settingsAgentProfileInstructionsField,
                  maxLines: 6,
                ),
                DropdownButtonFormField<String>(
                  key: const ValueKey('agent-profile-provider'),
                  initialValue: _providerId.isEmpty ? null : _providerId,
                  decoration: InputDecoration(
                    labelText: l10n.settingsAgentProfileProviderField,
                  ),
                  items: widget.providers
                      .map(
                        (provider) => DropdownMenuItem(
                          value: provider.id,
                          child: Text(provider.name),
                        ),
                      )
                      .toList(growable: false),
                  onChanged: (providerId) {
                    if (providerId == null) return;
                    setState(() {
                      _providerId = providerId;
                      _model = _modelsFor(providerId).firstOrNull?.slug ?? '';
                      _effort = _canonicalEffort(null);
                    });
                  },
                  validator: (value) =>
                      value == null ? l10n.settingsAgentProfileRequired : null,
                ),
                const SizedBox(height: 10),
                DropdownButtonFormField<String>(
                  key: ValueKey('agent-profile-model-$_providerId'),
                  initialValue: _model.isEmpty ? null : _model,
                  decoration: InputDecoration(
                    labelText: l10n.settingsModelField,
                  ),
                  items: _modelsFor(_providerId)
                      .map(
                        (model) => DropdownMenuItem(
                          value: model.slug,
                          child: Text(
                            model.displayName.isEmpty
                                ? model.slug
                                : model.displayName,
                          ),
                        ),
                      )
                      .toList(growable: false),
                  onChanged: (model) {
                    if (model == null) return;
                    setState(() {
                      _model = model;
                      _effort = _canonicalEffort(null);
                    });
                  },
                  validator: (value) =>
                      value == null ? l10n.settingsAgentProfileRequired : null,
                ),
                const SizedBox(height: 10),
                DropdownButtonFormField<String?>(
                  key: ValueKey('agent-profile-effort-$_providerId-$_model'),
                  initialValue: _effort,
                  decoration: InputDecoration(
                    labelText: l10n.statusReasoningEffort,
                  ),
                  items: [
                    DropdownMenuItem<String?>(
                      value: null,
                      child: Text(l10n.settingsAgentProfileEffortDefault),
                    ),
                    for (final effort in _efforts)
                      DropdownMenuItem<String?>(
                        value: effort,
                        child: Text(effort),
                      ),
                  ],
                  onChanged: (effort) => setState(() => _effort = effort),
                ),
                DropdownButtonFormField<AgentWorkspaceMode>(
                  key: const ValueKey('agent-profile-workspace-mode'),
                  initialValue: _workspaceMode,
                  decoration: InputDecoration(
                    labelText: l10n.settingsAgentProfileWorkspaceModeField,
                  ),
                  items: AgentWorkspaceMode.values
                      .map(
                        (mode) => DropdownMenuItem(
                          value: mode,
                          child: Text(_workspaceModeLabel(mode)),
                        ),
                      )
                      .toList(growable: false),
                  onChanged: (mode) => setState(
                    () => _workspaceMode = mode ?? AgentWorkspaceMode.directory,
                  ),
                ),
                if (_workspaceMode == AgentWorkspaceMode.directory)
                  Padding(
                    padding: const EdgeInsets.only(top: 8, bottom: 10),
                    child: Text(
                      l10n.settingsAgentProfileWorkspaceDirectoryHint,
                    ),
                  ),
                SwitchListTile(
                  key: const ValueKey('agent-profile-enabled'),
                  contentPadding: EdgeInsets.zero,
                  title: Text(l10n.settingsAgentProfileEnabledTitle),
                  subtitle: Text(l10n.settingsAgentProfileEnabledSubtitle),
                  value: _enabled,
                  onChanged: (value) => setState(() => _enabled = value),
                ),
              ],
            ),
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: Text(l10n.commonCancel),
        ),
        FilledButton(
          key: const ValueKey('agent-profile-save'),
          onPressed: _save,
          child: Text(l10n.settingsAgentProfileSave),
        ),
      ],
    );
  }

  Widget _field(
    TextEditingController controller,
    String label, {
    bool enabled = true,
    bool required = true,
    int maxLines = 1,
  }) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: TextFormField(
        controller: controller,
        enabled: enabled,
        maxLines: maxLines,
        decoration: InputDecoration(labelText: label),
        validator: required
            ? (value) => value == null || value.trim().isEmpty
                  ? context.l10n.settingsAgentProfileRequired
                  : null
            : null,
      ),
    );
  }

  void _save() {
    if (!_formKey.currentState!.validate()) return;
    Navigator.pop(
      context,
      AgentProfileDraft(
        id: _id.text.trim(),
        displayName: _displayName.text.trim(),
        description: _description.text.trim(),
        whenToUse: _whenToUse.text.trim(),
        systemInstructions: _instructions.text.trim(),
        providerId: _providerId,
        model: _model,
        effort: _effort,
        enabled: _enabled,
        workspaceMode: _workspaceMode,
      ),
    );
  }

  List<ProviderModelView> _modelsFor(String providerId) =>
      widget.providers
          .where((provider) => provider.id == providerId)
          .firstOrNull
          ?.allModels ??
      const [];

  ProviderModelView? get _selectedModel =>
      _modelsFor(_providerId)
          .where((model) => model.slug == _model)
          .firstOrNull;

  List<String> get _efforts => _selectedModel?.reasoningEfforts ?? const [];

  String? _canonicalEffort(String? candidate) {
    final model = _selectedModel;
    if (candidate != null &&
        model?.reasoningEfforts.contains(candidate) == true) {
      return candidate;
    }
    final declaredDefault = model?.defaultReasoningEffort;
    if (declaredDefault != null &&
        declaredDefault.isNotEmpty &&
        model?.reasoningEfforts.contains(declaredDefault) == true) {
      return declaredDefault;
    }
    return model?.reasoningEfforts.firstOrNull;
  }
}

String _workspaceModeLabel(AgentWorkspaceMode mode) => switch (mode) {
  AgentWorkspaceMode.unrestricted => 'Unrestricted',
  AgentWorkspaceMode.directory => 'Directory',
  AgentWorkspaceMode.worktree => 'Worktree',
};
