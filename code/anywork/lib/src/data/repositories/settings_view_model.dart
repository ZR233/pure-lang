import '../../domain/models/studio_models.dart';

/// Immutable presentation projection for Settings.
///
/// This type deliberately contains no mutation methods and no Bridge handle.
/// A tab can keep the projection it needs while a catalog refresh advances its
/// own revision; a settings mutation always goes back through the controller's
/// typed intent path.
final class SettingsViewModel {
  const SettingsViewModel({
    required this.revision,
    required this.catalogRevision,
    required this.providers,
    required this.defaultProviderId,
    required this.modeModelRoutes,
    required this.roles,
    required this.instructions,
    required this.skills,
    required this.mcpServers,
    required this.general,
    required this.webSearch,
    required this.deepSeekWebSearch,
    required this.permissionMode,
  });

  factory SettingsViewModel.fromSnapshot(SettingsStateSnapshot snapshot) {
    final data = snapshot.state.value;
    if (data == null) {
      return SettingsViewModel.empty;
    }
    return SettingsViewModel(
      revision: snapshot.revision,
      catalogRevision: snapshot.modelCatalogRevision,
      providers: List.unmodifiable(snapshot.providers),
      defaultProviderId: snapshot.defaultProviderId,
      modeModelRoutes: List.unmodifiable(snapshot.modeModelRoutes),
      roles: List.unmodifiable(snapshot.roles),
      instructions: snapshot.instructions,
      skills: snapshot.skills,
      mcpServers: List.unmodifiable(snapshot.mcpServers),
      general: snapshot.general,
      webSearch: snapshot.webSearch,
      deepSeekWebSearch: snapshot.deepSeekWebSearch,
      permissionMode: snapshot.permissionMode,
    );
  }

  static const empty = SettingsViewModel(
    revision: 0,
    catalogRevision: 0,
    providers: <ProviderSettingsView>[],
    defaultProviderId: null,
    modeModelRoutes: <ModeModelRouteView>[],
    roles: <RoleSettingsView>[],
    instructions: InstructionsSettingsView(),
    skills: SkillsSettingsView(),
    mcpServers: <McpServerSettingsView>[],
    general: GeneralSettingsView(),
    webSearch: WebSearchSettingsView(),
    deepSeekWebSearch: DeepSeekWebSearchSettingsView(),
    permissionMode: PermissionMode.requestApproval,
  );

  final int revision;
  final int catalogRevision;
  final List<ProviderSettingsView> providers;
  final String? defaultProviderId;
  final List<ModeModelRouteView> modeModelRoutes;
  final List<RoleSettingsView> roles;
  final InstructionsSettingsView instructions;
  final SkillsSettingsView skills;
  final List<McpServerSettingsView> mcpServers;
  final GeneralSettingsView general;
  final WebSearchSettingsView webSearch;
  final DeepSeekWebSearchSettingsView deepSeekWebSearch;
  final PermissionMode permissionMode;
}
