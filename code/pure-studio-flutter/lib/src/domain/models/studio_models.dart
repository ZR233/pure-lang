enum PermissionMode { requestApproval, autoReview, fullAccess }

enum CompileMode { auto, plan }

enum TurnPhase {
  idle,
  queued,
  contextLoading,
  waitingForModel,
  streaming,
  waitingForInteraction,
  runningTool,
  completed,
  failed,
  cancelled,
}

enum TimelinePartType { text, reasoning, tool, plan, agent }

enum InteractionKind { toolApproval, userInput, planConfirmation }

class StudioProject {
  const StudioProject({
    required this.id,
    required this.name,
    required this.path,
  });

  final String id;
  final String name;
  final String path;
}

class StudioSession {
  const StudioSession({
    required this.id,
    required this.projectId,
    required this.title,
    required this.mode,
    required this.updatedAt,
  });

  final String id;
  final String projectId;
  final String title;
  final CompileMode mode;
  final DateTime updatedAt;

  StudioSession copyWith({
    String? title,
    CompileMode? mode,
    DateTime? updatedAt,
  }) {
    return StudioSession(
      id: id,
      projectId: projectId,
      title: title ?? this.title,
      mode: mode ?? this.mode,
      updatedAt: updatedAt ?? this.updatedAt,
    );
  }
}

class TimelinePart {
  const TimelinePart({
    required this.id,
    required this.messageId,
    required this.type,
    required this.text,
    this.title,
    this.status = 'completed',
    this.collapsed = false,
  });

  final String id;
  final String messageId;
  final TimelinePartType type;
  final String text;
  final String? title;
  final String status;
  final bool collapsed;

  TimelinePart copyWith({String? text, String? status, bool? collapsed}) {
    return TimelinePart(
      id: id,
      messageId: messageId,
      type: type,
      text: text ?? this.text,
      title: title,
      status: status ?? this.status,
      collapsed: collapsed ?? this.collapsed,
    );
  }
}

class TimelineMessage {
  const TimelineMessage({
    required this.id,
    required this.sessionId,
    required this.role,
    required this.createdAt,
    required this.parts,
  });

  final String id;
  final String sessionId;
  final String role;
  final DateTime createdAt;
  final List<TimelinePart> parts;

  TimelineMessage copyWith({List<TimelinePart>? parts}) {
    return TimelineMessage(
      id: id,
      sessionId: sessionId,
      role: role,
      createdAt: createdAt,
      parts: parts ?? this.parts,
    );
  }
}

class PendingInteraction {
  const PendingInteraction({
    required this.id,
    required this.sessionId,
    required this.kind,
    required this.title,
    required this.body,
    this.payload = const {},
  });

  final String id;
  final String sessionId;
  final InteractionKind kind;
  final String title;
  final String body;
  final Map<String, Object?> payload;
}

class SessionRuntimeView {
  const SessionRuntimeView({
    required this.model,
    required this.contextTokens,
    required this.contextWindow,
    required this.totalTokens,
    required this.costLabel,
    required this.activeSkills,
    required this.activeMcpServers,
    required this.activeLspServers,
    required this.agentCount,
  });

  final String model;
  final int contextTokens;
  final int contextWindow;
  final int totalTokens;
  final String costLabel;
  final List<String> activeSkills;
  final List<String> activeMcpServers;
  final List<String> activeLspServers;
  final int agentCount;

  SessionRuntimeView copyWith({
    String? model,
    int? contextTokens,
    int? contextWindow,
    int? totalTokens,
    String? costLabel,
    List<String>? activeSkills,
    List<String>? activeMcpServers,
    List<String>? activeLspServers,
    int? agentCount,
  }) {
    return SessionRuntimeView(
      model: model ?? this.model,
      contextTokens: contextTokens ?? this.contextTokens,
      contextWindow: contextWindow ?? this.contextWindow,
      totalTokens: totalTokens ?? this.totalTokens,
      costLabel: costLabel ?? this.costLabel,
      activeSkills: activeSkills ?? this.activeSkills,
      activeMcpServers: activeMcpServers ?? this.activeMcpServers,
      activeLspServers: activeLspServers ?? this.activeLspServers,
      agentCount: agentCount ?? this.agentCount,
    );
  }
}

class ProviderModelView {
  const ProviderModelView({
    required this.slug,
    required this.displayName,
    required this.reasoningEfforts,
    this.description = '',
    this.contextWindow,
    this.maxOutputTokens,
    this.currency = '',
    this.inputPricePerMTok,
    this.outputPricePerMTok,
    this.cacheReadPricePerMTok,
    this.baseInstructions = '',
  });

  final String slug;
  final String displayName;
  final List<String> reasoningEfforts;
  final String description;
  final int? contextWindow;
  final int? maxOutputTokens;
  final String currency;
  final double? inputPricePerMTok;
  final double? outputPricePerMTok;
  final double? cacheReadPricePerMTok;
  final String baseInstructions;

  ProviderModelView copyWith({
    String? slug,
    String? displayName,
    List<String>? reasoningEfforts,
    String? description,
    int? contextWindow,
    int? maxOutputTokens,
    String? currency,
    double? inputPricePerMTok,
    double? outputPricePerMTok,
    double? cacheReadPricePerMTok,
    String? baseInstructions,
  }) {
    return ProviderModelView(
      slug: slug ?? this.slug,
      displayName: displayName ?? this.displayName,
      reasoningEfforts: reasoningEfforts ?? this.reasoningEfforts,
      description: description ?? this.description,
      contextWindow: contextWindow ?? this.contextWindow,
      maxOutputTokens: maxOutputTokens ?? this.maxOutputTokens,
      currency: currency ?? this.currency,
      inputPricePerMTok: inputPricePerMTok ?? this.inputPricePerMTok,
      outputPricePerMTok: outputPricePerMTok ?? this.outputPricePerMTok,
      cacheReadPricePerMTok:
          cacheReadPricePerMTok ?? this.cacheReadPricePerMTok,
      baseInstructions: baseInstructions ?? this.baseInstructions,
    );
  }
}

class ProviderSettingsView {
  const ProviderSettingsView({
    required this.id,
    this.templateKind = 'openai',
    required this.name,
    this.subtitle = '',
    required this.baseUrl,
    this.bearerToken = '',
    this.hasBearerToken = false,
    required this.defaultModel,
    required this.models,
    this.defaultModels = const [],
    this.customModels = const [],
    required this.status,
    required this.usageLabel,
    this.modelCount = '',
    this.updatedAt = '',
    this.providerKind = '',
  });

  final String id;
  final String templateKind;
  final String name;
  final String subtitle;
  final String baseUrl;
  final String bearerToken;
  final bool hasBearerToken;
  final String defaultModel;
  final List<ProviderModelView> models;
  final List<ProviderModelView> defaultModels;
  final List<ProviderModelView> customModels;
  final String status;
  final String usageLabel;
  final String modelCount;
  final String updatedAt;
  final String providerKind;

  List<ProviderModelView> get allModels {
    if (models.isNotEmpty) {
      return models;
    }
    return [...defaultModels, ...customModels];
  }

  ProviderSettingsView copyWith({
    String? id,
    String? templateKind,
    String? name,
    String? subtitle,
    String? baseUrl,
    String? bearerToken,
    bool? hasBearerToken,
    String? defaultModel,
    List<ProviderModelView>? models,
    List<ProviderModelView>? defaultModels,
    List<ProviderModelView>? customModels,
    String? status,
    String? usageLabel,
    String? modelCount,
    String? updatedAt,
    String? providerKind,
  }) {
    return ProviderSettingsView(
      id: id ?? this.id,
      templateKind: templateKind ?? this.templateKind,
      name: name ?? this.name,
      subtitle: subtitle ?? this.subtitle,
      baseUrl: baseUrl ?? this.baseUrl,
      bearerToken: bearerToken ?? this.bearerToken,
      hasBearerToken: hasBearerToken ?? this.hasBearerToken,
      defaultModel: defaultModel ?? this.defaultModel,
      models: models ?? this.models,
      defaultModels: defaultModels ?? this.defaultModels,
      customModels: customModels ?? this.customModels,
      status: status ?? this.status,
      usageLabel: usageLabel ?? this.usageLabel,
      modelCount: modelCount ?? this.modelCount,
      updatedAt: updatedAt ?? this.updatedAt,
      providerKind: providerKind ?? this.providerKind,
    );
  }
}

class ProviderUsageView {
  const ProviderUsageView({
    required this.providerId,
    required this.updatedAt,
    required this.status,
    required this.usageKind,
    this.message,
    this.balance,
    this.codingPlan,
  });

  final String providerId;
  final int updatedAt;
  final String status;
  final String usageKind;
  final String? message;
  final DeepSeekBalanceUsageView? balance;
  final ZhipuCodingPlanUsageView? codingPlan;
}

class DeepSeekBalanceUsageView {
  const DeepSeekBalanceUsageView({
    required this.isAvailable,
    required this.balances,
  });

  final bool isAvailable;
  final List<DeepSeekBalanceInfoView> balances;
}

class DeepSeekBalanceInfoView {
  const DeepSeekBalanceInfoView({
    required this.currency,
    required this.totalBalance,
    required this.grantedBalance,
    required this.toppedUpBalance,
  });

  final String currency;
  final String totalBalance;
  final String grantedBalance;
  final String toppedUpBalance;
}

class ZhipuCodingPlanUsageView {
  const ZhipuCodingPlanUsageView({this.level, required this.limits});

  final String? level;
  final List<ZhipuQuotaLimitView> limits;
}

class ZhipuQuotaLimitView {
  const ZhipuQuotaLimitView({
    required this.window,
    required this.label,
    required this.percentage,
    this.currentValue,
    this.total,
    this.remaining,
    this.nextResetAt,
    required this.usageDetails,
  });

  final String window;
  final String label;
  final double percentage;
  final double? currentValue;
  final double? total;
  final double? remaining;
  final int? nextResetAt;
  final List<ZhipuToolUsageDetailView> usageDetails;
}

class ZhipuToolUsageDetailView {
  const ZhipuToolUsageDetailView({
    required this.name,
    this.currentValue,
    this.total,
    this.percentage,
  });

  final String name;
  final double? currentValue;
  final double? total;
  final double? percentage;
}

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

class StudioState {
  const StudioState({
    required this.projects,
    required this.sessions,
    required this.messagesBySession,
    required this.providers,
    this.providerUsages = const [],
    required this.roles,
    required this.mcpServers,
    this.instructions = const InstructionsSettingsView(),
    this.skills = const SkillsSettingsView(),
    this.general = const GeneralSettingsView(),
    required this.selectedProjectId,
    required this.selectedSessionId,
    required this.permissionMode,
    required this.turnPhase,
    required this.runtime,
    required this.pendingInteractions,
    this.eventCursorsBySession = const {},
    this.composerText = '',
  });

  final List<StudioProject> projects;
  final List<StudioSession> sessions;
  final Map<String, List<TimelineMessage>> messagesBySession;
  final List<ProviderSettingsView> providers;
  final List<ProviderUsageView> providerUsages;
  final List<RoleSettingsView> roles;
  final List<McpServerSettingsView> mcpServers;
  final InstructionsSettingsView instructions;
  final SkillsSettingsView skills;
  final GeneralSettingsView general;
  final String? selectedProjectId;
  final String? selectedSessionId;
  final PermissionMode permissionMode;
  final TurnPhase turnPhase;
  final SessionRuntimeView runtime;
  final List<PendingInteraction> pendingInteractions;
  final Map<String, int> eventCursorsBySession;
  final String composerText;

  List<TimelineMessage> get selectedMessages {
    final sessionId = selectedSessionId;
    if (sessionId == null) {
      return const [];
    }
    return messagesBySession[sessionId] ?? const [];
  }

  PendingInteraction? get activeInteraction {
    final sessionId = selectedSessionId;
    if (sessionId == null) {
      return null;
    }
    final scoped = pendingInteractions
        .where((interaction) => interaction.sessionId == sessionId)
        .toList();
    scoped.sort(
      (a, b) =>
          interactionPriority(a.kind).compareTo(interactionPriority(b.kind)),
    );
    return scoped.firstOrNull;
  }

  RoleSettingsView? role(String key) {
    return roles.where((role) => role.key == key).firstOrNull;
  }

  bool get isBusy {
    return switch (turnPhase) {
      TurnPhase.queued ||
      TurnPhase.contextLoading ||
      TurnPhase.waitingForModel ||
      TurnPhase.streaming ||
      TurnPhase.waitingForInteraction ||
      TurnPhase.runningTool => true,
      TurnPhase.idle ||
      TurnPhase.completed ||
      TurnPhase.failed ||
      TurnPhase.cancelled => false,
    };
  }

  StudioState copyWith({
    List<StudioProject>? projects,
    List<StudioSession>? sessions,
    Map<String, List<TimelineMessage>>? messagesBySession,
    List<ProviderSettingsView>? providers,
    List<ProviderUsageView>? providerUsages,
    List<RoleSettingsView>? roles,
    List<McpServerSettingsView>? mcpServers,
    InstructionsSettingsView? instructions,
    SkillsSettingsView? skills,
    GeneralSettingsView? general,
    String? selectedProjectId,
    String? selectedSessionId,
    PermissionMode? permissionMode,
    TurnPhase? turnPhase,
    SessionRuntimeView? runtime,
    List<PendingInteraction>? pendingInteractions,
    Map<String, int>? eventCursorsBySession,
    String? composerText,
  }) {
    return StudioState(
      projects: projects ?? this.projects,
      sessions: sessions ?? this.sessions,
      messagesBySession: messagesBySession ?? this.messagesBySession,
      providers: providers ?? this.providers,
      providerUsages: providerUsages ?? this.providerUsages,
      roles: roles ?? this.roles,
      mcpServers: mcpServers ?? this.mcpServers,
      instructions: instructions ?? this.instructions,
      skills: skills ?? this.skills,
      general: general ?? this.general,
      selectedProjectId: selectedProjectId ?? this.selectedProjectId,
      selectedSessionId: selectedSessionId ?? this.selectedSessionId,
      permissionMode: permissionMode ?? this.permissionMode,
      turnPhase: turnPhase ?? this.turnPhase,
      runtime: runtime ?? this.runtime,
      pendingInteractions: pendingInteractions ?? this.pendingInteractions,
      eventCursorsBySession:
          eventCursorsBySession ?? this.eventCursorsBySession,
      composerText: composerText ?? this.composerText,
    );
  }
}

int interactionPriority(InteractionKind kind) {
  return switch (kind) {
    InteractionKind.toolApproval => 0,
    InteractionKind.userInput => 1,
    InteractionKind.planConfirmation => 2,
  };
}

extension FirstOrNull<T> on Iterable<T> {
  T? get firstOrNull {
    final iterator = this.iterator;
    if (iterator.moveNext()) {
      return iterator.current;
    }
    return null;
  }
}
