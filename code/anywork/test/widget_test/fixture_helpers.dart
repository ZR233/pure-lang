part of '../widget_test.dart';

/// 显式打开当前选中会话并等到首个权威帧后的历史窗口落地。
///
/// 生产代码的首屏只恢复“选择”，不打开会话（§6.1）；依赖订阅/实时帧/历史窗口的测试
/// 必须先显式打开，语义与用户点击“打开会话”完全一致。
Future<void> _openSelectedThread(ProviderContainer container) async {
  await container.read(studioControllerProvider.notifier).openSelectedThread();
  await pumpEventQueue();
}

Widget _localizedApp({
  required Widget home,
  Locale locale = const Locale('en'),
  bool disableAnimations = false,
}) {
  return MaterialApp(
    theme: pureStudioTheme(),
    themeMode: ThemeMode.light,
    locale: locale,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    builder: disableAnimations
        ? (context, child) => MediaQuery(
            data: MediaQuery.of(context).copyWith(disableAnimations: true),
            child: child!,
          )
        : null,
    home: home,
  );
}

void _configureResponsiveView(WidgetTester tester, Size size) {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
}
