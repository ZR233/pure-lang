import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';

class SettingsTextEdit extends StatelessWidget {
  const SettingsTextEdit({
    super.key,
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
      style: context.text.bodyMedium?.copyWith(color: context.colors.onSurface),
      decoration: InputDecoration(labelText: label),
      onChanged: onChanged,
    );
  }
}

class SettingsResponsiveFieldGrid extends StatelessWidget {
  const SettingsResponsiveFieldGrid({super.key, required this.children});

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

class SettingsSearchField extends StatelessWidget {
  const SettingsSearchField({
    super.key,
    required this.hintText,
    required this.onChanged,
  });

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
        ),
        onChanged: onChanged,
      ),
    );
  }
}

class SettingsReadonlyField extends StatelessWidget {
  const SettingsReadonlyField({
    required this.label,
    required this.value,
    super.key,
  });

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 8),
      child: TextFormField(
        initialValue: value,
        readOnly: true,
        decoration: InputDecoration(labelText: label),
      ),
    );
  }
}

/// Vertical form rhythm shared by bounded editors.
class SettingsFieldStack extends StatelessWidget {
  const SettingsFieldStack({required this.children, super.key});
  final List<Widget> children;
  @override
  Widget build(BuildContext context) => Column(
    mainAxisSize: MainAxisSize.min,
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      for (final child in children)
        Padding(padding: const EdgeInsets.only(bottom: 12), child: child),
    ],
  );
}
