import 'package:flutter/material.dart';

import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';

class ModelCatalogReadout extends StatelessWidget {
  const ModelCatalogReadout({
    super.key,
    required this.status,
    required this.refreshing,
    required this.onRefresh,
  });
  final ModelCatalogStatusView status;
  final bool refreshing;
  final VoidCallback? onRefresh;

  @override
  Widget build(BuildContext context) {
    final busy = refreshing || status.probing;
    final source = switch (status.source) {
      ModelCatalogSource.defaultDefinition =>
        context.l10n.settingsModelCatalogDefault,
      ModelCatalogSource.cached => context.l10n.settingsModelCatalogCached,
      ModelCatalogSource.online => context.l10n.settingsModelCatalogOnline,
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Wrap(
          spacing: 12,
          runSpacing: 8,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            Text(source),
            TextButton.icon(
              key: StudioDriverKeys.modelCatalogRefresh,
              onPressed: busy ? null : onRefresh,
              icon: busy
                  ? const SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.refresh),
              label: Text(context.l10n.settingsModelCatalogRefresh),
            ),
          ],
        ),
        if (status.checkedAt case final checkedAt?)
          Text(
            '${context.l10n.settingsModelCatalogChecked}: '
            '${DateTime.fromMillisecondsSinceEpoch(checkedAt * 1000).toLocal()}',
          ),
        if (status.error != null)
          Text(
            context.l10n.settingsModelCatalogFailed,
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        if (status.cacheWarning != null)
          Text(context.l10n.settingsModelCatalogCacheWarning),
      ],
    );
  }
}
