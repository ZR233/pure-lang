import 'package:flutter/material.dart';

import '../../shared/studio_chrome.dart';

class SettingsInlineError extends StatelessWidget {
  const SettingsInlineError({super.key, required this.message});

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

class SettingsEmptyMessage extends StatelessWidget {
  const SettingsEmptyMessage({
    super.key,
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
