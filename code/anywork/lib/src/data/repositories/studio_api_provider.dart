import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../frb/studio_api.dart';

part 'studio_api_provider.g.dart';

@Riverpod(keepAlive: true)
StudioBridgeDataSource studioBridgeDataSource(Ref ref) {
  if (const bool.fromEnvironment('ANYWORK_DEMO')) {
    if (const bool.fromEnvironment('ANYWORK_DRIVER')) {
      return DriverDemoStudioBridgeDataSource(lspActivityLoop: true);
    }
    return DemoStudioBridgeDataSource(lspActivityLoop: true);
  }
  return FrbStudioBridgeDataSource();
}
