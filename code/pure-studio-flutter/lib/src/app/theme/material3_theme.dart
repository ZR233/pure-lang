import 'package:flutter/material.dart';

import 'studio_tokens.dart';

ThemeData pureStudioTheme(Brightness brightness) {
  final light = brightness == Brightness.light;
  final scheme = ColorScheme(
    brightness: brightness,
    primary: light ? StudioColors.clay : StudioColors.clay,
    onPrimary: Colors.white,
    primaryContainer: light ? StudioColors.claySoft : const Color(0xff693828),
    onPrimaryContainer: light ? StudioColors.clayDeep : StudioColors.claySoft,
    secondary: light ? StudioColors.sage : const Color(0xff9cb98f),
    onSecondary: Colors.white,
    secondaryContainer: light ? StudioColors.sageSoft : const Color(0xff33422d),
    onSecondaryContainer: light ? StudioColors.sage : StudioColors.sageSoft,
    tertiary: light ? StudioColors.ochre : const Color(0xffd9b06d),
    onTertiary: Colors.white,
    tertiaryContainer: light
        ? const Color(0xffffefd4)
        : const Color(0xff563b19),
    onTertiaryContainer: light
        ? const Color(0xff6e4d20)
        : const Color(0xffffefd4),
    error: StudioColors.rose,
    onError: Colors.white,
    errorContainer: light ? const Color(0xffffe1e5) : const Color(0xff653039),
    onErrorContainer: light ? StudioColors.rose : const Color(0xffffe1e5),
    surface: light ? StudioColors.paper : StudioColors.darkPaper,
    onSurface: light ? StudioColors.ink : StudioColors.darkInk,
    surfaceContainerLowest: light
        ? StudioColors.white
        : const Color(0xff191614),
    surfaceContainerLow: light ? StudioColors.paper2 : StudioColors.darkPaper2,
    surfaceContainer: light ? StudioColors.paper2 : StudioColors.darkPaper2,
    surfaceContainerHigh: light ? StudioColors.paper3 : StudioColors.darkPaper3,
    surfaceContainerHighest: light
        ? const Color(0xffe7ded7)
        : const Color(0xff40362f),
    onSurfaceVariant: light ? StudioColors.inkSoft : StudioColors.darkInkSoft,
    outline: light ? StudioColors.line2 : StudioColors.darkLine,
    outlineVariant: light ? StudioColors.line : StudioColors.darkLine,
    shadow: Colors.black,
    scrim: Colors.black,
    inverseSurface: light ? StudioColors.ink : StudioColors.darkInk,
    onInverseSurface: light ? StudioColors.paper : StudioColors.darkPaper,
    inversePrimary: light ? const Color(0xffffb097) : StudioColors.clayDeep,
  );
  return ThemeData(
    colorScheme: scheme,
    useMaterial3: true,
    visualDensity: VisualDensity.compact,
    scaffoldBackgroundColor: scheme.surface,
    fontFamilyFallback: const ['Inter', 'Noto Sans SC', 'Segoe UI'],
    textTheme: Typography.material2021().black.apply(
      bodyColor: scheme.onSurface,
      displayColor: scheme.onSurface,
    ),
    appBarTheme: AppBarTheme(
      elevation: 0,
      scrolledUnderElevation: 0,
      backgroundColor: scheme.surface,
      foregroundColor: scheme.onSurface,
      centerTitle: false,
      titleTextStyle: TextStyle(
        color: scheme.onSurface,
        fontSize: 18,
        fontWeight: FontWeight.w600,
      ),
    ),
    navigationRailTheme: NavigationRailThemeData(
      backgroundColor: scheme.surfaceContainerLow,
      selectedIconTheme: IconThemeData(color: scheme.primary),
      selectedLabelTextStyle: TextStyle(color: scheme.primary),
    ),
    dividerTheme: DividerThemeData(
      color: scheme.outlineVariant.withValues(alpha: 0.7),
      thickness: 0.7,
    ),
    listTileTheme: ListTileThemeData(
      iconColor: scheme.onSurfaceVariant,
      selectedColor: scheme.primary,
      selectedTileColor: scheme.surfaceContainerHighest,
    ),
    cardTheme: CardThemeData(
      elevation: 0,
      margin: EdgeInsets.zero,
      color: scheme.surface,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(StudioRadii.sm),
        side: BorderSide(color: scheme.outlineVariant.withValues(alpha: 0.78)),
      ),
    ),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: scheme.surfaceContainerLowest,
      isDense: true,
      border: OutlineInputBorder(
        borderRadius: BorderRadius.circular(StudioRadii.sm),
        borderSide: BorderSide(color: scheme.outlineVariant),
      ),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(StudioRadii.sm),
        borderSide: BorderSide(color: scheme.outlineVariant),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(StudioRadii.sm),
        borderSide: BorderSide(color: scheme.primary.withValues(alpha: 0.72)),
      ),
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        backgroundColor: scheme.primary,
        foregroundColor: scheme.onPrimary,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(StudioRadii.sm),
        ),
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(
        foregroundColor: scheme.onSurfaceVariant,
        side: BorderSide(color: scheme.outlineVariant),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(StudioRadii.sm),
        ),
      ),
    ),
    tooltipTheme: const TooltipThemeData(
      waitDuration: Duration(milliseconds: 350),
    ),
    iconButtonTheme: IconButtonThemeData(
      style: IconButton.styleFrom(
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(StudioRadii.sm),
        ),
      ),
    ),
  );
}
