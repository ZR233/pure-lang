import 'dart:io';

bool get isWindowsPlatform => Platform.isWindows;

Future<void> openExternalUrl(String url) async {
  if (Platform.isWindows) {
    await Process.start('rundll32.exe', [
      'url.dll,FileProtocolHandler',
      url,
    ], mode: ProcessStartMode.detached);
    return;
  }
  if (Platform.isLinux) {
    await Process.start('xdg-open', [url], mode: ProcessStartMode.detached);
    return;
  }
  throw UnsupportedError(
    'External URL launching is unsupported on ${Platform.operatingSystem}',
  );
}
