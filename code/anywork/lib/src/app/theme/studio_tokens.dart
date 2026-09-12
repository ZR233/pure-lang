import 'package:flutter/material.dart';

import 'studio_semantic_colors.dart';

export 'studio_semantic_colors.dart';

final class StudioRadii {
  const StudioRadii._();

  static const xs = 6.0;
  static const sm = 8.0;
  static const md = 8.0;
  static const lg = 8.0;
  static const pill = 999.0;
}

final class StudioLayout {
  const StudioLayout._();

  static const sidebarWidth = 232.0;
  static const compactRailWidth = 60.0;
  static const conversationWidth = 860.0;
  static const settingsNavigationWidth = 196.0;
  static const compactBreakpoint = 900.0;
}

final class StudioShadows {
  const StudioShadows._();

  static List<BoxShadow> soft(Color color) {
    return [
      BoxShadow(
        color: color.withValues(alpha: 0.07),
        blurRadius: 18,
        offset: const Offset(0, 8),
      ),
    ];
  }

  static List<BoxShadow> lifted(Color color) {
    return [
      BoxShadow(
        color: color.withValues(alpha: 0.11),
        blurRadius: 26,
        offset: const Offset(0, 14),
      ),
    ];
  }
}

extension StudioThemeTokens on BuildContext {
  ColorScheme get colors => Theme.of(this).colorScheme;
  TextTheme get text => Theme.of(this).textTheme;

  StudioSemanticColors get statusColors =>
      Theme.of(this).extension<StudioSemanticColors>()!;
}
