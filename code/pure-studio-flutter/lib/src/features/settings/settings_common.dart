part of 'settings_page.dart';

class _SettingsHeader extends StatelessWidget {
  const _SettingsHeader({
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
                color: context.studioInk,
                fontWeight: FontWeight.w500,
                height: 1.12,
              ),
            ),
            if (subtitle.isNotEmpty) ...[
              const SizedBox(height: 5),
              Text(
                subtitle,
                maxLines: compact ? 2 : 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(
                  context,
                ).textTheme.bodySmall?.copyWith(color: context.studioInkSoft),
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

class _SectionPanel extends StatelessWidget {
  const _SectionPanel({
    required this.title,
    required this.children,
    this.trailing,
  });

  final String title;
  final List<Widget> children;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    return StudioPanel(
      backgroundColor: Theme.of(context).colorScheme.surfaceContainerLowest,
      radius: StudioRadii.md,
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  title,
                  style: Theme.of(context).textTheme.titleSmall?.copyWith(
                    color: context.studioInk,
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

class _SettingsGroup extends StatelessWidget {
  const _SettingsGroup({required this.children});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return StudioPanel(
      backgroundColor: Theme.of(context).colorScheme.surfaceContainerLowest,
      radius: StudioRadii.md,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (var index = 0; index < children.length; index++) ...[
            children[index],
            if (index < children.length - 1)
              Divider(height: 1, color: context.studioLine),
          ],
        ],
      ),
    );
  }
}

class _TextEdit extends StatelessWidget {
  const _TextEdit({
    required this.label,
    required this.value,
    required this.onChanged,
    this.enabled = true,
    this.obscureText = false,
  });

  final String label;
  final String value;
  final ValueChanged<String> onChanged;
  final bool enabled;
  final bool obscureText;

  @override
  Widget build(BuildContext context) {
    return TextFormField(
      initialValue: value,
      enabled: enabled,
      obscureText: obscureText,
      style: context.text.bodyMedium?.copyWith(color: context.studioInk),
      decoration: InputDecoration(
        labelText: label,
        filled: true,
        fillColor: context.colors.surfaceContainerLowest,
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(StudioRadii.sm),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(StudioRadii.sm),
          borderSide: BorderSide(color: context.studioLine),
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(StudioRadii.sm),
          borderSide: const BorderSide(color: StudioColors.clay),
        ),
      ),
      onChanged: onChanged,
    );
  }
}

class _ResponsiveFieldGrid extends StatelessWidget {
  const _ResponsiveFieldGrid({required this.children});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final twoColumns = constraints.maxWidth >= 620;
        if (!twoColumns) {
          return Column(
            children: [
              for (final child in children)
                Padding(
                  padding: const EdgeInsets.only(bottom: 10),
                  child: child,
                ),
            ],
          );
        }
        return Wrap(
          spacing: 12,
          runSpacing: 10,
          children: [
            for (final child in children)
              SizedBox(width: (constraints.maxWidth - 12) / 2, child: child),
          ],
        );
      },
    );
  }
}

class _Readout extends StatelessWidget {
  const _Readout({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 7),
      child: Row(
        children: [
          SizedBox(
            width: 140,
            child: Text(
              label,
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                color: Theme.of(context).colorScheme.onSurfaceVariant,
              ),
            ),
          ),
          Expanded(
            child: Text(value, maxLines: 1, overflow: TextOverflow.ellipsis),
          ),
        ],
      ),
    );
  }
}

class _ProviderStatusChip extends StatelessWidget {
  const _ProviderStatusChip({required this.provider});

  final ProviderSettingsView provider;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final ready = provider.status == 'ready';
    return StudioPill(
      icon: ready ? Icons.check_circle_outline : Icons.error_outline,
      label: ready ? 'ready' : 'setup',
      backgroundColor: ready
          ? colors.secondaryContainer.withValues(alpha: 0.42)
          : colors.tertiaryContainer.withValues(alpha: 0.38),
      foregroundColor: ready ? colors.secondary : colors.tertiary,
      borderColor: ready
          ? colors.secondary.withValues(alpha: 0.24)
          : colors.tertiary.withValues(alpha: 0.22),
    );
  }
}

class _MiniMeta extends StatelessWidget {
  const _MiniMeta({required this.icon, required this.label});

  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    return StudioCompactChip(
      icon: icon,
      label: label,
      maxWidth: 220,
      backgroundColor: context.studioPaper2,
      borderColor: context.studioLine,
    );
  }
}

class _InfoPill extends StatelessWidget {
  const _InfoPill({required this.icon, required this.label});

  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    return StudioCompactChip(
      icon: icon,
      label: label,
      backgroundColor: context.studioPaper2,
      borderColor: context.studioLine,
      maxWidth: 220,
    );
  }
}

class _InlineError extends StatelessWidget {
  const _InlineError({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    return StudioNotice(
      icon: Icons.error_outline,
      message: message,
      tone: StudioNoticeTone.danger,
      padding: const EdgeInsets.all(10),
    );
  }
}

class _SettingsSearchField extends StatelessWidget {
  const _SettingsSearchField({required this.hintText, required this.onChanged});

  final String hintText;
  final ValueChanged<String> onChanged;

  @override
  Widget build(BuildContext context) {
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 380),
      child: TextField(
        decoration: InputDecoration(
          hintText: hintText,
          prefixIcon: const Icon(Icons.search, size: 18),
          filled: true,
          fillColor: context.colors.surfaceContainerLowest,
          isDense: true,
          border: OutlineInputBorder(
            borderRadius: BorderRadius.circular(StudioRadii.sm),
          ),
          enabledBorder: OutlineInputBorder(
            borderRadius: BorderRadius.circular(StudioRadii.sm),
            borderSide: BorderSide(color: context.studioLine),
          ),
          focusedBorder: OutlineInputBorder(
            borderRadius: BorderRadius.circular(StudioRadii.sm),
            borderSide: const BorderSide(color: StudioColors.clay),
          ),
        ),
        onChanged: onChanged,
      ),
    );
  }
}

class _SettingsRow extends StatelessWidget {
  const _SettingsRow({
    required this.icon,
    required this.title,
    this.subtitle,
    this.trailing,
    this.onTap,
  });

  final IconData icon;
  final String title;
  final String? subtitle;
  final Widget? trailing;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    const accent = StudioColors.clay;
    final content = Padding(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      child: Row(
        children: [
          StudioIconBadge(
            icon: icon,
            size: 32,
            backgroundColor: accent.withValues(alpha: 0.14),
            foregroundColor: accent,
          ),
          const SizedBox(width: 13),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  title,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.bodyMedium?.copyWith(
                    color: context.studioInk,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                if (subtitle?.isNotEmpty ?? false) ...[
                  const SizedBox(height: 2),
                  Text(
                    subtitle!,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: context.text.bodySmall?.copyWith(
                      color: context.studioInkSoft.withValues(alpha: 0.72),
                      fontFamily: subtitle!.contains('/') ? 'Consolas' : null,
                    ),
                  ),
                ],
              ],
            ),
          ),
          if (trailing != null) ...[const SizedBox(width: 12), trailing!],
        ],
      ),
    );
    return Material(
      color: Colors.transparent,
      child: onTap == null ? content : InkWell(onTap: onTap, child: content),
    );
  }
}

class _SettingsToggleRow extends StatelessWidget {
  const _SettingsToggleRow({
    required this.icon,
    required this.title,
    required this.value,
    required this.onChanged,
    this.subtitle,
  });

  final IconData icon;
  final String title;
  final String? subtitle;
  final bool value;
  final ValueChanged<bool> onChanged;

  @override
  Widget build(BuildContext context) {
    return _SettingsRow(
      icon: icon,
      title: title,
      subtitle: subtitle,
      trailing: Switch(value: value, onChanged: onChanged),
      onTap: () => onChanged(!value),
    );
  }
}

class _EmptySettingsMessage extends StatelessWidget {
  const _EmptySettingsMessage({
    required this.icon,
    required this.title,
    required this.body,
  });

  final IconData icon;
  final String title;
  final String body;

  @override
  Widget build(BuildContext context) {
    return StudioNotice(
      icon: icon,
      title: title,
      message: body,
      padding: const EdgeInsets.all(16),
    );
  }
}
