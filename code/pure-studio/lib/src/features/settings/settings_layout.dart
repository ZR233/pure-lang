import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';

class SettingsHeader extends StatelessWidget {
  const SettingsHeader({
    super.key,
    required this.title,
    required this.subtitle,
    this.trailing,
  });

  final String title;
  final String subtitle;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 620;
        final titleBlock = Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              title,
              style: Theme.of(context).textTheme.headlineSmall?.copyWith(
                color: context.colors.onSurface,
                fontWeight: FontWeight.w500,
                height: 1.12,
              ),
            ),
            if (subtitle.isNotEmpty) ...[
              const SizedBox(height: 5),
              Tooltip(
                message: subtitle,
                child: Text(
                  subtitle,
                  maxLines: compact ? 2 : 1,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(context).textTheme.bodySmall
                      ?.copyWith(color: context.colors.onSurfaceVariant),
                ),
              ),
            ],
          ],
        );
        if (trailing == null) {
          return titleBlock;
        }
        if (compact) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              titleBlock,
              const SizedBox(height: 12),
              Align(alignment: Alignment.centerLeft, child: trailing!),
            ],
          );
        }
        return Row(
          crossAxisAlignment: CrossAxisAlignment.end,
          children: [
            Expanded(child: titleBlock),
            const SizedBox(width: 16),
            trailing!,
          ],
        );
      },
    );
  }
}

class SettingsSectionPanel extends StatelessWidget {
  const SettingsSectionPanel({
    super.key,
    required this.title,
    required this.children,
    this.trailing,
  });

  final String title;
  final List<Widget> children;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  title,
                  style: Theme.of(context).textTheme.titleSmall?.copyWith(
                    color: context.colors.onSurface,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              ?trailing,
            ],
          ),
          const SizedBox(height: 12),
          ...children,
        ],
      ),
    );
  }
}

class SettingsGroup extends StatelessWidget {
  const SettingsGroup({super.key, required this.children});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Colors.transparent,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (var index = 0; index < children.length; index++) ...[
            children[index],
            if (index < children.length - 1)
              Divider(height: 1, color: context.colors.outlineVariant),
          ],
        ],
      ),
    );
  }
}

/// Fixed page heading and actions with an independently scrolling content area.
class SettingsPane extends StatelessWidget {
  const SettingsPane({
    required this.header,
    required this.children,
    this.toolbar,
    this.maxWidth = 1040,
    super.key,
  });

  final Widget header;
  final List<Widget> children;
  final double maxWidth;
  final Widget? toolbar;

  @override
  Widget build(BuildContext context) => SettingsPageLayout(
    header: header,
    toolbar: toolbar,
    maxWidth: maxWidth,
    child: ListView(
      key: const ValueKey('settings-pane-scroll'),
      padding: const EdgeInsets.only(bottom: 24),
      children: children,
    ),
  );
}

class SettingsPageLayout extends StatelessWidget {
  const SettingsPageLayout({
    required this.header,
    required this.child,
    this.toolbar,
    this.footer,
    this.maxWidth = 1040,
    super.key,
  });

  final Widget header;
  final Widget child;
  final Widget? toolbar;
  final Widget? footer;
  final double maxWidth;

  @override
  Widget build(BuildContext context) => Align(
    alignment: Alignment.topCenter,
    child: ConstrainedBox(
      constraints: BoxConstraints(maxWidth: maxWidth),
      child: Padding(
        padding: EdgeInsets.symmetric(
          horizontal: MediaQuery.sizeOf(context).width < 700 ? 16 : 28,
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 22),
              child: header,
            ),
            if (toolbar != null)
              Padding(
                padding: const EdgeInsets.only(bottom: 16),
                child: toolbar!,
              ),
            Expanded(child: child),
            if (footer != null) ...[
              const Divider(height: 1),
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 12),
                child: footer!,
              ),
            ],
          ],
        ),
      ),
    ),
  );
}
