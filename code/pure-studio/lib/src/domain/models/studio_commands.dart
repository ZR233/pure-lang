import 'provider_models.dart';
import 'settings_models.dart';

enum ProviderSecretAction { preserve, replace, clear }

class ProviderSecretCommand {
  const ProviderSecretCommand._(this.action, this.value);

  const ProviderSecretCommand.preserve()
    : this._(ProviderSecretAction.preserve, null);

  const ProviderSecretCommand.replace(String value)
    : this._(ProviderSecretAction.replace, value);

  const ProviderSecretCommand.clear()
    : this._(ProviderSecretAction.clear, null);

  final ProviderSecretAction action;
  final String? value;
}

class ProviderModelCommand {
  const ProviderModelCommand({
    required this.slug,
    required this.displayName,
    required this.reasoningEfforts,
    required this.wireProtocol,
    required this.supportedConnectionModes,
    required this.defaultConnectionMode,
    this.baseInstructions,
  });

  final String slug;
  final String displayName;
  final List<String> reasoningEfforts;
  final String wireProtocol;
  final List<String> supportedConnectionModes;
  final String defaultConnectionMode;
  final String? baseInstructions;
}

class ProviderModelConnectionCommand {
  const ProviderModelConnectionCommand({
    required this.slug,
    required this.connectionMode,
  });

  final String slug;
  final String connectionMode;
}

class ProviderCommand {
  const ProviderCommand({
    required this.id,
    this.originalId,
    required this.templateKind,
    required this.name,
    required this.baseUrl,
    required this.secret,
    required this.capabilitySource,
    required this.hostedWebSearch,
    this.standaloneWebSearch,
    required this.promptCacheDialect,
    required this.responsesProgrammaticToolCalling,
    required this.defaultModel,
    required this.customModels,
    required this.modelConnectionModes,
  });

  final String id;
  final String? originalId;
  final String templateKind;
  final String name;
  final String baseUrl;
  final ProviderSecretCommand secret;
  final String capabilitySource;
  final bool hostedWebSearch;
  final String? standaloneWebSearch;
  final String promptCacheDialect;
  final bool responsesProgrammaticToolCalling;
  final String defaultModel;
  final List<ProviderModelCommand> customModels;
  final List<ProviderModelConnectionCommand> modelConnectionModes;
}

class RoleSettingsCommand {
  const RoleSettingsCommand({
    required this.key,
    required this.providerId,
    required this.model,
    required this.effort,
  });

  final String key;
  final String providerId;
  final String model;
  final String effort;
}

class ProviderSettingsCommand {
  const ProviderSettingsCommand({
    required this.defaultProviderId,
    required this.providers,
    required this.roles,
  });

  final String defaultProviderId;
  final List<ProviderCommand> providers;
  final List<RoleSettingsCommand> roles;
}

abstract final class ProviderSettingsCommandBuilder {
  static ProviderSettingsCommand build({
    required List<ProviderSettingsView> providers,
    required List<RoleSettingsView> roles,
    String? selectedProviderId,
    String? renamedFrom,
    String? renamedTo,
    String? removedProviderId,
  }) {
    final normalized = providers.map(normalizeProvider).toList();
    final fallback = normalized.isEmpty ? null : normalized.first;
    final providerIds = normalized.map((provider) => provider.id).toSet();
    final commands = <RoleSettingsCommand>[];
    if (fallback != null) {
      for (final role in roles) {
        var providerId = role.providerId;
        if (renamedFrom != null && providerId == renamedFrom) {
          providerId = renamedTo ?? providerId;
        }
        if (removedProviderId != null && providerId == removedProviderId) {
          providerId = fallback.id;
        }
        final provider = _providerById(normalized, providerId);
        final safeProvider =
            providerIds.contains(providerId) && provider != null
            ? provider
            : fallback;
        final model =
            safeProvider.allModels.any(
              (candidate) => candidate.slug == role.model,
            )
            ? role.model
            : safeProvider.defaultModel;
        final selectedModel = _modelBySlug(safeProvider.allModels, model);
        final effort =
            selectedModel?.reasoningEfforts.contains(role.effort) == true
            ? role.effort
            : selectedModel?.defaultReasoningEffort.isNotEmpty == true
            ? selectedModel!.defaultReasoningEffort
            : selectedModel?.reasoningEfforts.firstOrNull ?? '';
        commands.add(
          RoleSettingsCommand(
            key: role.key,
            providerId: safeProvider.id,
            model: model,
            effort: effort,
          ),
        );
      }
    }
    return ProviderSettingsCommand(
      defaultProviderId:
          selectedProviderId ?? (normalized.isEmpty ? '' : normalized.first.id),
      providers: [
        for (final provider in normalized)
          ProviderCommand(
            id: provider.id,
            originalId: provider.id == renamedTo ? renamedFrom : null,
            templateKind: provider.templateKind,
            name: provider.name,
            baseUrl: provider.baseUrl,
            secret: provider.bearerToken.trim().isNotEmpty
                ? ProviderSecretCommand.replace(provider.bearerToken.trim())
                : provider.hasBearerToken
                ? const ProviderSecretCommand.preserve()
                : const ProviderSecretCommand.clear(),
            capabilitySource: provider.capabilitySource,
            hostedWebSearch: provider.hostedWebSearch,
            standaloneWebSearch: provider.standaloneWebSearch.trim().isEmpty
                ? null
                : provider.standaloneWebSearch.trim(),
            promptCacheDialect: provider.promptCacheDialect,
            responsesProgrammaticToolCalling:
                provider.responsesProgrammaticToolCalling,
            defaultModel: provider.defaultModel,
            customModels: [
              for (final model in provider.customModels)
                ProviderModelCommand(
                  slug: model.slug.trim(),
                  displayName: model.displayName.trim(),
                  reasoningEfforts: model.reasoningEfforts,
                  wireProtocol: model.wireProtocol,
                  supportedConnectionModes: model.supportedConnectionModes,
                  defaultConnectionMode: model.defaultConnectionMode,
                  baseInstructions: model.baseInstructions.trim().isEmpty
                      ? null
                      : model.baseInstructions,
                ),
            ],
            modelConnectionModes: [
              for (final model in provider.allModels)
                ProviderModelConnectionCommand(
                  slug: model.slug.trim(),
                  connectionMode: model.connectionMode,
                ),
            ],
          ),
      ],
      roles: commands,
    );
  }

  static ProviderSettingsView normalizeProvider(ProviderSettingsView provider) {
    final models = provider.allModels
        .where((model) => model.slug.trim().isNotEmpty)
        .toList();
    final defaultModel =
        models.any((model) => model.slug == provider.defaultModel)
        ? provider.defaultModel
        : models.firstOrNull?.slug ?? provider.defaultModel;
    return provider.copyWith(
      id: provider.id.trim(),
      name: provider.name.trim(),
      baseUrl: provider.baseUrl.trim(),
      defaultModel: defaultModel.trim(),
      models: models,
      customModels: provider.customModels
          .where((model) => model.slug.trim().isNotEmpty)
          .toList(),
    );
  }

  static ProviderSettingsView? _providerById(
    List<ProviderSettingsView> providers,
    String id,
  ) {
    for (final provider in providers) {
      if (provider.id == id) return provider;
    }
    return null;
  }

  static ProviderModelView? _modelBySlug(
    List<ProviderModelView> models,
    String slug,
  ) {
    for (final model in models) {
      if (model.slug == slug) return model;
    }
    return null;
  }
}

class InstructionsSettingsCommand {
  const InstructionsSettingsCommand({
    required this.baseOverride,
    required this.developer,
    required this.user,
    required this.projectDocMaxBytes,
    required this.projectDocFallbackFilenames,
  });

  final String baseOverride;
  final String developer;
  final String user;
  final int projectDocMaxBytes;
  final List<String> projectDocFallbackFilenames;
}

class SkillsSettingsCommand {
  const SkillsSettingsCommand({
    required this.enabled,
    required this.autoLearn,
    required this.systemEnabled,
    required this.projectDir,
    required this.userDir,
    required this.externalDirs,
    required this.disabled,
    required this.autoLearnMinToolCalls,
  });

  final bool enabled;
  final bool autoLearn;
  final bool systemEnabled;
  final String projectDir;
  final String userDir;
  final List<String> externalDirs;
  final List<String> disabled;
  final int autoLearnMinToolCalls;
}

class McpServerCommand {
  const McpServerCommand({
    required this.id,
    required this.enabled,
    required this.transport,
    required this.endpoint,
  });

  final String id;
  final bool enabled;
  final String transport;
  final String endpoint;
}

class McpSettingsCommand {
  const McpSettingsCommand({required this.servers});

  final List<McpServerCommand> servers;
}

class GeneralSettingsCommand {
  const GeneralSettingsCommand({
    required this.followSystemTheme,
    required this.followActiveTurn,
    required this.compactTimeline,
  });

  final bool followSystemTheme;
  final bool followActiveTurn;
  final bool compactTimeline;
}

class WebSearchSettingsCommand {
  const WebSearchSettingsCommand({
    required this.mode,
    required this.contextSize,
    required this.allowedDomains,
    this.country,
    this.region,
    this.city,
    this.timezone,
  });

  final String mode;
  final String? contextSize;
  final List<String> allowedDomains;
  final String? country;
  final String? region;
  final String? city;
  final String? timezone;
}

extension<T> on List<T> {
  T? get firstOrNull => isEmpty ? null : first;
}
