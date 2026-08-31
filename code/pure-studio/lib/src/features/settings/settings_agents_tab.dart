import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
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
            const SettingsHeader(
              title: 'Agent Profiles',
              subtitle: '系统 Profile 的用途与工作区模式固定；可统一配置启用状态和模型。Directory 只约束 Pure 内置文件写工具，shell、Git、MCP 可绕过。',
            ),
            if (worktreeIssues.isNotEmpty) ...[
              Text(
                'Recovery',
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
                label: const Text('添加用户 Profile'),
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
                                  tooltip: '编辑',
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
              'head ${worktree.headCommit ?? 'unavailable'}\n'
              '${worktree.path}',
            ),
            if (worktree.changedFiles.isNotEmpty) ...[
              const SizedBox(height: 8),
              Text('Changed files: ${worktree.changedFiles.join(', ')}'),
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
              label: const Text('显式清理 worktree 与分支'),
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
    return AlertDialog(
      title: Text(
        widget.profile == null ? '添加用户 Agent Profile' : '编辑用户 Agent Profile',
      ),
      content: SizedBox(
        width: 560,
        child: Form(
          key: _formKey,
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                _field(_id, 'Agent ID', enabled: widget.profile == null),
                _field(_displayName, '显示名称'),
                _field(_description, '介绍'),
                _field(_whenToUse, '适用任务'),
                _field(_instructions, '系统指令', maxLines: 6),
                DropdownButtonFormField<String>(
                  key: const ValueKey('agent-profile-provider'),
                  initialValue: _providerId.isEmpty ? null : _providerId,
                  decoration: const InputDecoration(labelText: 'Provider'),
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
                  validator: (value) => value == null ? '必填' : null,
                ),
                const SizedBox(height: 10),
                DropdownButtonFormField<String>(
                  key: ValueKey('agent-profile-model-$_providerId'),
                  initialValue: _model.isEmpty ? null : _model,
                  decoration: const InputDecoration(labelText: 'Model'),
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
                  validator: (value) => value == null ? '必填' : null,
                ),
                const SizedBox(height: 10),
                DropdownButtonFormField<String?>(
                  key: ValueKey('agent-profile-effort-$_providerId-$_model'),
                  initialValue: _effort,
                  decoration: const InputDecoration(labelText: '思考等级'),
                  items: [
                    const DropdownMenuItem<String?>(
                      value: null,
                      child: Text('使用模型默认值'),
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
                  decoration: const InputDecoration(labelText: '工作区模式'),
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
                  const Padding(
                    padding: EdgeInsets.only(top: 8, bottom: 10),
                    child: Text(
                      'Directory 是合作式文件工具边界，不是 OS 沙箱；shell、Git 和 MCP 可能绕过。',
                    ),
                  ),
                SwitchListTile(
                  key: const ValueKey('agent-profile-enabled'),
                  contentPadding: EdgeInsets.zero,
                  title: const Text('启用'),
                  subtitle: const Text('禁用后仍保留 TOML，但不会出现在 Agent 工具目录。'),
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
          child: const Text('取消'),
        ),
        FilledButton(
          key: const ValueKey('agent-profile-save'),
          onPressed: _save,
          child: const Text('原子保存 TOML'),
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
            ? (value) => value == null || value.trim().isEmpty ? '必填' : null
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
