part of '../widget_test.dart';

void registerVisualFoundationTests() {
  group('dense console visual foundation', () {
    test('caps shared panel radii at eight pixels', () {
      expect([StudioRadii.md, StudioRadii.lg], [8.0, 8.0]);
    });

    test('exposes one shared layout contract', () {
      expect(
        [
          StudioLayout.sidebarWidth,
          StudioLayout.compactRailWidth,
          StudioLayout.conversationWidth,
          StudioLayout.settingsNavigationWidth,
          StudioLayout.compactBreakpoint,
        ],
        [232.0, 60.0, 860.0, 196.0, 900.0],
      );
    });
  });
}
