import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../domain/models/studio_models.dart';
import '../l10n/studio_l10n.dart';
import 'studio_driver_keys.dart';
import 'upward_popup_menu.dart';

/// A catalog model paired with its provider-instance identity for selection.
///
/// Display names are presentation only. Equality deliberately follows the
/// canonical route identity: provider ID plus model slug.
class ModelSelectionOption {
  const ModelSelectionOption({
    required this.providerId,
    required this.providerName,
    required this.descriptor,
  });

  final String providerId;
  final String providerName;
  final ProviderModelView descriptor;

  String get model => descriptor.slug;

  String get displayName =>
      descriptor.displayName.isEmpty ? descriptor.slug : descriptor.displayName;

  List<String> get reasoningEfforts => descriptor.reasoningEfforts;

  String get defaultReasoningEffort => descriptor.defaultReasoningEffort;

  List<ModelInputCapabilityView> get inputCapabilities =>
      descriptor.inputCapabilities;

  @override
  bool operator ==(Object other) =>
      other is ModelSelectionOption &&
      other.providerId == providerId &&
      other.model == model;

  @override
  int get hashCode => Object.hash(providerId, model);
}

/// Builds presentation options while leaving each caller in charge of which
/// catalog models (including any existing fallback) are selectable.
List<ModelSelectionOption> buildModelSelectionOptions(
  List<ProviderSettingsView> providers, {
  required List<ProviderModelView> Function(ProviderSettingsView provider)
  modelsForProvider,
}) {
  return [
    for (final provider in providers)
      for (final model in modelsForProvider(provider))
        ModelSelectionOption(
          providerId: provider.id,
          providerName: provider.name,
          descriptor: model,
        ),
  ];
}

typedef ModelSelectionOptionKeyBuilder = Key Function(
  ModelSelectionOption option,
);

/// Shared provider-grouped model picker for session routes and Agent settings.
///
/// This widget only presents the supplied choices and reports a selection;
/// it never chooses a fallback route or persists the result.
class ModelRouteSelector extends StatelessWidget {
  const ModelRouteSelector({
    required this.options,
    required this.providerId,
    required this.model,
    required this.onSelected,
    required this.fieldLabel,
    required this.selectorKey,
    this.unresolvedLabel,
    this.showUnresolvedOption = false,
    this.enabled = true,
    this.tooltip,
    this.onBlockedTap,
    this.compact = false,
    this.compactMaxWidth = 180,
    this.optionKeyBuilder,
    this.errorText,
    super.key,
  });

  final List<ModelSelectionOption> options;
  final String providerId;
  final String model;
  final ValueChanged<ModelSelectionOption> onSelected;
  final String fieldLabel;
  final Key selectorKey;
  final String? unresolvedLabel;
  final bool showUnresolvedOption;
  final bool enabled;
  final String? tooltip;
  final VoidCallback? onBlockedTap;
  final bool compact;
  final double compactMaxWidth;
  final ModelSelectionOptionKeyBuilder? optionKeyBuilder;
  final String? errorText;

  @override
  Widget build(BuildContext context) {
    final selected = _findOption(options, providerId, model);
    final selectedLabel = selected?.displayName ?? unresolvedLabel ?? model;
    final canSelect = enabled && options.isNotEmpty;
    final menuSize = _menuSize(context);
    final contentWidth = math.max(0.0, menuSize.width - 40);

    if (compact) {
      return UpwardPopupMenu<ModelSelectionOption>(
        key: selectorKey,
        tooltip: tooltip ?? fieldLabel,
        initialValue: selected,
        enabled: canSelect,
        onBlockedTap: canSelect ? null : onBlockedTap,
        constraints: BoxConstraints(
          maxWidth: menuSize.width,
          maxHeight: menuSize.height,
        ),
        onSelected: onSelected,
        itemBuilder: (context) => _popupItems(
          context,
          contentWidth: contentWidth,
          selected: selected,
        ),
        child: StudioMenuLabel(
          label: selectedLabel,
          enabled: canSelect,
          maxWidth: compactMaxWidth,
        ),
      );
    }

    return MenuAnchor(
      style: MenuStyle(
        maximumSize: WidgetStatePropertyAll(menuSize),
        padding: const WidgetStatePropertyAll(
          EdgeInsets.symmetric(vertical: 6),
        ),
      ),
      menuChildren: _menuChildren(
        context,
        contentWidth: contentWidth,
        selected: selected,
        canSelect: canSelect,
      ),
      builder: (context, controller, child) {
        return Tooltip(
          message: tooltip ?? fieldLabel,
          child: Semantics(
            button: true,
            enabled: canSelect,
            label: [
              fieldLabel,
              selectedLabel,
            ].where((part) => part.isNotEmpty).join(': '),
            child: InkWell(
              key: selectorKey,
              onTap: canSelect
                  ? () => controller.isOpen
                        ? controller.close()
                        : controller.open()
                  : null,
              borderRadius: BorderRadius.circular(4),
              child: InputDecorator(
                isEmpty: selectedLabel.isEmpty,
                isFocused: controller.isOpen,
                decoration: InputDecoration(
                  labelText: fieldLabel,
                  isDense: true,
                  enabled: canSelect,
                  errorText: errorText,
                  suffixIcon: const Icon(Icons.arrow_drop_down),
                ),
                child: Text(
                  selectedLabel,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ),
          ),
        );
      },
    );
  }

  List<PopupMenuEntry<ModelSelectionOption>> _popupItems(
    BuildContext context, {
    required double contentWidth,
    required ModelSelectionOption? selected,
  }) {
    final items = <PopupMenuEntry<ModelSelectionOption>>[];
    if (showUnresolvedOption && selected == null && unresolvedLabel != null) {
      items.add(
        PopupMenuItem<ModelSelectionOption>(
          enabled: false,
          height: 42,
          child: SizedBox(
            width: contentWidth,
            child: _unresolvedEntry(context, unresolvedLabel!),
          ),
        ),
      );
    }
    for (final group in _groups()) {
      items.add(
        PopupMenuItem<ModelSelectionOption>(
          enabled: false,
          height: 38,
          child: SizedBox(
            width: contentWidth,
            child: _providerHeading(context, group.name),
          ),
        ),
      );
      for (final option in group.options) {
        items.add(
          PopupMenuItem<ModelSelectionOption>(
            key: _optionKey(option),
            value: option,
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
            child: SizedBox(
              width: contentWidth,
              child: _modelOptionContents(
                context,
                option,
                selected: option == selected,
              ),
            ),
          ),
        );
      }
    }
    return items;
  }

  List<Widget> _menuChildren(
    BuildContext context, {
    required double contentWidth,
    required ModelSelectionOption? selected,
    required bool canSelect,
  }) {
    final children = <Widget>[];
    if (showUnresolvedOption && selected == null && unresolvedLabel != null) {
      children.add(
        SizedBox(
          width: contentWidth,
          child: _unresolvedEntry(context, unresolvedLabel!),
        ),
      );
    }
    for (final group in _groups()) {
      children.add(
        SizedBox(
          width: contentWidth,
          child: _providerHeading(context, group.name),
        ),
      );
      for (final option in group.options) {
        children.add(
          MenuItemButton(
            key: _optionKey(option),
            onPressed: canSelect ? () => onSelected(option) : null,
            child: SizedBox(
              width: contentWidth,
              child: _modelOptionContents(
                context,
                option,
                selected: option == selected,
              ),
            ),
          ),
        );
      }
    }
    return children;
  }

  List<_ProviderModelGroup> _groups() {
    final groups = <String, _ProviderModelGroup>{};
    for (final option in options) {
      groups
          .putIfAbsent(
            option.providerId,
            () => _ProviderModelGroup(option.providerName),
          )
          .options
          .add(option);
    }
    return groups.values.toList(growable: false);
  }

  Key _optionKey(ModelSelectionOption option) =>
      optionKeyBuilder?.call(option) ??
      StudioDriverKeys.modelOption(option.providerId, option.model);

  static ModelSelectionOption? _findOption(
    List<ModelSelectionOption> options,
    String providerId,
    String model,
  ) {
    return options
        .where(
          (option) => option.providerId == providerId && option.model == model,
        )
        .firstOrNull;
  }

  static Size _menuSize(BuildContext context) {
    final window = MediaQuery.sizeOf(context);
    return Size(
      math.min(420.0, math.max(0.0, window.width - 24)),
      math.min(480.0, math.max(0.0, window.height - 32)),
    );
  }

  static Widget _providerHeading(BuildContext context, String name) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(8, 8, 8, 4),
      child: Semantics(
        header: true,
        child: Text(
          name,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: Theme.of(context).textTheme.labelMedium?.copyWith(
            color: Theme.of(context).colorScheme.onSurfaceVariant,
            fontWeight: FontWeight.w600,
          ),
        ),
      ),
    );
  }

  static Widget _unresolvedEntry(BuildContext context, String label) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
      child: Text(
        label,
        maxLines: 2,
        overflow: TextOverflow.ellipsis,
        style: Theme.of(context).textTheme.bodySmall
            ?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant),
      ),
    );
  }

  static Widget _modelOptionContents(
    BuildContext context,
    ModelSelectionOption option, {
    required bool selected,
  }) {
    final modalities = option.inputCapabilities
        .map((capability) => capability.modality)
        .toSet();
    return Semantics(
      selected: selected,
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 20,
            child: selected
                ? Icon(
                    Icons.check,
                    size: 16,
                    color: Theme.of(context).colorScheme.primary,
                  )
                : null,
          ),
          const SizedBox(width: 6),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  option.displayName,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                if (modalities.isNotEmpty) ...[
                  const SizedBox(height: 4),
                  Wrap(
                    key: StudioDriverKeys.modelCapabilityTags(
                      option.providerId,
                      option.model,
                    ),
                    spacing: 5,
                    runSpacing: 4,
                    children: [
                      for (final modality in modalities)
                        _CapabilityCard(modality: modality),
                    ],
                  ),
                ],
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _ProviderModelGroup {
  _ProviderModelGroup(this.name);

  final String name;
  final List<ModelSelectionOption> options = [];
}

class _CapabilityCard extends StatelessWidget {
  const _CapabilityCard({required this.modality});

  final ModelModalityView modality;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final textStyle = Theme.of(context).textTheme.labelSmall;
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 128),
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: colors.surfaceContainerHigh,
          border: Border.all(color: colors.outlineVariant),
          borderRadius: BorderRadius.circular(6),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 3),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(
                _modalityIcon(modality),
                size: 13,
                color: colors.onSurfaceVariant,
              ),
              const SizedBox(width: 4),
              Flexible(
                child: Text(
                  context.modalityLabel(modality),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: textStyle?.copyWith(color: colors.onSurfaceVariant),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  IconData _modalityIcon(ModelModalityView modality) => switch (modality) {
    ModelModalityView.text => Icons.text_fields,
    ModelModalityView.image => Icons.image_outlined,
    ModelModalityView.audio => Icons.graphic_eq,
    ModelModalityView.video => Icons.movie_outlined,
    ModelModalityView.file => Icons.attach_file,
  };
}
