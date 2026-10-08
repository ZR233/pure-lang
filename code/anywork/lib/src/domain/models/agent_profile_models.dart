enum AgentWorkspaceMode { unrestricted, directory, worktree }

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
    required this.workspaceMode,
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
  final AgentWorkspaceMode workspaceMode;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is AgentProfileView &&
          id == other.id &&
          displayName == other.displayName &&
          description == other.description &&
          whenToUse == other.whenToUse &&
          systemInstructions == other.systemInstructions &&
          providerId == other.providerId &&
          model == other.model &&
          effort == other.effort &&
          source == other.source &&
          revision == other.revision &&
          contentHash == other.contentHash &&
          system == other.system &&
          enabled == other.enabled &&
          workspaceMode == other.workspaceMode;

  @override
  int get hashCode => Object.hash(
    id,
    displayName,
    description,
    whenToUse,
    systemInstructions,
    providerId,
    model,
    effort,
    source,
    revision,
    contentHash,
    system,
    enabled,
    workspaceMode,
  );
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
    this.workspaceMode = AgentWorkspaceMode.directory,
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
        workspaceMode: profile.workspaceMode,
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
  final AgentWorkspaceMode workspaceMode;
}
