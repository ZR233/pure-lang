class RoleSettingsView {
  const RoleSettingsView({
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

class McpServerSettingsView {
  const McpServerSettingsView({
    required this.id,
    required this.transport,
    required this.endpoint,
    required this.state,
    this.sourceKind = 'user',
    this.mutationPolicy = 'userEditable',
  });

  final String id;
  final String transport;
  final String endpoint;
  final McpServerState state;
  final String sourceKind;
  final String mutationPolicy;

  bool get hasLockedIdentity => mutationPolicy == 'lockedIdentity';
  bool get enabled => state is! McpDisabledState;
}

sealed class McpServerState {
  const McpServerState();
}

final class McpDisabledState extends McpServerState {
  const McpDisabledState({required this.message});
  final String message;
}

final class McpMissingCredentialState extends McpServerState {
  const McpMissingCredentialState({required this.message});
  final String message;
}

final class McpCheckingState extends McpServerState {
  const McpCheckingState({required this.message});
  final String message;
}

final class McpAvailableState extends McpServerState {
  const McpAvailableState({required this.checkedAt, required this.toolCount});
  final int checkedAt;
  final int toolCount;
}

final class McpUnavailableState extends McpServerState {
  const McpUnavailableState({
    required this.checkedAt,
    required this.code,
    required this.message,
    required this.retryable,
  });
  final int checkedAt;
  final String code;
  final String message;
  final bool retryable;
}

class InstructionsSettingsView {
  const InstructionsSettingsView({
    this.baseOverride = '',
    this.developer = '',
    this.user = '',
    this.projectDocMaxBytes = 65536,
    this.projectDocFallbackFilenames = const [],
  });

  final String baseOverride;
  final String developer;
  final String user;
  final int projectDocMaxBytes;
  final List<String> projectDocFallbackFilenames;

  InstructionsSettingsView copyWith({
    String? baseOverride,
    String? developer,
    String? user,
    int? projectDocMaxBytes,
    List<String>? projectDocFallbackFilenames,
  }) {
    return InstructionsSettingsView(
      baseOverride: baseOverride ?? this.baseOverride,
      developer: developer ?? this.developer,
      user: user ?? this.user,
      projectDocMaxBytes: projectDocMaxBytes ?? this.projectDocMaxBytes,
      projectDocFallbackFilenames:
          projectDocFallbackFilenames ?? this.projectDocFallbackFilenames,
    );
  }
}

class SkillsSettingsView {
  const SkillsSettingsView({
    this.enabled = true,
    this.autoLearn = true,
    this.systemEnabled = true,
    this.projectDir = 'skills',
    this.userDir = '~/.pure/skills',
    this.externalDirs = const [],
    this.disabled = const [],
    this.autoLearnMinToolCalls = 5,
  });

  final bool enabled;
  final bool autoLearn;
  final bool systemEnabled;
  final String projectDir;
  final String userDir;
  final List<String> externalDirs;
  final List<String> disabled;
  final int autoLearnMinToolCalls;

  SkillsSettingsView copyWith({
    bool? enabled,
    bool? autoLearn,
    bool? systemEnabled,
    String? projectDir,
    String? userDir,
    List<String>? externalDirs,
    List<String>? disabled,
    int? autoLearnMinToolCalls,
  }) {
    return SkillsSettingsView(
      enabled: enabled ?? this.enabled,
      autoLearn: autoLearn ?? this.autoLearn,
      systemEnabled: systemEnabled ?? this.systemEnabled,
      projectDir: projectDir ?? this.projectDir,
      userDir: userDir ?? this.userDir,
      externalDirs: externalDirs ?? this.externalDirs,
      disabled: disabled ?? this.disabled,
      autoLearnMinToolCalls:
          autoLearnMinToolCalls ?? this.autoLearnMinToolCalls,
    );
  }
}

class GeneralSettingsView {
  const GeneralSettingsView({
    this.followSystemTheme = true,
    this.followActiveTurn = true,
    this.compactTimeline = false,
  });

  final bool followSystemTheme;
  final bool followActiveTurn;
  final bool compactTimeline;

  GeneralSettingsView copyWith({
    bool? followSystemTheme,
    bool? followActiveTurn,
    bool? compactTimeline,
  }) {
    return GeneralSettingsView(
      followSystemTheme: followSystemTheme ?? this.followSystemTheme,
      followActiveTurn: followActiveTurn ?? this.followActiveTurn,
      compactTimeline: compactTimeline ?? this.compactTimeline,
    );
  }
}

class WebSearchSettingsView {
  const WebSearchSettingsView({
    this.configuredMode = 'cached',
    this.effectiveMode = 'disabled',
    this.availability = 'missingCredential',
    this.contextSize,
    this.allowedDomains = const [],
    this.country,
    this.region,
    this.city,
    this.timezone,
    this.providerId,
    this.model,
  });

  final String configuredMode;
  final String effectiveMode;
  final String availability;
  final String? contextSize;
  final List<String> allowedDomains;
  final String? country;
  final String? region;
  final String? city;
  final String? timezone;
  final String? providerId;
  final String? model;

  bool get isAvailable => availability == 'available';

  WebSearchSettingsView withConfiguredValues({
    required String configuredMode,
    String? contextSize,
    List<String> allowedDomains = const [],
    String? country,
    String? region,
    String? city,
    String? timezone,
  }) {
    return WebSearchSettingsView(
      configuredMode: configuredMode,
      effectiveMode: effectiveMode,
      availability: availability,
      contextSize: contextSize,
      allowedDomains: allowedDomains,
      country: country,
      region: region,
      city: city,
      timezone: timezone,
      providerId: providerId,
      model: model,
    );
  }
}
