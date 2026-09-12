import 'dart:developer' as developer;

void recordDartError(Object error, StackTrace? stack) {
  developer.log(
    'Unhandled anywork Web error',
    name: 'anywork',
    error: error,
    stackTrace: stack ?? StackTrace.current,
  );
}
