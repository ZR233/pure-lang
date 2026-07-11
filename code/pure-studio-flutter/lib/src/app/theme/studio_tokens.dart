import 'package:flutter/material.dart';

final class StudioColors {
  const StudioColors._();

  static const paper = Color(0xfffaf9f7);
  static const paper2 = Color(0xfff3eeea);
  static const paper3 = Color(0xffece5de);
  static const ink = Color(0xff2b2520);
  static const inkSoft = Color(0xff6b635a);
  static const inkFaint = Color(0xffa8a097);
  static const line = Color(0xffe6ddd6);
  static const line2 = Color(0xffdcd2c8);
  static const clay = Color(0xffd97757);
  static const claySoft = Color(0xfff8e6dc);
  static const clayDeep = Color(0xffb85c3e);
  static const sage = Color(0xff6b8e5a);
  static const sageSoft = Color(0xffe3ead8);
  static const ochre = Color(0xffc08a3e);
  static const rose = Color(0xffc25563);
  static const white = Color(0xffffffff);

  static const darkPaper = Color(0xff1f1b18);
  static const darkPaper2 = Color(0xff29231f);
  static const darkPaper3 = Color(0xff342c26);
  static const darkInk = Color(0xfff4ece6);
  static const darkInkSoft = Color(0xffcfc3ba);
  static const darkLine = Color(0xff493d35);
}

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

  bool get isDark => Theme.of(this).brightness == Brightness.dark;

  Color get studioPaper => isDark ? StudioColors.darkPaper : StudioColors.paper;
  Color get studioPaper2 =>
      isDark ? StudioColors.darkPaper2 : StudioColors.paper2;
  Color get studioPaper3 =>
      isDark ? StudioColors.darkPaper3 : StudioColors.paper3;
  Color get studioInk => isDark ? StudioColors.darkInk : StudioColors.ink;
  Color get studioInkSoft =>
      isDark ? StudioColors.darkInkSoft : StudioColors.inkSoft;
  Color get studioLine => isDark ? StudioColors.darkLine : StudioColors.line;
  Color get studioLine2 => isDark ? StudioColors.darkLine : StudioColors.line2;
}
