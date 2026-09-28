import 'package:flutter/material.dart';

import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/model_route_selector.dart';
import 'agent_workspace_mode_label.dart';
import 'settings_common.dart';

class AgentProfileDialog extends StatefulWidget {
  const AgentProfileDialog({super.key, this.profile, required this.providers});

  final AgentProfileView? profile;
  final List<ProviderSettingsView> providers;

  @override
  State<AgentProfileDialog> createState() => _AgentProfileDialogState();
}

class _AgentProfileDialogState extends State<AgentProfileDialog> {
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
    if (profile == null) {
      _providerId = widget.providers.firstOrNull?.id ?? '';
      _model = _modelsFor(_providerId).firstOrNull?.slug ?? '';
      _effort = _canonicalEffort(null);
    } else {
      // 保留 canonical provider/model/effort；不可解析时只在展示层标记
      // unavailable，不静默改为 options.first 或模型默认 effort，也不阻断
      // 其他字段的保存。
      _providerId = profile.providerId;
      _model = profile.model;
      _effort = profile.effort;
    }
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
    return SettingsFormDialog(
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
                FormField<String>(
                  initialValue: _model.isEmpty ? null : _model,
                  validator: (value) => _providerId.isEmpty || value == null
                      ? l10n.settingsAgentProfileRequired
                      : null,
                  builder: (field) {
                    final options = _modelSelectionOptions;
                    final providerName =
                        widget.providers
                            .where((provider) => provider.id == _providerId)
                            .firstOrNull
                            ?.name ??
                        _providerId;
                    final unavailableLabel =
                        _providerUnavailable || _modelUnavailable
                        ? l10n.settingsAgentRouteUnavailable(
                            [
                              providerName,
                              _model,
                            ].where((part) => part.isNotEmpty).join(' / '),
                          )
                        : null;
                    final errorText = field.errorText;
                    return Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Row(
                          children: [
                            Padding(
                              padding: const EdgeInsets.only(right: 8),
                              child: SettingsFieldLabel(
                                label: l10n.settingsModelField,
                              ),
                            ),
                            Flexible(
                              child: ModelRouteSelector(
                                selectorKey: ValueKey(
                                  'agent-profile-model-$_providerId',
                                ),
                                fieldLabel: l10n.settingsModelField,
                                options: options,
                                providerId: _providerId,
                                model: _model,
                                unresolvedLabel: unavailableLabel,
                                showUnresolvedOption: unavailableLabel != null,
                                enabled: options.isNotEmpty,
                                onSelected: (option) {
                                  setState(() {
                                    _providerId = option.providerId;
                                    _model = option.model;
                                    _effort = _canonicalEffort(null);
                                  });
                                  field.didChange(
                                    option.model.isEmpty ? null : option.model,
                                  );
                                },
                              ),
                            ),
                          ],
                        ),
                        if (errorText != null)
                          Padding(
                            padding: const EdgeInsets.only(top: 4, left: 8),
                            child: Semantics(
                              liveRegion: true,
                              child: Text(
                                errorText,
                                style: Theme.of(context).textTheme.bodySmall
                                    ?.copyWith(
                                      color: Theme.of(context)
                                          .colorScheme
                                          .error,
                                    ),
                              ),
                            ),
                          ),
                      ],
                    );
                  },
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
                    if (_effortUnavailable)
                      DropdownMenuItem<String?>(
                        value: _effort,
                        child: Text(_effort!),
                      ),
                    for (final effort in _efforts)
                      DropdownMenuItem<String?>(
                        value: effort,
                        child: Text(effort),
                      ),
                  ],
                  onChanged: (effort) => setState(() => _effort = effort),
                ),
                const SizedBox(height: 12),
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
                          child: Text(agentWorkspaceModeLabel(context, mode)),
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

  bool get _providerUnavailable =>
      _providerId.isNotEmpty &&
      !widget.providers.any((provider) => provider.id == _providerId);

  bool get _modelUnavailable => _model.isNotEmpty && _selectedModel == null;

  List<ModelSelectionOption> get _modelSelectionOptions =>
      buildModelSelectionOptions(
        widget.providers,
        modelsForProvider: (provider) => provider.allModels,
      );

  bool get _effortUnavailable => _effort != null && !_efforts.contains(_effort);

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
