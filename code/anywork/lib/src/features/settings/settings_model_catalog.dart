import 'package:flutter/material.dart';

import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';

class ModelCatalogReadout extends StatelessWidget {
  const ModelCatalogReadout({super.key, required this.status});
  final ModelCatalogStatusView status;

  @override
  Widget build(BuildContext context) {
    final source = switch (status.source) {
      ModelCatalogSource.defaultDefinition =>
        context.l10n.settingsModelCatalogDefault,
      ModelCatalogSource.cached => context.l10n.settingsModelCatalogCached,
      ModelCatalogSource.online => context.l10n.settingsModelCatalogOnline,
    };
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(source),
        Text(context.l10n.settingsModelCatalogRestartHint),
        if (status.lastSuccessAt case final lastSuccessAt?
            when lastSuccessAt != status.checkedAt)
          Text(_formatCatalogTime(lastSuccessAt)),
        if (status.checkedAt case final checkedAt?)
          Text(
            '${context.l10n.settingsModelCatalogChecked}: '
            '${_formatCatalogTime(checkedAt)}',
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

String _formatCatalogTime(int unixSeconds) =>
    DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000)
        .toLocal()
        .toString();
