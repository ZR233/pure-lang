import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';

class SkillsTab extends ConsumerStatefulWidget {
  const SkillsTab({
    super.key,
    required this.skills,
    required this.fallbackSkillNames,
    required this.settings,
    required this.projectId,
    required this.catalogRevision,
    required this.tabIndex,
  });

  final List<SkillSummaryView> skills;
  final List<String> fallbackSkillNames;
  final SkillsSettingsView settings;
  final String? projectId;
  final int catalogRevision;
  final int tabIndex;

  @override
  ConsumerState<SkillsTab> createState() => _SkillsTabState();
}

class _SkillsTabState extends ConsumerState<SkillsTab> {
  String _query = '';
  TabController? _tabs;
  bool _selected = false;
  bool _discovering = false;
  String? _discoverError;
  String? _saveError;
  Timer? _searchTimer;
  int _searchRequest = 0;
  List<SkillSummaryView>? _searchMatches;

  @override
  void initState() {
    super.initState();
    unawaited(_refreshSkillsState());
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final tabs = DefaultTabController.maybeOf(context);
    if (identical(tabs, _tabs)) return;
    _tabs?.removeListener(_handleTabChange);
    _tabs = tabs;
    _selected = tabs?.index == widget.tabIndex;
    tabs?.addListener(_handleTabChange);
  }

  void _handleTabChange() {
    final selected = _tabs?.index == widget.tabIndex;
    if (selected && !_selected) unawaited(_refreshSkillsState());
    _selected = selected;
  }

  @override
  void didUpdateWidget(covariant SkillsTab oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.projectId != widget.projectId ||
        oldWidget.catalogRevision != widget.catalogRevision) {
      _searchTimer?.cancel();
      _searchRequest += 1;
      _searchMatches = null;
      if (_query.isNotEmpty) {
        _scheduleSearch(_query);
      }
    }
  }

  @override
  void dispose() {
    _searchTimer?.cancel();
    _tabs?.removeListener(_handleTabChange);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final skills = _query.isEmpty
        ? _allSkills()
        : (_searchMatches ?? const <SkillSummaryView>[]);
    final disabledSkills = widget.settings.disabled.toSet();
    return SettingsPane(
      header: SettingsHeader(
        title: context.l10n.settingsSkillsTitle,
        subtitle: context.l10n.settingsSkillsSubtitle,
        trailing: FilledButton.icon(
          key: StudioDriverKeys.skillsDiscover,
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
      toolbar: SettingsSearchField(
        hintText: context.l10n.settingsFilterSkills,
        onChanged: _onSearchChanged,
      ),
      children: [
        const SizedBox(height: 16),
        const SizedBox(height: 14),
        if (skills.isNotEmpty)
          SettingsGroup(
            children: [
              for (final skill in skills)
                ExpansionTile(
                  controlAffinity: ListTileControlAffinity.leading,
                  key: PageStorageKey('skill-details-${skill.name}'),
                  tilePadding: EdgeInsets.zero,
                  title: Text(skill.name, style: context.text.titleSmall),
                  // 开关是唯一的启停指示，副标题改为技能描述，避免与开关语义重复。
                  subtitle: skill.description.isEmpty
                      ? null
                      : Text(
                          skill.description,
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                        ),
                  trailing: Switch(
                    key: ValueKey('skill-enabled-${skill.name}'),
                    value: !disabledSkills.contains(skill.name),
                    onChanged: (selected) {
                      final disabled = {...disabledSkills};
                      if (selected) {
                        disabled.remove(skill.name);
                      } else {
                        disabled.add(skill.name);
                      }
                      unawaited(_saveDisabled(disabled));
                    },
                  ),
                  children: [
                    Align(
                      alignment: Alignment.centerLeft,
                      child: Padding(
                        padding: const EdgeInsets.only(bottom: 18),
                        child: SelectableText(
                          key: PageStorageKey(
                            'skill-description-${skill.name}',
                          ),
                          [
                            skill.description,
                            skill.source,
                          ].where((value) => value.isNotEmpty).join('\n'),
                          style: context.text.bodySmall?.copyWith(
                            color: context.colors.onSurfaceVariant,
                          ),
                        ),
                      ),
                    ),
                  ],
                ),
            ],
          ),
        if (skills.isEmpty) ...[
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

  List<SkillSummaryView> _allSkills() {
    final byName = {for (final skill in widget.skills) skill.name: skill};
    for (final name in widget.fallbackSkillNames) {
      byName.putIfAbsent(
        name,
        () => SkillSummaryView(
          name: name,
          description: '',
          source: '',
          providerId: '',
          modelInvocable: true,
          userInvocable: true,
          resourceBase: const SkillResourceBaseView(
            SkillResourceBaseKind.opaque,
            '',
          ),
        ),
      );
    }
    return byName.values.toList()..sort(
      (left, right) =>
          left.name.toLowerCase().compareTo(right.name.toLowerCase()),
    );
  }

  void _onSearchChanged(String value) {
    final query = value.trim();
    _searchTimer?.cancel();
    _searchRequest += 1;
    setState(() {
      _query = query;
      _searchMatches = null;
    });
    if (query.isNotEmpty) {
      _scheduleSearch(query);
    }
  }

  void _scheduleSearch(String query) {
    final request = _searchRequest;
    _searchTimer = Timer(const Duration(milliseconds: 150), () {
      unawaited(_searchSkills(query, request));
    });
  }

  Future<void> _searchSkills(String query, int request) async {
    try {
      final result = await ref
          .read(studioControllerProvider.notifier)
          .searchSkills(query);
      if (!mounted || request != _searchRequest || result == null) {
        return;
      }
      if (result.projectId != widget.projectId ||
          result.catalogRevision != widget.catalogRevision) {
        return;
      }
      setState(() => _searchMatches = result.matches);
    } catch (error) {
      if (mounted && request == _searchRequest) {
        setState(() => _discoverError = error.toString());
      }
    }
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

  Future<void> _refreshSkillsState() async {
    try {
      await ref.read(studioControllerProvider.notifier).refreshSkillsState();
    } catch (error) {
      if (mounted) {
        setState(() => _discoverError = error.toString());
      }
    }
  }

  Future<void> _discoverSkills() async {
    setState(() {
      _discovering = true;
      _discoverError = null;
    });
    try {
      await ref.read(studioControllerProvider.notifier).discoverSkills();
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
