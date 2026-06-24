import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';

class ContextUsageRing extends StatelessWidget {
  const ContextUsageRing({required this.runtime, super.key});

  final SessionRuntimeView runtime;

  @override
  Widget build(BuildContext context) {
    final progress = runtime.contextWindow <= 0
        ? 0.0
        : (runtime.contextTokens / runtime.contextWindow).clamp(0.0, 1.0);
    return Tooltip(
      message: _contextTooltip(context, runtime),
      child: Padding(
        padding: const EdgeInsets.only(left: 2, right: 10),
        child: Semantics(
          label: context.l10n.statusContextLabel,
          value: '${(progress * 100).round()}%',
          child: SizedBox.square(
            dimension: 18,
            child: CustomPaint(
              painter: _ContextUsagePainter(
                progress: progress,
                trackColor: context.colors.outlineVariant,
                progressColor: _progressColor(progress),
              ),
            ),
          ),
        ),
      ),
    );
  }

  Color _progressColor(double progress) {
    if (progress >= 0.9) {
      return StudioColors.rose;
    }
    if (progress >= 0.72) {
      return StudioColors.ochre;
    }
    return StudioColors.clay;
  }
}

class _ContextUsagePainter extends CustomPainter {
  const _ContextUsagePainter({
    required this.progress,
    required this.trackColor,
    required this.progressColor,
  });

  final double progress;
  final Color trackColor;
  final Color progressColor;

  @override
  void paint(Canvas canvas, Size size) {
    final rect = Offset.zero & size;
    final center = rect.center;
    final radius = math.min(size.width, size.height) / 2 - 2;
    final trackPaint = Paint()
      ..color = trackColor.withValues(alpha: 0.74)
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.round
      ..strokeWidth = 2.6;
    final progressPaint = Paint()
      ..color = progressColor
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.round
      ..strokeWidth = 2.6;

    canvas.drawCircle(center, radius, trackPaint);
    if (progress <= 0) {
      return;
    }
    canvas.drawArc(
      Rect.fromCircle(center: center, radius: radius),
      -math.pi / 2,
      math.pi * 2 * progress,
      false,
      progressPaint,
    );
  }

  @override
  bool shouldRepaint(covariant _ContextUsagePainter oldDelegate) {
    return progress != oldDelegate.progress ||
        trackColor != oldDelegate.trackColor ||
        progressColor != oldDelegate.progressColor;
  }
}

String _contextTooltip(BuildContext context, SessionRuntimeView runtime) {
  final percent = runtime.contextWindow <= 0
      ? 0
      : ((runtime.contextTokens / runtime.contextWindow) * 100)
            .clamp(0, 100)
            .round();
  if (runtime.model.isEmpty) {
    return context.l10n.statusContextTooltipNoModel(
      runtime.contextTokens,
      runtime.contextWindow,
      percent,
      runtime.totalTokens,
    );
  }
  return context.l10n.statusContextTooltip(
    runtime.contextTokens,
    runtime.contextWindow,
    percent,
    runtime.totalTokens,
    runtime.model,
  );
}
