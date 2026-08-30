import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import 'settings_common.dart';

/// Agent Profile 设置页。系统 Profile 只读；用户 Profile 由各自 TOML 文件管理。
class AgentsTab extends ConsumerStatefulWidget {
  const AgentsTab({super.key});

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
    final next = ref
        .read(studioApiProvider)
        .setSystemAgentEnabled(profileId: profile.id, enabled: enabled);
    setState(() => _profiles = next);
    await next;
  }

  Future<void> _editProfile([AgentProfileView? profile]) async {
    final draft = await showDialog<AgentProfileDraft>(
      context: context,
      builder: (context) => _AgentProfileDialog(profile: profile),
    );
    if (draft == null || !mounted) return;
    final next = ref.read(studioApiProvider).saveUserAgentProfile(draft);
    setState(() => _profiles = next);
    await next;
  }

  @override
  Widget build(BuildContext context) {
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
              subtitle: '系统 Profile 只读；用户 Profile 保存在 ~/.pure/agents/*.toml。',
            ),
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
                child: ListTile(
                  leading: Icon(
                    profile.system ? Icons.lock_outline : Icons.person_outline,
                  ),
                  title: Text(profile.displayName),
                  subtitle: Text(
                    '${profile.id} · ${profile.description}\n${profile.providerId}/${profile.model}',
                  ),
                  isThreeLine: true,
                  trailing: profile.system
                      ? Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            const Chip(label: Text('内置 · 只读')),
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
                              key: ValueKey('agent-profile-edit-${profile.id}'),
                              tooltip: '编辑',
                              onPressed: () => _editProfile(profile),
                              icon: const Icon(Icons.edit_outlined),
                            ),
                          ],
                        ),
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

class _AgentProfileDialog extends StatefulWidget {
  const _AgentProfileDialog({this.profile});

  final AgentProfileView? profile;

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
  late final TextEditingController _provider;
  late final TextEditingController _model;
  late final TextEditingController _effort;
  late bool _enabled;

  @override
  void initState() {
    super.initState();
    final profile = widget.profile;
    _id = TextEditingController(text: profile?.id);
    _displayName = TextEditingController(text: profile?.displayName);
    _description = TextEditingController(text: profile?.description);
    _whenToUse = TextEditingController(text: profile?.whenToUse);
    _instructions = TextEditingController(text: profile?.systemInstructions);
    _provider = TextEditingController(text: profile?.providerId);
    _model = TextEditingController(text: profile?.model);
    _effort = TextEditingController(text: profile?.effort);
    _enabled = profile?.enabled ?? true;
  }

  @override
  void dispose() {
    for (final controller in [
      _id,
      _displayName,
      _description,
      _whenToUse,
      _instructions,
      _provider,
      _model,
      _effort,
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
                _field(_provider, 'Provider ID'),
                _field(_model, 'Model'),
                _field(_effort, 'Effort（可选）', required: false),
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
        providerId: _provider.text.trim(),
        model: _model.text.trim(),
        effort: _effort.text.trim().isEmpty ? null : _effort.text.trim(),
        enabled: _enabled,
      ),
    );
  }
}
