import 'provider_models.dart';

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
    required this.wireProtocol,
    required this.contextWindow,
    required this.maxOutputTokens,
  });
  final String slug;
  final String displayName;
  final String wireProtocol;
  final int contextWindow;
  final int maxOutputTokens;
}

class ProviderModelConnectionCommand {
  const ProviderModelConnectionCommand({
    required this.slug,
    required this.connectionMode,
  });

  final String slug;
  final String connectionMode;
}

/// 某模型上下文压缩阈值用户覆盖的命令项；`limit` 为正整数 tokens。
class ProviderModelAutoCompactCommand {
  const ProviderModelAutoCompactCommand({
    required this.slug,
    required this.limit,
  });

  final String slug;
  final int limit;
}

class ProviderCommand {
  const ProviderCommand({
    required this.id,
    this.originalId,
    required this.templateKind,
    required this.name,
    required this.baseUrl,
    required this.secret,
    required this.pricingEnabled,
    required this.defaultModel,
    required this.customModels,
    required this.modelConnectionModes,
    required this.modelAutoCompactLimits,
  });

  final String id;
  final String? originalId;
  final String templateKind;
  final String name;
  final String baseUrl;
  final ProviderSecretCommand secret;
  final bool pricingEnabled;
  final String defaultModel;
  final List<ProviderModelCommand> customModels;
  final List<ProviderModelConnectionCommand> modelConnectionModes;
  final List<ProviderModelAutoCompactCommand> modelAutoCompactLimits;
}

abstract final class ProviderCommandBuilder {
  /// Builds the payload for one provider only.  Default provider, mode routes
  /// and role routes use separate commands and are never copied from this
  /// provider editor snapshot.
  static ProviderCommand buildProvider(
    ProviderSettingsView value, {
    String? originalId,
  }) {
    final provider = normalizeProvider(value);
    return ProviderCommand(
      id: provider.id,
      originalId: originalId,
      templateKind: provider.templateKind,
      name: provider.name,
      baseUrl: provider.baseUrl,
      secret: provider.bearerToken.trim().isNotEmpty
          ? ProviderSecretCommand.replace(provider.bearerToken.trim())
          : provider.hasBearerToken
          ? const ProviderSecretCommand.preserve()
          : const ProviderSecretCommand.clear(),
      pricingEnabled: provider.pricingEnabled,
      defaultModel: provider.defaultModel,
      customModels: [
        for (final model in provider.customModels)
          ProviderModelCommand(
            slug: model.slug.trim(),
            displayName: model.displayName.trim(),
            wireProtocol: model.wireProtocol,
            contextWindow: model.contextWindow ?? 32000,
            maxOutputTokens: model.maxOutputTokens ?? 4096,
          ),
      ],
      modelConnectionModes: [
        for (final override in provider.modelConnectionModes.entries)
          ProviderModelConnectionCommand(
            slug: override.key.trim(),
            connectionMode: override.value,
          ),
      ],
      modelAutoCompactLimits: _autoCompactCommands(provider),
    );
  }

  /// 该 provider 实例完整的压缩阈值覆盖集合；未列出模型恢复默认，未修改模型保留覆盖。
  ///
  /// 覆盖值与模型默认相同或为空时不下发，交由服务端按默认解析；只保留正整数值。
  static List<ProviderModelAutoCompactCommand> _autoCompactCommands(
    ProviderSettingsView provider,
  ) {
    final commands = <ProviderModelAutoCompactCommand>[];
    for (final model in provider.allModels) {
      final limit = provider.autoCompactLimits[model.slug];
      final override = limit?.overrideLimit;
      final slug = model.slug.trim();
      if (override == null || override <= 0 || slug.isEmpty) {
        continue;
      }
      if (override == limit!.defaultLimit) {
        continue;
      }
      commands.add(
        ProviderModelAutoCompactCommand(slug: slug, limit: override),
      );
    }
    return commands;
  }

  static ProviderSettingsView normalizeProvider(ProviderSettingsView provider) {
    final models = provider.allModels
        .where((model) => model.slug.trim().isNotEmpty)
        .toList();
    // canonical defaultModel 非空时始终保留（含当前无法解析的 slug），只有
    // 空值才回退到首个模型；仅用户显式选择才替换。
    final defaultModel = provider.defaultModel.isNotEmpty
        ? provider.defaultModel
        : models.firstOrNull?.slug ?? '';
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
}

/// One canonical settings field/resource mutation.
///
/// Each command carries only the value the user changed. The repository and
/// Bridge never receive a stale sibling settings snapshot to merge back.
sealed class SettingsFieldCommand {
  const SettingsFieldCommand();
}

class InstructionBaseOverrideCommand extends SettingsFieldCommand {
  const InstructionBaseOverrideCommand(this.value);
  final String value;
}

class InstructionDeveloperCommand extends SettingsFieldCommand {
  const InstructionDeveloperCommand(this.value);
  final String value;
}

class InstructionUserCommand extends SettingsFieldCommand {
  const InstructionUserCommand(this.value);
  final String value;
}

class ProjectDocMaxBytesCommand extends SettingsFieldCommand {
  const ProjectDocMaxBytesCommand(this.value);
  final int value;
}

class ProjectDocFallbackFilenamesCommand extends SettingsFieldCommand {
  const ProjectDocFallbackFilenamesCommand(this.value);
  final List<String> value;
}

class SkillsEnabledCommand extends SettingsFieldCommand {
  const SkillsEnabledCommand(this.value);
  final bool value;
}

class SkillsAutoLearnCommand extends SettingsFieldCommand {
  const SkillsAutoLearnCommand(this.value);
  final bool value;
}

class SkillsSystemEnabledCommand extends SettingsFieldCommand {
  const SkillsSystemEnabledCommand(this.value);
  final bool value;
}

class SkillsProjectDirCommand extends SettingsFieldCommand {
  const SkillsProjectDirCommand(this.value);
  final String value;
}

class SkillsUserDirCommand extends SettingsFieldCommand {
  const SkillsUserDirCommand(this.value);
  final String value;
}

class SkillsExternalDirsCommand extends SettingsFieldCommand {
  const SkillsExternalDirsCommand(this.value);
  final List<String> value;
}

class SkillsDisabledCommand extends SettingsFieldCommand {
  const SkillsDisabledCommand(this.value);
  final List<String> value;
}

class SkillsAutoLearnMinToolCallsCommand extends SettingsFieldCommand {
  const SkillsAutoLearnMinToolCallsCommand(this.value);
  final int value;
}

class McpServerEnabledCommand extends SettingsFieldCommand {
  const McpServerEnabledCommand({required this.id, required this.value});
  final String id;
  final bool value;
}

class McpServerTransportCommand extends SettingsFieldCommand {
  const McpServerTransportCommand({required this.id, required this.value});
  final String id;
  final String value;
}

class McpServerEndpointCommand extends SettingsFieldCommand {
  const McpServerEndpointCommand({required this.id, required this.value});
  final String id;
  final String value;
}

class GeneralFollowActiveTurnCommand extends SettingsFieldCommand {
  const GeneralFollowActiveTurnCommand(this.value);
  final bool value;
}

class GeneralCompactTimelineCommand extends SettingsFieldCommand {
  const GeneralCompactTimelineCommand(this.value);
  final bool value;
}

class GeneralSidebarWidthCommand extends SettingsFieldCommand {
  const GeneralSidebarWidthCommand(this.value);
  final int? value;
}

class GeneralPinnedThreadIdsCommand extends SettingsFieldCommand {
  const GeneralPinnedThreadIdsCommand(this.value);
  final List<String> value;
}

class GeneralPinnedProjectIdsCommand extends SettingsFieldCommand {
  const GeneralPinnedProjectIdsCommand(this.value);
  final List<String> value;
}

class WebSearchModeCommand extends SettingsFieldCommand {
  const WebSearchModeCommand(this.value);
  final String value;
}

class WebSearchContextSizeCommand extends SettingsFieldCommand {
  const WebSearchContextSizeCommand(this.value);
  final String? value;
}

class WebSearchAllowedDomainsCommand extends SettingsFieldCommand {
  const WebSearchAllowedDomainsCommand(this.value);
  final List<String> value;
}

class WebSearchCountryCommand extends SettingsFieldCommand {
  const WebSearchCountryCommand(this.value);
  final String? value;
}

class WebSearchRegionCommand extends SettingsFieldCommand {
  const WebSearchRegionCommand(this.value);
  final String? value;
}

class WebSearchCityCommand extends SettingsFieldCommand {
  const WebSearchCityCommand(this.value);
  final String? value;
}

class WebSearchTimezoneCommand extends SettingsFieldCommand {
  const WebSearchTimezoneCommand(this.value);
  final String? value;
}

class DeepSeekWebSearchEnabledCommand extends SettingsFieldCommand {
  const DeepSeekWebSearchEnabledCommand(this.value);
  final bool value;
}

class ModeModelCommand extends SettingsFieldCommand {
  const ModeModelCommand({
    required this.modeId,
    required this.providerId,
    required this.model,
  });
  final String modeId;
  final String providerId;
  final String model;
}

class ModeReasoningEffortCommand extends SettingsFieldCommand {
  const ModeReasoningEffortCommand({required this.modeId, this.effort});
  final String modeId;
  final String? effort;
}

class RoleModelCommand extends SettingsFieldCommand {
  const RoleModelCommand({
    required this.role,
    required this.providerId,
    required this.model,
  });
  final String role;
  final String providerId;
  final String model;
}

class RoleReasoningEffortCommand extends SettingsFieldCommand {
  const RoleReasoningEffortCommand({required this.role, this.effort});
  final String role;
  final String? effort;
}

extension<T> on List<T> {
  T? get firstOrNull => isEmpty ? null : first;
}
