import 'dart:io';

bool get isWindowsPlatform => Platform.isWindows;

Future<void> openExternalUrl(String url) async {
  if (!Platform.isWindows) {
    throw UnsupportedError('Release notes are only launched by Windows builds');
  }
  await Process.start('rundll32.exe', [
    'url.dll,FileProtocolHandler',
    url,
  ], mode: ProcessStartMode.detached);
}
