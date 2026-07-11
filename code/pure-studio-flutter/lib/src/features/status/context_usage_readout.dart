import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import 'status_detail_popover.dart';

class ContextUsageReadout extends StatefulWidget {
  const ContextUsageReadout({required this.runtime, super.key});

  final SessionRuntimeView runtime;

  @override
  State<ContextUsageReadout> createState() => _ContextUsageReadoutState();
}

class _ContextUsageReadoutState extends State<ContextUsageReadout> {
  bool _hovering = false;
  bool _focused = false;

  @override
  Widget build(BuildContext context) {
    final runtime = widget.runtime;
    final progress = runtime.contextWindow <= 0
        ? 0.0
        : (runtime.contextTokens / runtime.contextWindow).clamp(0.0, 1.0);
    final percent = (progress * 100).round();
    return StatusDetailPopover(
      width: 314,
      semanticsLabel: context.l10n.statusContextLabel,
      semanticsValue: '$percent%',
      onFocusChange: (focused) => setState(() => _focused = focused),
      detailBuilder: (context) => _ContextDetail(
        runtime: runtime,
        progress: progress,
        progressColor: _progressColor(progress),
      ),
      child: Padding(
        padding: const EdgeInsets.only(right: 2),
        child: MouseRegion(
          onEnter: (_) => setState(() => _hovering = true),
          onExit: (_) => setState(() => _hovering = false),
          child: AnimatedContainer(
            duration: const Duration(milliseconds: 120),
            height: 26,
            padding: const EdgeInsets.symmetric(horizontal: 7),
            decoration: BoxDecoration(
              color: _hovering || _focused
                  ? context.studioPaper.withValues(alpha: 0.76)
                  : Colors.transparent,
              borderRadius: BorderRadius.circular(StudioRadii.xs),
            ),
            child: Text(
              '$percent%',
              maxLines: 1,
              style: context.text.labelSmall?.copyWith(
                color: _progressColor(progress),
                fontWeight: FontWeight.w600,
                height: 1,
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

class _ContextDetail extends StatelessWidget {
  const _ContextDetail({
    required this.runtime,
    required this.progress,
    required this.progressColor,
  });

  final SessionRuntimeView runtime;
  final double progress;
  final Color progressColor;

  @override
  Widget build(BuildContext context) {
    final percent = (progress * 100).round();
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        SizedBox.square(
          dimension: 72,
          child: Stack(
            alignment: Alignment.center,
            children: [
              CustomPaint(
                size: const Size.square(72),
                painter: _ContextUsagePainter(
                  progress: progress,
                  trackColor: context.studioLine,
                  progressColor: progressColor,
                  strokeWidth: 7,
                  radiusInset: 6,
                ),
              ),
              Text(
                '$percent%',
                style: context.text.titleMedium?.copyWith(
                  color: context.studioInk,
                  fontWeight: FontWeight.w700,
                ),
              ),
            ],
          ),
        ),
        const SizedBox(width: 14),
        Expanded(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              StatusDetailRow(
                label: context.l10n.statusContextLabel,
                value:
                    '${_formatCount(runtime.contextTokens)} / ${_formatCount(runtime.contextWindow)}',
              ),
              StatusDetailRow(
                label: context.l10n.statusTotalTokensLabel,
                value: _formatCount(runtime.totalTokens),
              ),
              if (runtime.model.isNotEmpty)
                StatusDetailRow(
                  label: context.l10n.statusModelLabel,
                  value: runtime.model,
                ),
            ],
          ),
        ),
      ],
    );
  }
}

class _ContextUsagePainter extends CustomPainter {
  const _ContextUsagePainter({
    required this.progress,
    required this.trackColor,
    required this.progressColor,
    this.strokeWidth = 2.6,
    this.radiusInset = 2,
  });

  final double progress;
  final Color trackColor;
  final Color progressColor;
  final double strokeWidth;
  final double radiusInset;

  @override
  void paint(Canvas canvas, Size size) {
    final rect = Offset.zero & size;
    final center = rect.center;
    final radius = math.min(size.width, size.height) / 2 - radiusInset;
    final trackPaint = Paint()
      ..color = trackColor.withValues(alpha: 0.74)
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.round
      ..strokeWidth = strokeWidth;
    final progressPaint = Paint()
      ..color = progressColor
      ..style = PaintingStyle.stroke
      ..strokeCap = StrokeCap.round
      ..strokeWidth = strokeWidth;

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
        progressColor != oldDelegate.progressColor ||
        strokeWidth != oldDelegate.strokeWidth ||
        radiusInset != oldDelegate.radiusInset;
  }
}

String _formatCount(int value) {
  final text = value.toString();
  final buffer = StringBuffer();
  for (var index = 0; index < text.length; index++) {
    if (index > 0 && (text.length - index) % 3 == 0) {
      buffer.write(',');
    }
    buffer.write(text[index]);
  }
  return buffer.toString();
}
