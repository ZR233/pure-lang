import 'dart:developer' as developer;

void recordDartError(Object error, StackTrace? stack) {
  developer.log(
    'Unhandled Pure Studio Web error',
    name: 'pure_studio',
    error: error,
    stackTrace: stack ?? StackTrace.current,
  );
}
