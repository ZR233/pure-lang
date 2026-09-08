import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_chrome.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_model_labels.dart';
import 'settings_provider_drafts.dart';

class ProviderModelReadout extends StatelessWidget {
  const ProviderModelReadout({
    super.key,
    required this.model,
    this.providerId = '',
    this.framed = false,
    this.onConnectionModeChanged,
  });

  final ProviderModelView model;
  final String providerId;
  final bool framed;
  final ValueChanged<String>? onConnectionModeChanged;

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
            icon: Icons.smart_toy_outlined,
            size: 30,
            backgroundColor: context.studioPaper2,
            foregroundColor: context.studioInkSoft,
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
                    color: context.studioInk,
                  ),
                ),
                Text(
                  model.slug,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.bodySmall?.copyWith(
                    color: context.studioInkSoft,
                    fontFamily: 'Consolas',
                  ),
                ),
                Text(
                  '${modelProtocolLabel(model.wireProtocol)} · ${modelConnectionLabel(model.connectionMode)}',
                  style: context.text.labelSmall?.copyWith(
                    color: context.studioInkSoft,
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
                      color: context.studioInkSoft,
                    ),
                  ),
                if (outputCapabilities.isNotEmpty)
                  Text(
                    context.l10n.settingsModelOutputCapabilities(
                      outputCapabilities.join(' · '),
                    ),
                    style: context.text.labelSmall?.copyWith(
                      color: context.studioInkSoft,
                    ),
                  ),
                if (traits.isNotEmpty)
                  Text(
                    traits.join(' · '),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: context.text.labelSmall?.copyWith(
                      color: context.studioInkSoft,
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
                        '${tier.label}\n${model.currency}/1M · ${context.l10n.settingsPriceInput}: ${tier.input} · ${context.l10n.settingsPriceOutput}: ${tier.output} · ${context.l10n.settingsPriceCacheRead}: ${tier.cacheRead ?? "—"} · ${context.l10n.settingsPriceCacheWrite}: ${tier.cacheWrite ?? "—"}',
                  )
                  .join('\n\n'),
              child: Text(
                price,
                style: context.text.labelSmall?.copyWith(
                  color: context.studioInkSoft,
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
                        child: Text(modelConnectionLabel(mode)),
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
