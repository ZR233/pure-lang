import 'package:flutter/material.dart';

/// Quiet menu label shared by the start page and the active agent composer.
/// The owning menu supplies focus, hover, semantics and activation behavior.
class StudioMenuLabel extends StatelessWidget {
  const StudioMenuLabel({
    required this.label,
    this.enabled = true,
    this.maxWidth = 180,
    super.key,
  });

  final String label;
  final bool enabled;
  final double maxWidth;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final color = colors.onSurfaceVariant.withValues(alpha: enabled ? 1 : 0.5);
    return ConstrainedBox(
      constraints: const BoxConstraints(minHeight: 32),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 8),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            ConstrainedBox(
              constraints: BoxConstraints(maxWidth: maxWidth),
              child: Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.labelMedium
                    ?.copyWith(color: color),
              ),
            ),
            const SizedBox(width: 5),
            Icon(Icons.keyboard_arrow_down, size: 14, color: color),
          ],
        ),
      ),
    );
  }
}
