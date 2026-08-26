bool get isWindowsPlatform => false;

Future<void> openExternalUrl(String url) {
  return Future<void>.error(
    UnsupportedError('External URL launching is unavailable on this platform'),
  );
}
