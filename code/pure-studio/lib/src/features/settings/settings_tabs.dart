import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_chrome.dart';
import 'settings_common.dart';

class InstructionsTab extends ConsumerStatefulWidget {
  const InstructionsTab({super.key, required this.settings});

  final InstructionsSettingsView settings;

  @override
  ConsumerState<InstructionsTab> createState() => InstructionsTabState();
}

class InstructionsTabState extends ConsumerState<InstructionsTab> {
  late final TextEditingController _baseController;
  late final TextEditingController _developerController;
  late final TextEditingController _userController;
  Timer? _saveTimer;
  bool _saving = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _baseController = TextEditingController(text: widget.settings.baseOverride);
    _developerController = TextEditingController(
      text: widget.settings.developer,
    );
    _userController = TextEditingController(text: widget.settings.user);
  }

  @override
  void didUpdateWidget(covariant InstructionsTab oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_saving) {
      return;
    }
    if (oldWidget.settings.baseOverride != widget.settings.baseOverride &&
        _baseController.text != widget.settings.baseOverride) {
      _baseController.text = widget.settings.baseOverride;
    }
    if (oldWidget.settings.developer != widget.settings.developer &&
        _developerController.text != widget.settings.developer) {
      _developerController.text = widget.settings.developer;
    }
    if (oldWidget.settings.user != widget.settings.user &&
        _userController.text != widget.settings.user) {
      _userController.text = widget.settings.user;
    }
  }

  @override
  void dispose() {
    _saveTimer?.cancel();
    _baseController.dispose();
    _developerController.dispose();
    _userController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return SettingsPane(
      children: [
        SettingsHeader(
          title: context.l10n.settingsInstructionsTitle,
          subtitle: context.l10n.settingsInstructionsSubtitle,
        ),
        const SizedBox(height: 16),
        _InstructionEditor(
          controller: _baseController,
          label: context.l10n.settingsBaseInstructions,
          icon: Icons.notes_outlined,
          onChanged: _scheduleSave,
        ),
        const SizedBox(height: 12),
        _InstructionEditor(
          controller: _developerController,
          label: context.l10n.settingsDeveloperInstructions,
          icon: Icons.code,
          onChanged: _scheduleSave,
        ),
        const SizedBox(height: 12),
        _InstructionEditor(
          controller: _userController,
          label: context.l10n.settingsUserContext,
          icon: Icons.person_outline,
          onChanged: _scheduleSave,
        ),
        if (_saving || _error != null) ...[
          const SizedBox(height: 12),
          if (_saving) const LinearProgressIndicator(),
          if (_error != null) SettingsInlineError(message: _error!),
        ],
      ],
    );
  }

  void _scheduleSave() {
    _saveTimer?.cancel();
    _saveTimer = Timer(const Duration(milliseconds: 650), () {
      unawaited(_save());
    });
  }

  Future<void> _save() async {
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await ref
          .read(studioControllerProvider.notifier)
          .saveInstructionsSettings(
            InstructionsSettingsCommand(
              baseOverride: _baseController.text,
              developer: _developerController.text,
              user: _userController.text,
              projectDocMaxBytes: widget.settings.projectDocMaxBytes,
              projectDocFallbackFilenames:
                  widget.settings.projectDocFallbackFilenames,
            ),
          );
    } catch (error) {
      if (mounted) {
        setState(() => _error = error.toString());
      }
    } finally {
      if (mounted) {
        setState(() => _saving = false);
      }
    }
  }
}

class _InstructionEditor extends StatelessWidget {
  const _InstructionEditor({
    required this.controller,
    required this.label,
    required this.icon,
    required this.onChanged,
  });

  final TextEditingController controller;
  final String label;
  final IconData icon;
  final VoidCallback onChanged;

  @override
  Widget build(BuildContext context) {
    return StudioPanel(
      backgroundColor: context.colors.surfaceContainerLowest,
      radius: StudioRadii.md,
      padding: const EdgeInsets.all(14),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(icon, size: 18, color: context.studioInkSoft),
              const SizedBox(width: 10),
              Text(
                label,
                style: context.text.titleSmall?.copyWith(
                  color: context.studioInk,
                  fontWeight: FontWeight.w500,
                ),
              ),
            ],
          ),
          const SizedBox(height: 12),
          TextField(
            controller: controller,
            minLines: 5,
            maxLines: 8,
            style: context.text.bodyMedium?.copyWith(
              color: context.studioInk,
              height: 1.55,
            ),
            decoration: InputDecoration(
              hintText: context.l10n.settingsInstructionHint,
              filled: true,
              fillColor: context.studioPaper,
              contentPadding: const EdgeInsets.symmetric(
                horizontal: 13,
                vertical: 12,
              ),
              border: OutlineInputBorder(
                borderRadius: BorderRadius.circular(StudioRadii.sm),
              ),
              enabledBorder: OutlineInputBorder(
                borderRadius: BorderRadius.circular(StudioRadii.sm),
                borderSide: BorderSide(color: context.studioLine),
              ),
              focusedBorder: OutlineInputBorder(
                borderRadius: BorderRadius.circular(StudioRadii.sm),
                borderSide: const BorderSide(color: StudioColors.clay),
              ),
            ),
            onChanged: (_) => onChanged(),
          ),
        ],
      ),
    );
  }
}

class SkillsTab extends ConsumerStatefulWidget {
  const SkillsTab({
    super.key,
    required this.skills,
    required this.settings,
    required this.projectId,
    required this.tabIndex,
  });

  final List<String> skills;
  final SkillsSettingsView settings;
  final String? projectId;
  final int tabIndex;

  @override
  ConsumerState<SkillsTab> createState() => SkillsTabState();
}

class SkillsTabState extends ConsumerState<SkillsTab> {
  String _query = '';
  final Set<String> _discoveredSkills = {};
  bool _discovering = false;
  String? _discoverError;
  String? _saveError;
  TabController? _tabController;
  bool _wasTabActive = false;

  @override
  void initState() {
    super.initState();
    _discoveredSkills.addAll(widget.skills);
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final controller = DefaultTabController.of(context);
    if (controller != _tabController) {
      _tabController?.removeListener(_handleTabChanged);
      _tabController = controller;
      _tabController?.addListener(_handleTabChanged);
      _wasTabActive = controller.index == widget.tabIndex;
      if (_wasTabActive && !_discovering) {
        _discoverSkills();
  }
    }
  }

  @override
  void dispose() {
    _tabController?.removeListener(_handleTabChanged);
    super.dispose();
  }

  void _handleTabChanged() {
    final controller = _tabController;
    if (controller == null) return;
    final isActive =
        controller.index == widget.tabIndex && !controller.indexIsChanging;
    if (isActive && !_wasTabActive && !_discovering) {
      _discoverSkills();
    }
    _wasTabActive = isActive;
  }

  @override
  void didUpdateWidget(covariant SkillsTab oldWidget) {
    super.didUpdateWidget(oldWidget);
    _discoveredSkills.addAll(widget.skills);
  }

  @override
  Widget build(BuildContext context) {
    final skills = {...widget.skills, ..._discoveredSkills}.toList()..sort();
    final disabledSkills = widget.settings.disabled.toSet();
    final filteredSkills = skills
        .where((skill) => skill.toLowerCase().contains(_query.toLowerCase()))
        .toList();
    return SettingsPane(
      children: [
        SettingsHeader(
          title: context.l10n.settingsSkillsTitle,
          subtitle: context.l10n.settingsSkillsSubtitle,
          trailing: FilledButton.icon(
            icon: _discovering
                ? const SizedBox.square(
                    dimension: 18,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const Icon(Icons.travel_explore),
            label: Text(
              _discovering
                  ? context.l10n.settingsDiscovering
                  : context.l10n.settingsDiscover,
            ),
            onPressed: widget.projectId == null || _discovering
                ? null
                : _discoverSkills,
          ),
        ),
        const SizedBox(height: 16),
        SettingsSearchField(
          hintText: context.l10n.settingsFilterSkills,
          onChanged: (value) => setState(() => _query = value),
        ),
        const SizedBox(height: 14),
        if (filteredSkills.isNotEmpty)
          SettingsGroup(
            children: [
              for (final skill in filteredSkills)
                SettingsToggleRow(
                  icon: Icons.extension_outlined,
                  title: skill,
                  subtitle: disabledSkills.contains(skill)
                      ? context.l10n.settingsSkillDisabled
                      : context.l10n.settingsSkillEnabled,
                  value: !disabledSkills.contains(skill),
                  onChanged: (selected) {
                    final disabled = {...disabledSkills};
                    if (selected) {
                      disabled.remove(skill);
                    } else {
                      disabled.add(skill);
                    }
                    unawaited(_saveDisabled(disabled));
                  },
                ),
            ],
          ),
        if (filteredSkills.isEmpty) ...[
          const SizedBox(height: 12),
          SettingsEmptyMessage(
            icon: Icons.extension_outlined,
            title: widget.projectId == null
                ? context.l10n.settingsOpenProjectToDiscoverSkills
                : context.l10n.settingsNoSkillsMatchFilter,
            body: widget.projectId == null
                ? context.l10n.settingsSkillsDiscoverySources
                : context.l10n.settingsClearSearchOrDiscoverAgain,
          ),
        ],
        if (_discoverError != null) ...[
          const SizedBox(height: 12),
          SettingsInlineError(message: _discoverError!),
        ],
        if (_saveError != null) ...[
          const SizedBox(height: 12),
          SettingsInlineError(message: _saveError!),
        ],
      ],
    );
  }

  Future<void> _saveDisabled(Set<String> disabled) async {
    try {
      setState(() => _saveError = null);
      await ref
          .read(studioControllerProvider.notifier)
          .saveSkillsSettings(
            SkillsSettingsCommand(
              enabled: widget.settings.enabled,
              autoLearn: widget.settings.autoLearn,
              systemEnabled: widget.settings.systemEnabled,
              projectDir: widget.settings.projectDir,
              userDir: widget.settings.userDir,
              externalDirs: widget.settings.externalDirs,
              disabled: disabled.toList()..sort(),
              autoLearnMinToolCalls: widget.settings.autoLearnMinToolCalls,
            ),
          );
    } catch (error) {
      if (mounted) {
        setState(() => _saveError = error.toString());
      }
    }
  }

  Future<void> _discoverSkills() async {
    setState(() {
      _discovering = true;
      _discoverError = null;
    });
    try {
      final discovered = await ref
          .read(studioControllerProvider.notifier)
          .listDiscoveredSkills();
      if (!mounted) {
        return;
      }
      setState(() {
        _discoveredSkills
          ..clear()
          ..addAll(widget.skills)
          ..addAll(discovered);
      });
    } catch (error) {
      if (mounted) {
        setState(() => _discoverError = error.toString());
      }
    } finally {
      if (mounted) {
        setState(() => _discovering = false);
      }
    }
  }
}
