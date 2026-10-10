import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';
import 'settings_scope.dart';

class SkillsTab extends ConsumerStatefulWidget {
  const SkillsTab({super.key, required this.tabIndex});

  /// 本页在设置壳中的 tab 索引，用于「仅在可见时持有租约」。
  final int tabIndex;

  @override
  ConsumerState<SkillsTab> createState() => _SkillsTabState();
}

class _SkillsTabState extends ConsumerState<SkillsTab> {
  String _query = '';
  bool _visibleCache = false;
  bool _discovering = false;
  String? _discoverError;
  String? _saveError;
  Timer? _searchTimer;
  int _searchRequest = 0;
  List<SkillSummaryView>? _searchMatches;
  String? _lastProjectId;
  int? _lastCatalogRevision;

  @override
  void dispose() {
    _searchTimer?.cancel();
    super.dispose();
  }

  /// 变为可见时补一次显式读取；隐藏时释放租约（保留筛选/草稿 UI 状态）。
  void _syncVisibility(bool visible) {
    if (visible == _visibleCache) return;
    _visibleCache = visible;
    if (visible) unawaited(_refreshSkillsState());
  }

  String? _currentProjectId() =>
      ref.read(studioControllerProvider).value?.selectedProjectId;

  int _currentCatalogRevision() {
    final value = ref.read(studioControllerProvider).value;
    final projectId = value?.selectedProjectId;
    if (projectId == null) return 0;
    return value?.skillsByProject[projectId]?.catalogRevision ?? 0;
  }

  @override
  Widget build(BuildContext context) {
    // 只在 Skills tab 可见时租用当前 project 的 Skills topic；隐藏页不持租约。
    final visible = ref.watch(settingsVisibleTabProvider) == widget.tabIndex;
    _syncVisibility(visible);
    if (visible) {
      ref.watch(settingsSkillsScopeProvider);
    }
    // 只 watch 选中项目 identity 与 canonical 快照身份；无关领域更新不重建本页。
    final selection = ref.watch(
      studioControllerProvider.select((state) {
        final value = state.value;
        final projectId = value?.selectedProjectId;
        return (
          projectId: projectId,
          catalog: projectId == null ? null : value?.skillsByProject[projectId],
          settings: value?.skills,
          activeSkills: value?.runtime.activeSkills,
        );
      }),
    );
    final projectId = selection.projectId;
    final catalog = selection.catalog;
    final settings = selection.settings;
    final summaries = catalog?.summaries ?? const <SkillSummaryView>[];
    final catalogRevision = catalog?.catalogRevision ?? 0;
    // 项目/catalog 身份变化时释放旧搜索：不把旧 project 的结果混进新作用域。
    if (projectId != _lastProjectId ||
        catalogRevision != _lastCatalogRevision) {
      _lastProjectId = projectId;
      _lastCatalogRevision = catalogRevision;
      _searchTimer?.cancel();
      _searchRequest += 1;
      _searchMatches = null;
      if (_query.isNotEmpty) _scheduleSearch(_query);
    }
    final fallbackNames = <String>{
      ...?selection.activeSkills,
      ...?catalog?.skills,
      ...?settings?.disabled,
    }.toList();
    final skills = _query.isEmpty
        ? _allSkills(summaries, fallbackNames)
        : (_searchMatches ?? const <SkillSummaryView>[]);
    final disabledSkills = settings?.disabled.toSet() ?? const <String>{};
    return SettingsPane(
      header: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          SettingsHeader(
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
              onPressed: projectId == null || _discovering
                  ? null
                  : _discoverSkills,
            ),
          ),
          if (visible && projectId != null)
            SettingsTopicStatus(topic: SkillsTopic(projectId: projectId)),
        ],
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
            title: projectId == null
                ? context.l10n.settingsOpenProjectToDiscoverSkills
                : context.l10n.settingsNoSkillsMatchFilter,
            body: projectId == null
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

  List<SkillSummaryView> _allSkills(
    List<SkillSummaryView> summaries,
    List<String> fallbackNames,
  ) {
    final byName = {for (final skill in summaries) skill.name: skill};
    for (final name in fallbackNames) {
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
      if (result.projectId != _currentProjectId() ||
          result.catalogRevision != _currentCatalogRevision()) {
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
      final settings = ref.read(studioControllerProvider).value?.skills;
      if (settings == null) return;
      await ref
          .read(studioControllerProvider.notifier)
          .applySettingsField(SkillsDisabledCommand(disabled.toList()..sort()));
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
