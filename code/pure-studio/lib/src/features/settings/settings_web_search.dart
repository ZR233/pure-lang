import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import 'settings_common.dart';

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
  String? _error;

  @override
  void initState() {
    super.initState();
    _mode = widget.settings.configuredMode;
    _contextSize = widget.settings.contextSize;
    _domainsController = TextEditingController(
      text: widget.settings.allowedDomains.join(', '),
    );
    _countryController = TextEditingController(text: widget.settings.country);
    _regionController = TextEditingController(text: widget.settings.region);
    _cityController = TextEditingController(text: widget.settings.city);
    _timezoneController = TextEditingController(text: widget.settings.timezone);
  }

  @override
  void didUpdateWidget(covariant WebSearchSettingsCard oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (_saving || oldWidget.settings == widget.settings) {
      return;
    }
    _mode = widget.settings.configuredMode;
    _contextSize = widget.settings.contextSize;
    _replaceText(_domainsController, widget.settings.allowedDomains.join(', '));
    _replaceText(_countryController, widget.settings.country ?? '');
    _replaceText(_regionController, widget.settings.region ?? '');
    _replaceText(_cityController, widget.settings.city ?? '');
    _replaceText(_timezoneController, widget.settings.timezone ?? '');
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
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.travel_explore, color: context.studioInkSoft),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      context.l10n.settingsWebSearchTitle,
                      style: context.text.titleMedium?.copyWith(
                        color: context.studioInk,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      context.l10n.settingsWebSearchSubtitle,
                      style: context.text.bodySmall?.copyWith(
                        color: context.studioInkSoft,
                      ),
                    ),
                  ],
                ),
              ),
              Chip(
                visualDensity: VisualDensity.compact,
                label: Text(_availabilityLabel(context, settings)),
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
                    child: DropdownButtonFormField<String>(
                      initialValue: _mode,
                      decoration: InputDecoration(
                        labelText: context.l10n.settingsWebSearchMode,
                      ),
                      items: [
                        for (final mode in const [
                          'disabled',
                          'cached',
                          'indexed',
                          'live',
                        ])
                          DropdownMenuItem(
                            value: mode,
                            child: Text(_modeLabel(context, mode)),
                          ),
                      ],
                      onChanged: _saving
                          ? null
                          : (value) => setState(() => _mode = value ?? _mode),
                    ),
                  ),
                  SizedBox(
                    width: fieldWidth,
                    child: DropdownButtonFormField<String>(
                      initialValue: _contextSize ?? '',
                      decoration: InputDecoration(
                        labelText: context.l10n.settingsWebSearchContextSize,
                      ),
                      items: [
                        DropdownMenuItem(
                          value: '',
                          child: Text(context.l10n.settingsServiceDefault),
                        ),
                        for (final size in const ['low', 'medium', 'high'])
                          DropdownMenuItem(
                            value: size,
                            child: Text(_contextSizeLabel(context, size)),
                          ),
                      ],
                      onChanged: _saving
                          ? null
                          : (value) => setState(
                              () => _contextSize = value?.isEmpty == true
                                  ? null
                                  : value,
                            ),
                    ),
                  ),
                  SizedBox(
                    width: constraints.maxWidth,
                    child: TextField(
                      controller: _domainsController,
                      decoration: InputDecoration(
                        labelText: context.l10n.settingsWebSearchAllowedDomains,
                        hintText: context.l10n.settingsWebSearchDomainsHint,
                      ),
                    ),
                  ),
                  for (final field in [
                    (
                      controller: _countryController,
                      label: context.l10n.settingsWebSearchCountry,
                    ),
                    (
                      controller: _regionController,
                      label: context.l10n.settingsWebSearchRegion,
                    ),
                    (
                      controller: _cityController,
                      label: context.l10n.settingsWebSearchCity,
                    ),
                    (
                      controller: _timezoneController,
                      label: context.l10n.settingsWebSearchTimezone,
                    ),
                  ])
                    SizedBox(
                      width: fieldWidth,
                      child: TextField(
                        controller: field.controller,
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
                onPressed: _saving ? null : _save,
                icon: _saving
                    ? const SizedBox.square(
                        dimension: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.save_outlined),
                label: Text(context.l10n.settingsSaveWebSearch),
              ),
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
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await ref
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
              color: context.studioInkSoft,
            ),
          ),
          const SizedBox(height: 2),
          Text(
            value,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: context.text.bodyMedium?.copyWith(color: context.studioInk),
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
              Icon(Icons.public, color: context.studioInkSoft),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      context.l10n.settingsDeepSeekWebSearchTitle,
                      style: context.text.titleMedium?.copyWith(
                        color: context.studioInk,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    const SizedBox(height: 2),
                    Text(
                      context.l10n.settingsDeepSeekWebSearchSubtitle,
                      style: context.text.bodySmall?.copyWith(
                        color: context.studioInkSoft,
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
                label: context.l10n.settingsWebSearchProvider,
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
            _availabilityLabelForValue(
              context,
              settings.availability,
              selected: settings.selected,
            ),
            style: context.text.bodySmall?.copyWith(
              color: settings.isAvailable
                  ? context.studioInkSoft
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
    'indexed' => context.l10n.settingsWebSearchModeIndexed,
    'live' => context.l10n.settingsWebSearchModeLive,
    _ => context.l10n.settingsWebSearchModeCached,
  };
}

String _contextSizeLabel(BuildContext context, String size) {
  return switch (size) {
    'low' => context.l10n.settingsWebSearchContextLow,
    'high' => context.l10n.settingsWebSearchContextHigh,
    _ => context.l10n.settingsWebSearchContextMedium,
  };
}

String _availabilityLabel(
  BuildContext context,
  WebSearchSettingsView settings,
) {
  return _availabilityLabelForValue(
    context,
    settings.availability,
    selected: settings.selected,
  );
}

String _availabilityLabelForValue(
  BuildContext context,
  String availability, {
  required bool selected,
}) {
  return switch (availability) {
    'available' when !selected =>
      context.l10n.settingsWebSearchAvailableNotSelected,
    'available' => context.l10n.settingsWebSearchAvailable,
    'disabled' => context.l10n.settingsWebSearchDisabled,
    'providerUnsupported' => context.l10n.settingsWebSearchUnsupportedProvider,
    'modelUnsupported' => context.l10n.settingsWebSearchUnsupportedModel,
    _ => context.l10n.settingsWebSearchMissingCredential,
  };
}

String _availabilityReason(BuildContext context, String availability) {
  return switch (availability) {
    'providerUnsupported' =>
      context.l10n.settingsWebSearchUnsupportedProviderReason,
    'modelUnsupported' => context.l10n.settingsWebSearchUnsupportedModelReason,
    _ => context.l10n.settingsWebSearchMissingCredentialReason,
  };
}

String _enabledLabel(BuildContext context, bool enabled) => enabled
    ? context.l10n.settingsDeepSeekWebSearchEnabled
    : context.l10n.settingsWebSearchDisabled;
