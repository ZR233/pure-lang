/// Agent Profile 的只读 UI 快照。
class AgentProfileView {
  const AgentProfileView({
    required this.id,
    required this.displayName,
    required this.description,
    required this.whenToUse,
    required this.systemInstructions,
    required this.providerId,
    required this.model,
    required this.effort,
    required this.source,
    required this.revision,
    required this.contentHash,
    required this.system,
    required this.enabled,
  });

  final String id;
  final String displayName;
  final String description;
  final String whenToUse;
  final String systemInstructions;
  final String providerId;
  final String model;
  final String? effort;
  final String source;
  final String revision;
  final String contentHash;
  final bool system;
  final bool enabled;
}

class AgentProfileDraft {
  const AgentProfileDraft({
    required this.id,
    required this.displayName,
    required this.description,
    required this.whenToUse,
    required this.systemInstructions,
    required this.providerId,
    required this.model,
    this.effort,
    this.enabled = true,
  });

  factory AgentProfileDraft.fromView(AgentProfileView profile) =>
      AgentProfileDraft(
        id: profile.id,
        displayName: profile.displayName,
        description: profile.description,
        whenToUse: profile.whenToUse,
        systemInstructions: profile.systemInstructions,
        providerId: profile.providerId,
        model: profile.model,
        effort: profile.effort,
        enabled: profile.enabled,
      );

  final String id;
  final String displayName;
  final String description;
  final String whenToUse;
  final String systemInstructions;
  final String providerId;
  final String model;
  final String? effort;
  final bool enabled;
}
