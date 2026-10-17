import 'package:anywork/src/features/settings/settings_views.dart';
import 'package:anywork/src/presentation/providers/client_provider.dart';
import 'package:anywork_client/studio_client.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test('settings providers view keeps snapshot origins for grouping', () {
    final client = AnyworkClient(
      transport: _UnusedTransport(),
      clientId: 'origin-adapter',
    );
    addTearDown(client.close);
    client.settings.merge(
      SettingsStateSnapshot(
        revision: 3,
        providers: [
          ProviderSettingsView(
            id: 'openai',
            name: 'OpenAI',
            baseUrl: 'https://api.openai.com',
            defaultModel: 'gpt-manual',
            status: 'ready',
            usageLabel: '',
            models: const [
              ProviderModelView(
                slug: 'gpt-online',
                displayName: 'Online',
                reasoningEfforts: [],
                origin: ProviderModelOrigin.online,
              ),
              ProviderModelView(
                slug: 'gpt-bundled',
                displayName: 'Bundled',
                reasoningEfforts: [],
                origin: ProviderModelOrigin.bundled,
              ),
              ProviderModelView(
                slug: 'gpt-manual',
                displayName: 'Manual',
                reasoningEfforts: [],
                origin: ProviderModelOrigin.manual,
              ),
            ],
          ),
        ],
      ),
    );
    final container = ProviderContainer(
      overrides: [anyworkClientProvider.overrideWith((ref) => client)],
    );
    addTearDown(container.dispose);
    final view = container.read(settingsProvidersViewProvider);
    final provider = view.providers.single;
    expect(provider.onlineModels.map((model) => model.slug), ['gpt-online']);
    expect(provider.bundledModels.map((model) => model.slug), ['gpt-bundled']);
    expect(provider.customModels.map((model) => model.slug), ['gpt-manual']);
  });
}

class _UnusedTransport implements StudioTransport {
  @override
  dynamic noSuchMethod(Invocation invocation) =>
      throw UnsupportedError('Unused transport: ${invocation.memberName}');
}
