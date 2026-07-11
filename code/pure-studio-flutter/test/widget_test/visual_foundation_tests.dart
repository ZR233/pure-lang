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

    test('visual captures ship a licensed repository test font', () {
      final font = File('test/assets/fonts/NotoSans-Variable.ttf');
      final license = File('test/assets/fonts/OFL.txt');
      final source = File('test/assets/fonts/SOURCE.md');

      expect(font.existsSync(), isTrue);
      expect(font.lengthSync(), greaterThan(100000));
      expect(license.existsSync(), isTrue);
      expect(
        license.readAsStringSync(),
        contains('SIL OPEN FONT LICENSE Version 1.1'),
      );
      expect(source.existsSync(), isTrue);
      expect(source.readAsStringSync(), contains('github.com/google/fonts'));
      expect(source.readAsStringSync(), contains('test-only'));
    });
  });
}
