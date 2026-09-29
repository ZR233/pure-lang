import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_chrome.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_provider_auto_compact.dart';
import 'settings_provider_drafts.dart';

class ProviderModelReadout extends StatelessWidget {
  const ProviderModelReadout({
    super.key,
    required this.model,
    this.providerId = '',
    this.framed = false,
    this.onConnectionModeChanged,
    this.autoCompact,
    this.autoCompactEnabled = true,
    this.onAutoCompactChanged,
  });

  final ProviderModelView model;
  final String providerId;
  final bool framed;
  final ValueChanged<String>? onConnectionModeChanged;

  /// 该模型的三层压缩阈值视图；为 `null` 时不展示阈值区块。
  final ProviderModelAutoCompactView? autoCompact;

  /// 非空时提供压缩阈值编辑；`null` 表示只读展示。
  final ValueChanged<int?>? onAutoCompactChanged;

  /// 压缩阈值输入与滑块是否可用（保存中禁用）。
  final bool autoCompactEnabled;

  @override
  Widget build(BuildContext context) {
    final price = providerModelPriceLabel(model);
    final traits = model.capabilities;
    final inputCapabilities = model.inputCapabilities
        .map((capability) => context.modalityLabel(capability.modality))
        .toList();
    final outputCapabilities = model.outputModalities
        .map(context.modalityLabel)
        .toList();
    final row = Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
      child: Row(
        children: [
          StudioIconBadge(
            tone: StudioTone.neutral,
            icon: Icons.smart_toy_outlined,
            size: 30,
          ),
          const SizedBox(width: 11),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  model.displayName,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.labelLarge?.copyWith(
                    color: context.colors.onSurface,
                  ),
                ),
                Text(
                  model.slug,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.bodySmall?.copyWith(
                    color: context.colors.onSurfaceVariant,
                    fontFamily: 'Consolas',
                  ),
                ),
                Text(
                  '${context.modelProtocolLabel(model.wireProtocol)} · ${context.modelConnectionLabel(model.connectionMode)}',
                  style: context.text.labelSmall?.copyWith(
                    color: context.colors.onSurfaceVariant,
                  ),
                ),
                if (inputCapabilities.isNotEmpty)
                  Text(
                    inputCapabilities.join(' · '),
                    key: StudioDriverKeys.modelCapabilityTags(
                      providerId,
                      model.slug,
                    ),
                    style: context.text.labelSmall?.copyWith(
                      color: context.colors.onSurfaceVariant,
                    ),
                  ),
                if (outputCapabilities.isNotEmpty)
                  Text(
                    context.l10n.settingsModelOutputCapabilities(
                      outputCapabilities.join(' · '),
                    ),
                    style: context.text.labelSmall?.copyWith(
                      color: context.colors.onSurfaceVariant,
                    ),
                  ),
                if (traits.isNotEmpty)
                  Text(
                    traits.join(' · '),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: context.text.labelSmall?.copyWith(
                      color: context.colors.onSurfaceVariant,
                    ),
                  ),
              ],
            ),
          ),
          if (price.isNotEmpty) ...[
            const SizedBox(width: 10),
            Tooltip(
              message: model.priceTiers
                  .map(
                    (tier) =>
                        '${tier.label}\n${context.l10n.settingsPriceTierUnit(model.currency)} · ${context.l10n.settingsPriceInput}: ${tier.input} · ${context.l10n.settingsPriceOutput}: ${tier.output} · ${context.l10n.settingsPriceCacheRead}: ${tier.cacheRead ?? "—"} · ${context.l10n.settingsPriceCacheWrite}: ${tier.cacheWrite ?? "—"}',
                  )
                  .join('\n\n'),
              child: Text(
                price,
                style: context.text.labelSmall?.copyWith(
                  color: context.colors.onSurfaceVariant,
                ),
              ),
            ),
          ],
        ],
      ),
    );
    final content = Column(
      children: [
        row,
        if (model.supportedConnectionModes.length > 1) ...[
          const Divider(height: 1),
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 8, 12, 10),
            child: Align(
              alignment: Alignment.centerLeft,
              child: SegmentedButton<String>(
                key: StudioDriverKeys.providerModelConnectionMode(
                  providerId,
                  model.slug,
                ),
                segments: [
                  for (final mode in model.supportedConnectionModes)
                    ButtonSegment<String>(
                      value: mode,
                      label: KeyedSubtree(
                        key: StudioDriverKeys.providerModelConnectionModeOption(
                          providerId,
                          model.slug,
                          mode,
                        ),
                        child: Text(context.modelConnectionLabel(mode)),
                      ),
                    ),
                ],
                selected: {model.connectionMode},
                showSelectedIcon: false,
                onSelectionChanged: onConnectionModeChanged == null
                    ? null
                    : (selection) => onConnectionModeChanged!(selection.single),
              ),
            ),
          ),
        ],
        if (autoCompact != null) ...[
          const Divider(height: 1),
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 0, 12, 10),
            child: ProviderModelAutoCompactControl(
              limit: autoCompact!,
              providerId: providerId,
              modelSlug: model.slug,
              enabled: autoCompactEnabled,
              onOverrideChanged: onAutoCompactChanged,
            ),
          ),
        ],
      ],
    );
    if (!framed) {
      return content;
    }
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: StudioPanel(
        backgroundColor: Theme.of(context).colorScheme.surfaceContainerLowest,
        radius: StudioRadii.sm,
        child: content,
      ),
    );
  }
}
