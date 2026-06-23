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
    required this.enabled,
    required this.status,
  });

  final String id;
  final String transport;
  final String endpoint;
  final bool enabled;
  final String status;
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
