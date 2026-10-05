import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../shared/studio_anchored_panel.dart';

/// 状态读数的悬停/点击详情弹层。
///
/// 锚定、开合、键盘入口与打开期间的 canonical 刷新由共享
/// [StudioAnchoredPanel] 承担；本组件只补充状态语义标签。
class StatusDetailPopover extends StatelessWidget {
  const StatusDetailPopover({
    required this.child,
    required this.detailBuilder,
    required this.semanticsLabel,
    required this.semanticsValue,
    this.onFocusChange,
    this.width = 300,
    super.key,
  });

  final Widget child;
  final WidgetBuilder detailBuilder;
  final String semanticsLabel;
  final String semanticsValue;
  final ValueChanged<bool>? onFocusChange;
  final double width;

  @override
  Widget build(BuildContext context) {
    return StudioAnchoredPanel(
      width: width,
      semanticsLabel: semanticsLabel,
      semanticsValue: semanticsValue,
      onFocusChange: onFocusChange,
      panelBuilder: detailBuilder,
      child: child,
    );
  }
}

class StatusDetailPanel extends StatelessWidget {
  const StatusDetailPanel({
    required this.title,
    required this.children,
    super.key,
  });

  final String title;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          title.toUpperCase(),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: context.text.labelSmall?.copyWith(
            color: context.colors.onSurfaceVariant,
            fontFamily: 'Consolas',
            fontSize: 9.5,
            fontWeight: FontWeight.w600,
            letterSpacing: 1,
          ),
        ),
        const SizedBox(height: 10),
        ...children,
      ],
    );
  }
}

class StatusDetailRow extends StatelessWidget {
  const StatusDetailRow({
    required this.label,
    required this.value,
    this.valueMaxLines = 1,
    this.valueKey,
    super.key,
  });

  final String label;
  final String value;
  final int valueMaxLines;
  final Key? valueKey;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3.5),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Text(
              label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: context.text.bodySmall?.copyWith(
                color: context.colors.onSurfaceVariant,
                height: 1.25,
              ),
            ),
          ),
          const SizedBox(width: 14),
          Flexible(
            flex: 2,
            child: Text(
              value,
              key: valueKey,
              maxLines: valueMaxLines,
              overflow: TextOverflow.ellipsis,
              textAlign: TextAlign.right,
              style: context.text.bodySmall?.copyWith(
                color: context.colors.onSurface,
                fontWeight: FontWeight.w600,
                height: 1.25,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class StatusDetailIconRow extends StatelessWidget {
  const StatusDetailIconRow({
    required this.icon,
    required this.title,
    required this.detail,
    this.tone = StudioTone.neutral,
    super.key,
  });

  final IconData icon;
  final String title;
  final String detail;
  final StudioTone tone;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox.square(
            dimension: 22,
            child: DecoratedBox(
              decoration: BoxDecoration(
                color: tone.background(context),
                borderRadius: BorderRadius.circular(6),
              ),
              child: Icon(icon, size: 13, color: tone.indicator(context)),
            ),
          ),
          const SizedBox(width: 9),
          Expanded(
            child: Text.rich(
              TextSpan(
                children: [
                  TextSpan(
                    text: title,
                    style: const TextStyle(fontWeight: FontWeight.w600),
                  ),
                  TextSpan(text: ' · $detail'),
                ],
              ),
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: context.text.bodySmall?.copyWith(
                color: context.colors.onSurface,
                height: 1.35,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class StatusDetailIconList extends StatelessWidget {
  const StatusDetailIconList({
    required this.icon,
    required this.title,
    required this.items,
    this.tone = StudioTone.neutral,
    this.itemKey,
    super.key,
  });

  final IconData icon;
  final String title;
  final List<String> items;
  final StudioTone tone;
  final Key Function(String item)? itemKey;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      container: true,
      label: title,
      value: items.join(', '),
      child: Padding(
        padding: const EdgeInsets.only(bottom: 8),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SizedBox.square(
              dimension: 22,
              child: DecoratedBox(
                decoration: BoxDecoration(
                  color: tone.background(context),
                  borderRadius: BorderRadius.circular(6),
                ),
                child: Icon(icon, size: 13, color: tone.indicator(context)),
              ),
            ),
            const SizedBox(width: 9),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    title,
                    style: context.text.bodySmall?.copyWith(
                      color: context.colors.onSurface,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  const SizedBox(height: 4),
                  for (final item in items)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 3),
                      child: Text(
                        '• $item',
                        key: itemKey?.call(item),
                        style: context.text.bodySmall?.copyWith(
                          color: context.colors.onSurface,
                          height: 1.3,
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
