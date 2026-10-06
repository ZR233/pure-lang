import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_form_select.dart';
import 'settings_common.dart';

/// Canonical web-search modes the settings card can edit. Any other runtime
/// value is shown raw and never silently coerced into a known mode.
const _knownWebSearchModes = ['disabled', 'cached', 'indexed', 'live'];

/// Canonical context-size values; an empty selection means the service
/// default. Any other runtime value is shown raw.
const _knownWebSearchContextSizes = ['low', 'medium', 'high'];

class WebSearchSettingsCard extends ConsumerStatefulWidget {
  const WebSearchSettingsCard({super.key, required this.settings});

  final WebSearchSettingsView settings;

  @override
  ConsumerState<WebSearchSettingsCard> createState() =>
      WebSearchSettingsCardState();
}

class WebSearchSettingsCardState extends ConsumerState<WebSearchSettingsCard> {
  late String _mode;
  String? _contextSize;
  late final TextEditingController _domainsController;
  late final TextEditingController _countryController;
  late final TextEditingController _regionController;
  late final TextEditingController _cityController;
  late final TextEditingController _timezoneController;
  bool _saving = false;

  /// 草稿是否与 canonical 编辑值不同：dirty 时外部 canonical 变化只更新状态摘要，不覆盖草稿。
  ///
  /// 由 [_computeDirty] 按完整草稿字段与 canonical 编辑值比较得到，焦点/选区变化或选择同值
  /// 都不会置位；编辑后再恢复 canonical 值会重新变为 false。
  bool _dirty = false;

  /// 正在把 canonical 值写入控件；期间控件回调不应把草稿标记为 dirty。
  bool _applyingCanonical = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _domainsController = TextEditingController(
      text: widget.settings.allowedDomains.join(', '),
    );
    _countryController = TextEditingController(text: widget.settings.country);
    _regionController = TextEditingController(text: widget.settings.region);
    _cityController = TextEditingController(text: widget.settings.city);
    _timezoneController = TextEditingController(text: widget.settings.timezone);
    for (final controller in [
      _domainsController,
      _countryController,
      _regionController,
      _cityController,
      _timezoneController,
    ]) {
      controller.addListener(_handleDraftEdited);
    }
    _syncFrom(widget.settings);
  }

  @override
  void didUpdateWidget(covariant WebSearchSettingsCard oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.settings == widget.settings) return;
    // 外部 canonical（含换模型/换 key 导致的可用性变化）始终更新 controller 状态；控件只在
    // 用户未编辑时跟随，dirty 草稿保留，避免失败或外部变化清空未保存输入。成功后由 [_save]
    // 显式用返回的 canonical 同步并清除 dirty。
    if (_dirty) {
      // 保留未保存草稿；外部值恰好等于草稿时重算可清除 dirty。
      _dirty = _computeDirty();
    } else {
      _syncFrom(widget.settings);
      _dirty = false;
    }
  }

  /// 完整草稿字段是否与 canonical 编辑值不同。
  ///
  /// 直接与 [WebSearchSettingsCard.settings] 的编辑字段逐一比较，不新增第二 canonical 事实源。
  bool _computeDirty() {
    final settings = widget.settings;
    return _mode != settings.configuredMode ||
        _contextSize != settings.contextSize ||
        _domainsController.text != settings.allowedDomains.join(', ') ||
        _countryController.text != (settings.country ?? '') ||
        _regionController.text != (settings.region ?? '') ||
        _cityController.text != (settings.city ?? '') ||
        _timezoneController.text != (settings.timezone ?? '');
  }

  /// 文本控件回调（含焦点/选区变化）：按实际值重算 dirty，同步写入 canonical 时跳过。
  void _handleDraftEdited() {
    if (_applyingCanonical) return;
    _recomputeDirty();
  }

  void _recomputeDirty() {
    final next = _computeDirty();
    if (next == _dirty) return;
    setState(() => _dirty = next);
  }

  /// 用 canonical [settings] 覆盖本地草稿：下拉与文本框都回到已发布值。
  void _syncFrom(WebSearchSettingsView settings) {
    _applyingCanonical = true;
    try {
      _mode = settings.configuredMode;
      _contextSize = settings.contextSize;
      _replaceText(_domainsController, settings.allowedDomains.join(', '));
      _replaceText(_countryController, settings.country ?? '');
      _replaceText(_regionController, settings.region ?? '');
      _replaceText(_cityController, settings.city ?? '');
      _replaceText(_timezoneController, settings.timezone ?? '');
    } finally {
      _applyingCanonical = false;
    }
  }

  @override
  void dispose() {
    _domainsController.dispose();
    _countryController.dispose();
    _regionController.dispose();
    _cityController.dispose();
    _timezoneController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final settings = widget.settings;
    final sizeValue = _contextSize ?? '';
    final contextSizeIsKnown =
        sizeValue.isEmpty || _knownWebSearchContextSizes.contains(sizeValue);
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(
                Icons.travel_explore,
                color: context.colors.onSurfaceVariant,
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      context.l10n.settingsWebSearchTitle,
                      style: context.text.titleMedium?.copyWith(
                        color: context.colors.onSurface,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      context.l10n.settingsWebSearchSubtitle,
                      style: context.text.bodySmall?.copyWith(
                        color: context.colors.onSurfaceVariant,
                      ),
                    ),
                  ],
                ),
              ),
              Chip(
                visualDensity: VisualDensity.compact,
                label: Text(_availabilityLabel(context, settings.availability)),
              ),
            ],
          ),
          const SizedBox(height: 16),
          Wrap(
            spacing: 12,
            runSpacing: 12,
            children: [
              _WebSearchStatusValue(
                label: context.l10n.settingsWebSearchConfiguredMode,
                value: _modeLabel(context, settings.configuredMode),
              ),
              _WebSearchStatusValue(
                label: context.l10n.settingsWebSearchEffectiveMode,
                value: _modeLabel(context, settings.effectiveMode),
              ),
              _WebSearchStatusValue(
                label: context.l10n.settingsWebSearchProvider,
                value: settings.providerId ?? context.l10n.settingsNotAvailable,
              ),
              _WebSearchStatusValue(
                label: context.l10n.settingsWebSearchModel,
                value: settings.model ?? context.l10n.settingsNotAvailable,
              ),
            ],
          ),
          if (!settings.isAvailable && settings.availability != 'disabled') ...[
            const SizedBox(height: 12),
            Text(
              _availabilityReason(context, settings.availability),
              style: context.text.bodySmall?.copyWith(
                color: context.colors.error,
              ),
            ),
          ],
          const SizedBox(height: 18),
          LayoutBuilder(
            builder: (context, constraints) {
              final fieldWidth = constraints.maxWidth >= 620
                  ? (constraints.maxWidth - 12) / 2
                  : constraints.maxWidth;
              return Wrap(
                spacing: 12,
                runSpacing: 12,
                children: [
                  SizedBox(
                    width: fieldWidth,
                    child: StudioFormSelectField<String>(
                      key: const ValueKey('web_search_mode'),
                      value: _mode,
                      hint: _knownWebSearchModes.contains(_mode)
                          ? null
                          : Text(_modeLabel(context, _mode)),
                      decoration: InputDecoration(
                        labelText: context.l10n.settingsWebSearchMode,
                      ),
                      items: [
                        for (final mode in _knownWebSearchModes)
                          StudioFormSelectItem<String>(
                            value: mode,
                            itemKey: ValueKey('web_search_mode_$mode'),
                            child: Text(_modeLabel(context, mode)),
                          ),
                      ],
                      onChanged: _saving
                          ? null
                          : (value) => setState(() {
                              _mode = value ?? _mode;
                              _dirty = _computeDirty();
                            }),
                    ),
                  ),
                  SizedBox(
                    width: fieldWidth,
                    child: StudioFormSelectField<String>(
                      key: const ValueKey('web_search_context_size'),
                      value: sizeValue,
                      hint: contextSizeIsKnown
                          ? null
                          : Text(_contextSizeLabel(context, sizeValue)),
                      decoration: InputDecoration(
                        labelText: context.l10n.settingsWebSearchContextSize,
                      ),
                      items: [
                        StudioFormSelectItem<String>(
                          value: '',
                          itemKey: const ValueKey(
                            'web_search_context_size_default',
                          ),
                          child: Text(context.l10n.settingsServiceDefault),
                        ),
                        for (final size in _knownWebSearchContextSizes)
                          StudioFormSelectItem<String>(
                            value: size,
                            itemKey: ValueKey('web_search_context_size_$size'),
                            child: Text(_contextSizeLabel(context, size)),
                          ),
                      ],
                      onChanged: _saving
                          ? null
                          : (value) => setState(() {
                              _contextSize = value?.isEmpty == true
                                  ? null
                                  : value;
                              _dirty = _computeDirty();
                            }),
                    ),
                  ),
                  SizedBox(
                    width: constraints.maxWidth,
                    child: TextField(
                      key: const ValueKey('web_search_domains'),
                      controller: _domainsController,
                      enabled: !_saving,
                      decoration: InputDecoration(
                        labelText: context.l10n.settingsWebSearchAllowedDomains,
                        hintText: context.l10n.settingsWebSearchDomainsHint,
                      ),
                    ),
                  ),
                  for (final field in [
                    (
                      fieldKey: const ValueKey('web_search_country'),
                      controller: _countryController,
                      label: context.l10n.settingsWebSearchCountry,
                    ),
                    (
                      fieldKey: const ValueKey('web_search_region'),
                      controller: _regionController,
                      label: context.l10n.settingsWebSearchRegion,
                    ),
                    (
                      fieldKey: const ValueKey('web_search_city'),
                      controller: _cityController,
                      label: context.l10n.settingsWebSearchCity,
                    ),
                    (
                      fieldKey: const ValueKey('web_search_timezone'),
                      controller: _timezoneController,
                      label: context.l10n.settingsWebSearchTimezone,
                    ),
                  ])
                    SizedBox(
                      width: fieldWidth,
                      child: TextField(
                        key: field.fieldKey,
                        controller: field.controller,
                        enabled: !_saving,
                        decoration: InputDecoration(labelText: field.label),
                      ),
                    ),
                ],
              );
            },
          ),
          const SizedBox(height: 14),
          Row(
            children: [
              FilledButton.icon(
                key: const ValueKey('web_search_save'),
                onPressed: _saving ? null : _save,
                icon: _saving
                    ? const SizedBox.square(
                        dimension: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.save_outlined),
                label: Text(context.l10n.settingsSaveWebSearch),
              ),
              if (_dirty && !_saving) ...[
                const SizedBox(width: 12),
                Text(
                  context.l10n.settingsWebSearchUnsaved,
                  style: context.text.bodySmall?.copyWith(
                    color: context.colors.onSurfaceVariant,
                  ),
                ),
              ],
              if (_error != null) ...[
                const SizedBox(width: 12),
                Expanded(child: SettingsInlineError(message: _error!)),
              ],
            ],
          ),
        ],
      ),
    );
  }

  Future<void> _save() async {
    if (_saving) return;
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      final snapshot = await ref
          .read(studioControllerProvider.notifier)
          .saveWebSearchSettings(
            WebSearchSettingsCommand(
              mode: _mode,
              contextSize: _contextSize,
              allowedDomains: _domainsController.text
                  .split(RegExp(r'[,\n]'))
                  .map((value) => value.trim())
                  .where((value) => value.isNotEmpty)
                  .toSet()
                  .toList(),
              country: _nullableText(_countryController),
              region: _nullableText(_regionController),
              city: _nullableText(_cityController),
              timezone: _nullableText(_timezoneController),
            ),
          );
      if (!mounted) return;
      // 成功后显式采用已发布的 canonical 快照，清除 dirty，草稿与已保存值回到同一事实源。
      setState(() {
        _syncFrom(snapshot.webSearch);
        _dirty = false;
      });
    } catch (error) {
      if (!mounted) return;
      // 失败保留草稿与错误：controller 已在 revision/CAS 冲突时刷新 canonical 状态，
      // 这里不覆盖草稿，也不自动重放保存。
      setState(() => _error = error.toString());
    } finally {
      if (mounted) {
        setState(() => _saving = false);
      }
    }
  }

  String? _nullableText(TextEditingController controller) {
    final value = controller.text.trim();
    return value.isEmpty ? null : value;
  }

  void _replaceText(TextEditingController controller, String value) {
    if (controller.text != value) {
      controller.text = value;
    }
  }
}

class _WebSearchStatusValue extends StatelessWidget {
  const _WebSearchStatusValue({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: 180,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            label,
            style: context.text.labelSmall?.copyWith(
              color: context.colors.onSurfaceVariant,
            ),
          ),
          const SizedBox(height: 2),
          Text(
            value,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: context.text.bodyMedium?.copyWith(
              color: context.colors.onSurface,
            ),
          ),
        ],
      ),
    );
  }
}

class DeepSeekWebSearchSettingsCard extends ConsumerStatefulWidget {
  const DeepSeekWebSearchSettingsCard({super.key, required this.settings});

  final DeepSeekWebSearchSettingsView settings;

  @override
  ConsumerState<DeepSeekWebSearchSettingsCard> createState() =>
      _DeepSeekWebSearchSettingsCardState();
}

class _DeepSeekWebSearchSettingsCardState
    extends ConsumerState<DeepSeekWebSearchSettingsCard> {
  bool _saving = false;
  String? _error;

  @override
  Widget build(BuildContext context) {
    final settings = widget.settings;
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.public, color: context.colors.onSurfaceVariant),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      context.l10n.settingsDeepSeekWebSearchTitle,
                      style: context.text.titleMedium?.copyWith(
                        color: context.colors.onSurface,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      context.l10n.settingsDeepSeekWebSearchSubtitle,
                      style: context.text.bodySmall?.copyWith(
                        color: context.colors.onSurfaceVariant,
                      ),
                    ),
                  ],
                ),
              ),
              if (_saving)
                const Padding(
                  padding: EdgeInsets.all(12),
                  child: SizedBox.square(
                    dimension: 18,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  ),
                )
              else
                Switch(
                  key: const ValueKey('deepseek_web_search_enabled'),
                  value: settings.configuredEnabled,
                  onChanged: _save,
                ),
            ],
          ),
          const SizedBox(height: 16),
          Wrap(
            spacing: 12,
            runSpacing: 12,
            children: [
              _WebSearchStatusValue(
                label: context.l10n.settingsDeepSeekWebSearchConfigured,
                value: _enabledLabel(context, settings.configuredEnabled),
              ),
              _WebSearchStatusValue(
                label: context.l10n.settingsDeepSeekWebSearchEffective,
                value: _enabledLabel(context, settings.effectiveEnabled),
              ),
              _WebSearchStatusValue(
                label: context.l10n.settingsDeepSeekWebSearchProvider,
                value: settings.providerId ?? context.l10n.settingsNotAvailable,
              ),
              _WebSearchStatusValue(
                label: context.l10n.settingsWebSearchModel,
                value: settings.model ?? context.l10n.settingsNotAvailable,
              ),
            ],
          ),
          const SizedBox(height: 12),
          Text(
            _availabilityLabel(context, settings.availability),
            style: context.text.bodySmall?.copyWith(
              color: settings.isAvailable
                  ? context.colors.onSurfaceVariant
                  : context.colors.error,
            ),
          ),
          if (_error != null) ...[
            const SizedBox(height: 10),
            SettingsInlineError(message: _error!),
          ],
        ],
      ),
    );
  }

  Future<void> _save(bool enabled) async {
    if (_saving) return;
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await ref
          .read(studioControllerProvider.notifier)
          .saveDeepSeekWebSearchSettings(
            DeepSeekWebSearchSettingsCommand(enabled: enabled),
          );
    } catch (error) {
      if (mounted) setState(() => _error = error.toString());
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }
}

String _modeLabel(BuildContext context, String mode) {
  return switch (mode) {
    'disabled' => context.l10n.settingsWebSearchModeDisabled,
    'cached' => context.l10n.settingsWebSearchModeCached,
    'indexed' => context.l10n.settingsWebSearchModeIndexed,
    'live' => context.l10n.settingsWebSearchModeLive,
    _ => mode,
  };
}

String _contextSizeLabel(BuildContext context, String size) {
  return switch (size) {
    'low' => context.l10n.settingsWebSearchContextLow,
    'medium' => context.l10n.settingsWebSearchContextMedium,
    'high' => context.l10n.settingsWebSearchContextHigh,
    _ => size,
  };
}

String _availabilityLabel(BuildContext context, String availability) {
  return switch (availability) {
    'available' => context.l10n.settingsWebSearchAvailable,
    'disabled' => context.l10n.settingsWebSearchDisabled,
    'providerUnsupported' => context.l10n.settingsWebSearchUnsupportedProvider,
    'modelUnsupported' => context.l10n.settingsWebSearchUnsupportedModel,
    'missingCredential' => context.l10n.settingsWebSearchMissingCredential,
    _ => availability,
  };
}

String _availabilityReason(BuildContext context, String availability) {
  return switch (availability) {
    'providerUnsupported' =>
      context.l10n.settingsWebSearchUnsupportedProviderReason,
    'modelUnsupported' => context.l10n.settingsWebSearchUnsupportedModelReason,
    'missingCredential' =>
      context.l10n.settingsWebSearchMissingCredentialReason,
    _ => availability,
  };
}

String _enabledLabel(BuildContext context, bool enabled) => enabled
    ? context.l10n.settingsDeepSeekWebSearchEnabled
    : context.l10n.settingsWebSearchDisabled;
