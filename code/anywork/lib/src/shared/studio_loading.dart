import 'package:flutter/material.dart';

import '../app/theme/studio_tokens.dart';
import '../l10n/studio_l10n.dart';

/// Local loading feedback: refresh keeps its existing content interactive.
class StudioWorkspaceLoading extends StatelessWidget {
  const StudioWorkspaceLoading({super.key, this.hasContent = false});
  final bool hasContent;
  @override
  Widget build(BuildContext context) {
    final label = context.l10n.workspaceLoading;
    final progress = Semantics(
      liveRegion: true,
      child: Row(
        children: [
          SizedBox.square(
            dimension: 18,
            child: CircularProgressIndicator(
              strokeWidth: 2,
              color: context.statusColors.activeIndicator,
            ),
          ),
          const SizedBox(width: 12),
          Expanded(child: Text(label, style: context.text.bodyMedium)),
        ],
      ),
    );
    if (hasContent) {
      return IgnorePointer(
        child: Align(
          alignment: Alignment.topCenter,
          child: Material(
            color: context.colors.surfaceContainerLow,
            child: Padding(padding: const EdgeInsets.all(12), child: progress),
          ),
        ),
      );
    }
    return ColoredBox(
      color: context.colors.surface,
      child: Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(24),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 520),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                progress,
                const SizedBox(height: 28),
                for (final width in [1.0, .85, .62]) ...[
                  FractionallySizedBox(
                    widthFactor: width,
                    child: Container(
                      height: 12,
                      decoration: BoxDecoration(
                        color: context.colors.surfaceContainerHigh,
                        borderRadius: BorderRadius.circular(6),
                      ),
                    ),
                  ),
                  const SizedBox(height: 12),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}
