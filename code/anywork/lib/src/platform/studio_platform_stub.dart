bool get isWindowsPlatform => false;

bool get isLinuxPlatform => false;

Future<void> openExternalUrl(String url) {
  return Future<void>.error(
    UnsupportedError('External URL launching is unavailable on this platform'),
  );
}
