import 'package:flutter/material.dart';

import '../app/theme/studio_tokens.dart';

class StudioCodeBlock extends StatelessWidget {
  const StudioCodeBlock({
    required this.text,
    this.language,
    this.maxHeight,
    this.margin = EdgeInsets.zero,
    this.padding = const EdgeInsets.all(12),
    this.horizontalScroll = true,
    this.backgroundColor,
    this.headerBackgroundColor,
    this.borderColor,
    this.textStyle,
    this.languageTextStyle,
    super.key,
  });

  final String text;
  final String? language;
  final double? maxHeight;
  final EdgeInsetsGeometry margin;
  final EdgeInsetsGeometry padding;
  final bool horizontalScroll;
  final Color? backgroundColor;
  final Color? headerBackgroundColor;
  final Color? borderColor;
  final TextStyle? textStyle;
  final TextStyle? languageTextStyle;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final textTheme = Theme.of(context).textTheme;
    final languageLabel = language?.trim() ?? '';
    final codeTextStyle =
        textStyle ??
        textTheme.bodyMedium?.copyWith(
          color: colors.onSurface,
          fontFamily: 'JetBrains Mono',
          fontFamilyFallback: const ['Consolas', 'monospace'],
          fontSize: (textTheme.bodyMedium?.fontSize ?? 14) * 0.92,
          height: 1.35,
        );

    Widget codeBody = Padding(
      padding: padding,
      child: SelectableText(text, style: codeTextStyle),
    );
    if (horizontalScroll) {
      codeBody = SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: codeBody,
      );
    }
    if (maxHeight != null) {
      codeBody = ConstrainedBox(
        constraints: BoxConstraints(maxHeight: maxHeight!),
        child: SingleChildScrollView(child: codeBody),
      );
    }

    return Container(
      width: double.infinity,
      margin: margin,
      decoration: BoxDecoration(
        color: backgroundColor ?? colors.surfaceContainerLow,
        borderRadius: BorderRadius.circular(StudioRadii.sm),
        border: Border.all(color: borderColor ?? colors.outlineVariant),
      ),
      clipBehavior: Clip.antiAlias,
      child: languageLabel.isEmpty
          ? codeBody
          : Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              mainAxisSize: MainAxisSize.min,
              children: [
                DecoratedBox(
                  decoration: BoxDecoration(
                    color:
                        headerBackgroundColor ?? colors.surfaceContainerHighest,
                  ),
                  child: Padding(
                    padding: const EdgeInsets.fromLTRB(12, 7, 12, 6),
                    child: Text(
                      languageLabel,
                      style:
                          languageTextStyle ??
                          textTheme.labelSmall?.copyWith(
                            color: colors.onSurfaceVariant,
                            fontFamily: 'JetBrains Mono',
                            fontFamilyFallback: const ['Consolas', 'monospace'],
                          ),
                    ),
                  ),
                ),
                codeBody,
              ],
            ),
    );
  }
}
