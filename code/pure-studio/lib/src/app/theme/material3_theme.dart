import 'package:flutter/material.dart';
import 'package:gpt_markdown/gpt_markdown.dart';

import 'siamese_scheme.dart';
import 'studio_tokens.dart';

/// The single, system-independent Studio theme, including third-party content.
ThemeData pureStudioTheme() => _studioTheme;

// Stable identity avoids spurious AnimatedTheme transitions on unrelated rebuilds.
final ThemeData _studioTheme = _createStudioTheme();

ThemeData _createStudioTheme() {
  final scheme = siameseColorScheme();
  final shape = RoundedRectangleBorder(
    borderRadius: BorderRadius.circular(StudioRadii.sm),
  );
  final border = OutlineInputBorder(
    borderRadius: BorderRadius.circular(StudioRadii.sm),
    borderSide: BorderSide(color: scheme.outline),
  );
  final text = Typography.material2021().black.apply(
    bodyColor: scheme.onSurface,
    displayColor: scheme.onSurface,
  );
  Color? stateOverlay(Set<WidgetState> states) {
    if (states.contains(WidgetState.disabled)) return Colors.transparent;
    if (states.contains(WidgetState.pressed)) {
      return scheme.primary.withValues(alpha: 0.12);
    }
    if (states.contains(WidgetState.focused)) {
      return scheme.primary.withValues(alpha: 0.12);
    }
    if (states.contains(WidgetState.hovered)) {
      return scheme.primary.withValues(alpha: 0.06);
    }
    return null;
  }

  BorderSide focusBorder(Set<WidgetState> states) => BorderSide(
    color: states.contains(WidgetState.focused)
        ? scheme.primary
        : Colors.transparent,
    width: 2,
  );
  final button = ButtonStyle(
    shape: WidgetStatePropertyAll(shape),
    side: WidgetStateProperty.resolveWith(focusBorder),
    overlayColor: WidgetStateProperty.resolveWith(stateOverlay),
  );
  final menuStyle = MenuStyle(
    backgroundColor: WidgetStatePropertyAll(scheme.surfaceContainerLowest),
    surfaceTintColor: const WidgetStatePropertyAll(Colors.transparent),
    side: WidgetStatePropertyAll(BorderSide(color: scheme.outlineVariant)),
    shape: WidgetStatePropertyAll(shape),
    elevation: const WidgetStatePropertyAll(2),
  );
  return ThemeData(
    colorScheme: scheme,
    useMaterial3: true,
    visualDensity: VisualDensity.compact,
    scaffoldBackgroundColor: scheme.surface,
    canvasColor: scheme.surface,
    fontFamilyFallback: const ['Inter', 'Noto Sans SC', 'Segoe UI'],
    textTheme: text,
    iconTheme: IconThemeData(color: scheme.onSurfaceVariant),
    hoverColor: scheme.primary.withValues(alpha: 0.06),
    focusColor: scheme.primary.withValues(alpha: 0.12),
    highlightColor: scheme.primary.withValues(alpha: 0.10),
    splashColor: scheme.primary.withValues(alpha: 0.12),
    disabledColor: scheme.onSurface.withValues(alpha: 0.38),
    appBarTheme: AppBarTheme(
      elevation: 0,
      scrolledUnderElevation: 0,
      surfaceTintColor: Colors.transparent,
      backgroundColor: scheme.surface,
      foregroundColor: scheme.onSurface,
      centerTitle: false,
      titleTextStyle: text.titleLarge?.copyWith(
        color: scheme.onSurface,
        fontSize: 18,
        fontWeight: FontWeight.w600,
      ),
    ),
    navigationRailTheme: NavigationRailThemeData(
      backgroundColor: scheme.surfaceContainer,
      indicatorColor: scheme.surfaceContainerHigh,
      selectedIconTheme: IconThemeData(color: scheme.primary),
      selectedLabelTextStyle: TextStyle(color: scheme.onSurface),
    ),
    navigationBarTheme: NavigationBarThemeData(
      backgroundColor: scheme.surfaceContainer,
      indicatorColor: scheme.surfaceContainerHigh,
      surfaceTintColor: Colors.transparent,
    ),
    dividerTheme: DividerThemeData(
      color: scheme.outlineVariant,
      thickness: 0.7,
    ),
    listTileTheme: ListTileThemeData(
      iconColor: scheme.onSurfaceVariant,
      textColor: scheme.onSurface,
      selectedColor: scheme.onSurface,
      selectedTileColor: scheme.surfaceContainerHigh,
    ),
    dialogTheme: DialogThemeData(
      backgroundColor: scheme.surface,
      surfaceTintColor: Colors.transparent,
      barrierColor: scheme.scrim.withValues(alpha: 0.32),
      shape: shape,
    ),
    drawerTheme: DrawerThemeData(
      scrimColor: scheme.scrim.withValues(alpha: 0.32),
      backgroundColor: scheme.surfaceContainer,
      surfaceTintColor: Colors.transparent,
    ),
    bottomSheetTheme: BottomSheetThemeData(
      backgroundColor: scheme.surface,
      modalBackgroundColor: scheme.surface,
      surfaceTintColor: Colors.transparent,
      modalBarrierColor: scheme.scrim.withValues(alpha: 0.32),
    ),
    cardTheme: CardThemeData(
      elevation: 0,
      margin: EdgeInsets.zero,
      color: scheme.surface,
      surfaceTintColor: Colors.transparent,
      shape: shape.copyWith(side: BorderSide(color: scheme.outlineVariant)),
    ),
    inputDecorationTheme: InputDecorationThemeData(
      filled: true,
      fillColor: scheme.surfaceContainerLowest,
      isDense: true,
      labelStyle: TextStyle(color: scheme.onSurfaceVariant),
      hintStyle: TextStyle(color: scheme.onSurfaceVariant),
      border: border,
      enabledBorder: border,
      disabledBorder: border.copyWith(
        borderSide: BorderSide(color: scheme.outlineVariant),
      ),
      focusedBorder: border.copyWith(
        borderSide: BorderSide(color: scheme.primary, width: 2),
      ),
      errorBorder: border.copyWith(borderSide: BorderSide(color: scheme.error)),
      focusedErrorBorder: border.copyWith(
        borderSide: BorderSide(color: scheme.error, width: 2),
      ),
    ),
    textSelectionTheme: TextSelectionThemeData(
      cursorColor: scheme.primary,
      selectionColor: scheme.primaryContainer,
      selectionHandleColor: scheme.primary,
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: button.copyWith(
        overlayColor: WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.disabled)) return Colors.transparent;
          if (states.contains(WidgetState.focused) ||
              states.contains(WidgetState.pressed)) {
            return scheme.onPrimary.withValues(alpha: 0.16);
          }
          if (states.contains(WidgetState.hovered)) {
            return scheme.onPrimary.withValues(alpha: 0.08);
          }
          return null;
        }),
      ),
    ),
    elevatedButtonTheme: ElevatedButtonThemeData(
      style: button.copyWith(
        backgroundColor: WidgetStateProperty.resolveWith(
          (states) => states.contains(WidgetState.disabled)
              ? null
              : scheme.primaryContainer,
        ),
        foregroundColor: WidgetStateProperty.resolveWith(
          (states) => states.contains(WidgetState.disabled)
              ? null
              : scheme.onPrimaryContainer,
        ),
        surfaceTintColor: const WidgetStatePropertyAll(Colors.transparent),
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: button.copyWith(
        foregroundColor: WidgetStateProperty.resolveWith(
          (states) => states.contains(WidgetState.disabled)
              ? null
              : scheme.onSurfaceVariant,
        ),
        side: WidgetStateProperty.resolveWith(
          (states) => BorderSide(
            color: states.contains(WidgetState.disabled)
                ? scheme.outlineVariant
                : states.contains(WidgetState.focused)
                ? scheme.primary
                : scheme.outline,
            width: states.contains(WidgetState.focused) ? 2 : 1,
          ),
        ),
      ),
    ),
    textButtonTheme: TextButtonThemeData(style: button),
    iconButtonTheme: IconButtonThemeData(style: button),
    checkboxTheme: CheckboxThemeData(
      side: BorderSide(color: scheme.outline, width: 2),
    ),
    radioTheme: RadioThemeData(
      overlayColor: WidgetStateProperty.resolveWith(stateOverlay),
    ),
    switchTheme: SwitchThemeData(
      overlayColor: WidgetStateProperty.resolveWith(stateOverlay),
    ),
    progressIndicatorTheme: ProgressIndicatorThemeData(
      color: scheme.primary,
      linearTrackColor: scheme.surfaceContainerHighest,
      circularTrackColor: Colors.transparent,
    ),
    dropdownMenuTheme: DropdownMenuThemeData(menuStyle: menuStyle),
    menuTheme: MenuThemeData(style: menuStyle),
    menuBarTheme: MenuBarThemeData(style: menuStyle),
    menuButtonTheme: MenuButtonThemeData(style: button),
    popupMenuTheme: PopupMenuThemeData(
      color: scheme.surfaceContainerLowest,
      surfaceTintColor: Colors.transparent,
      shape: shape.copyWith(side: BorderSide(color: scheme.outlineVariant)),
      textStyle: text.bodyMedium?.copyWith(color: scheme.onSurface),
    ),
    chipTheme: ChipThemeData(
      backgroundColor: scheme.surfaceContainerLow,
      selectedColor: scheme.primaryContainer,
      labelStyle: text.labelSmall?.copyWith(color: scheme.onSurface),
      side: BorderSide(color: scheme.outlineVariant),
      shape: shape,
    ),
    dataTableTheme: DataTableThemeData(
      headingRowColor: WidgetStatePropertyAll(scheme.surfaceContainerLow),
      dataRowColor: WidgetStateProperty.resolveWith(
        (states) => states.contains(WidgetState.selected)
            ? scheme.surfaceContainerHigh
            : null,
      ),
      headingTextStyle: text.labelMedium?.copyWith(color: scheme.onSurface),
      dataTextStyle: text.bodyMedium?.copyWith(color: scheme.onSurface),
      dividerThickness: 0.7,
    ),
    scrollbarTheme: ScrollbarThemeData(
      thumbColor: WidgetStatePropertyAll(scheme.outline),
      trackColor: WidgetStatePropertyAll(scheme.surfaceContainerLow),
    ),
    tooltipTheme: TooltipThemeData(
      waitDuration: const Duration(milliseconds: 350),
      decoration: BoxDecoration(
        color: scheme.inverseSurface,
        borderRadius: BorderRadius.circular(StudioRadii.sm),
      ),
      textStyle: text.bodySmall?.copyWith(color: scheme.onInverseSurface),
    ),
    snackBarTheme: SnackBarThemeData(
      backgroundColor: scheme.inverseSurface,
      contentTextStyle: text.bodyMedium?.copyWith(
        color: scheme.onInverseSurface,
      ),
      actionTextColor: scheme.inversePrimary,
      closeIconColor: scheme.onInverseSurface,
    ),
    extensions: [
      siameseSemanticColors,
      GptMarkdownThemeData(
        brightness: Brightness.light,
        highlightColor: scheme.primaryContainer,
        h1: text.headlineLarge,
        h2: text.headlineMedium,
        h3: text.headlineSmall,
        h4: text.titleLarge,
        h5: text.titleMedium,
        h6: text.titleSmall,
        hrLineColor: scheme.outlineVariant,
        linkColor: scheme.primary,
        linkHoverColor: scheme.onPrimaryContainer,
      ),
    ],
  );
}
