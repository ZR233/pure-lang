import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../frb/studio_api.dart';

part 'studio_api_provider.g.dart';

@Riverpod(keepAlive: true)
StudioApi studioApi(Ref ref) {
  if (const bool.fromEnvironment('PURE_STUDIO_DEMO')) {
    if (const bool.fromEnvironment('PURE_STUDIO_DRIVER')) {
      return DriverDemoStudioApi();
    }
    return DemoStudioApi();
  }
  return FrbStudioApi();
}
